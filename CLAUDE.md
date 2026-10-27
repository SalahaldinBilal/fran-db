# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Project

Fran DB is a desktop SQL client built with Tauri 2 (Rust backend) and SolidJS + TypeScript (frontend). MySQL, PostgreSQL, SQLite and Cloudflare D1 are supported. Every database-specific piece sits behind a per-kind module so another database can be added without touching generic code.

## Commands

- `npm run tauri dev` runs the full app (Vite on port 1430 plus the Rust backend)
- `npm run dev` runs the frontend alone (Tauri APIs unavailable)
- `npm run tauri build` makes a release bundle
- `npm run tauri icon app-icon.svg` regenerates `src-tauri/icons/` from the SVG
- `cargo check` / `cargo test` inside `src-tauri/` compile or test the Rust side (the statement splitters and classifiers, the SQL generators and the Postgres type mapping have unit tests, `db/sqlite/session.rs` runs the SQLite driver end to end against a temp file, and `agent/tests.rs` drives the MCP server over HTTP with an MCP client)
- `cargo test --features e2e --test e2e` inside `src-tauri/` runs the driver end-to-end tests (`tests/e2e/`) against real servers in Docker, started and removed by the test run. A filter like `-- postgres_17` limits the run. `DOCKER_HOST=tcp://127.0.0.1:2375` points it at dockerd inside WSL, `FRAN_E2E_<SERVER_ID>=host:port:user:password` at an existing server instead of a container

No frontend tests or linters are configured.

## Releases

- User-facing changes go under `## [Unreleased]` in `CHANGELOG.md` (Keep a Changelog, `### Added` / `### Changed` / `### Fixed` with `- ` bullets). Those notes are shown in the app's update panel.
- `npm run version:bump <version>` sets the version in `package.json`, `src-tauri/Cargo.toml` and `tauri.conf.json`, and turns the Unreleased section into a dated one. Commit, then push a `v<version>` tag.
- The tag runs `.github/workflows/release.yml`: it checks the tag against `tauri.conf.json`, builds Windows (NSIS), macOS (universal, unsigned) and Linux (AppImage), then `scripts/deploy.ts` uploads to the `frandb-files` B2 bucket (served at `frandb.won.fyi`), creates the GitHub release with the installers attached, and writes `updater/latest.json` last. The `nix` job runs alongside the builds: it builds the flake on x86_64 and aarch64 Linux and pushes it to the `cache.kirsi.dev` niks3 cache. The flake takes its versions from `package.json` and `Cargo.toml`, so `version:bump` covers it. Once the deploy and the nix build both succeed, the `stable` branch is pushed to the tagged commit.
- The in-app updater (`states/updaterState.ts`, Settings > Updates) reads that manifest. Update files are signed with the `TAURI_SIGNING_PRIVATE_KEY` secret, which must match `plugins.updater.pubkey`.
- Repo secrets: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `B2_APPLICATION_KEY_ID`, `B2_APPLICATION_KEY`.

## Architecture

### Database independence

- Rust: generic code only talks to the `DatabaseSession` trait (`src-tauri/src/db/session.rs`). `DriverOptions` (`db/mod.rs`) is a `kind`/`options` tagged enum, and the `match` arms in that file are the only place that switches on kind (connect, required secrets, statement splitting).
- All MySQL code lives in `src-tauri/src/db/mysql/`: options, connect + SSL modes, metadata via `information_schema`, result reading, the statement splitter (quotes, all comment styles, `/*! */`, `DELIMITER`), identifier quoting.
- All Postgres code lives in `src-tauri/src/db/postgres/`. It has its own small wire client (`client/`) on `postgres-protocol`, because tokio-postgres drops the column details of text-mode results. User SQL runs one statement at a time with the extended protocol (Parse, Bind, Describe portal, Execute with a row cap, Sync) and text results. The portal's RowDescription gives each column's type OID, table OID and column number. The driver's own catalog queries use the extended protocol with text parameters and text results. Type OIDs resolve through `types.rs` (built-ins inline, the rest cached from `pg_type`), and `relations.rs` maps table OID + column number back to schema, table, column, NOT NULL and primary key. Metadata reads `pg_catalog`, not `information_schema`. `ddl.rs` rebuilds CREATE TABLE from the catalog since Postgres has no SHOW CREATE TABLE. The splitter handles `$tag$` quotes, `E''` strings, nested comments and `BEGIN ATOMIC` bodies. `design.rs` is the Postgres table designer: renames and comments are separate statements, indexes are `CREATE INDEX` / `DROP INDEX` unless they back a constraint, AUTO_INCREMENT maps to an identity column, and existing columns can't be reordered. Expression, partial and exclusion indexes are left out of the design so the designer never touches them.
- All SQLite code lives in `src-tauri/src/db/sqlite/`, split into a core and a backend. The core (`session.rs`, `metadata.rs`, `results.rs`, `sqlgen.rs`, `design.rs`, `splitter.rs`) only talks to the `SqliteConnection` and `SqliteConnector` traits in `connection.rs`: run one statement with bound parameters and get columns, rows as `SqlValue`, the change count and the inserted rowid. `local.rs` is the file backend on rusqlite (bundled SQLite, with blocking calls on `spawn_blocking`). Each query tab gets its own connection, so transactions stay per tab. Metadata comes from `sqlite_master` and plain PRAGMA statements (`table_xinfo`, `index_list`, `index_info`, `foreign_key_list`), not the `pragma_*` table functions D1 doesn't document, and `definition.rs` reads generated expressions, collations and AUTOINCREMENT from the stored CREATE TABLE text because no pragma reports them. Result columns get their source table from SQLite's column metadata. When a backend can't report that, `source.rs` reads the table from the statement if it's a plain single-table SELECT whose columns are `*` or bare names (no aliases, since columns are matched by name), so those results stay editable. The tree's top level is `main` plus attached databases. The designer only does what ALTER TABLE can (rename the table, rename/add/drop columns) plus standalone indexes. Anything else is refused with an explanation, because a rebuild drops the old table, and with foreign keys on that fires ON DELETE actions on tables that reference it.
- Cloudflare D1 (`d1.rs`) is a second backend for the same core: its own `DriverOptions` variant (`D1Options`: account ID, database name or UUID, an `apiToken` secret slot) and `src/databases/d1/`, which spreads the SQLite frontend module. Each statement is one request to the `/raw` endpoint. Parameters must be text, rows come back as JSON (blobs as byte arrays) with column names only, so only plain single-table SELECTs are editable (see `source.rs` above). BEGIN and COMMIT are refused, so `run_batch` sends a `batch` request, which D1 runs as one transaction without saying which statement failed. `_cf_` tables are hidden, `list_catalogs` / `switch_catalog` move between the account's databases (`SqliteConnector::with_catalog`), and `sqlgen` keeps each generated statement under D1's 100 KB limit.
- `DesignOptions.capabilities` tells the designer UI which fields the server has (engine, charset, column collation, ON UPDATE, index prefix lengths, reordering, virtual generated columns), and `indexMethods` fills the index method picker.
- Postgres sessions work in one database, and the tree's top level holds its schemas (`DatabaseModule.namespace`). `list_catalogs` / `switch_catalog` move a session to another database in place, reusing its login and tunnel. Open tabs are saved per database (`catalog`).
- Frontend: `src/databases/` holds a `DatabaseModule` per kind (connection form, defaults, CodeMirror dialect, SQL helpers). Use `withDatabaseModule(profile, (module, options) => ...)` to keep kind and options paired. Never branch on `kind === "mysql"` in generic UI.
- Values cross IPC as text (`Option<String>`) with a `ValueCategory` per column for rendering. Binary is `0x` hex, BIT is decimal.
- Adding a database: a Rust module implementing `DatabaseSession`, a `DriverOptions` variant, a frontend `DatabaseModule`, and the matching TS option types in `src/types/connection.ts`.

### Sessions and contexts

- `open_session` connects and registers an `OpenSession` in `SessionManager`. Metadata calls share one connection per session. Each query tab gets its own connection ("context", `open_query_context`) so session variables and transactions stay per tab. Contexts reconnect after a dropped connection and ping after a minute idle.
- `execute_query` splits the SQL with the kind's splitter, runs statements in order, and stops at the first failure, which is reported on that statement's outcome rather than as a command error. Each outcome goes to the tab over a `Channel` as soon as its statement finishes (drivers implement `execute_streaming`). The command returns how many it sent, because channel messages can arrive after the reply.
- The tab's row limit (`QueryTabState.rowLimit`, starting from the `defaultRowLimit` setting, 0 for none) is enforced on the server, so it plans and stops as it would for a LIMIT. MySQL sets `sql_select_limit` before each run. Postgres and SQLite (both backends) append `LIMIT` to reads whose top level has no LIMIT (or FETCH on Postgres) and don't write (`classify::with_row_limit`), and Postgres also passes the limit as Execute's row cap for statements that can't take one. Each asks for one row more than the limit so the result can say it was cut off.
- Cancel (`cancel_query`) opens a short side connection: MySQL runs `KILL QUERY <id>`, Postgres sends a CancelRequest with the context's key. SQLite interrupts the context's connection in place.

### Transports

- TCP, Unix socket, Windows named pipe (MySQL only), TCP over SSH.
- mysql_async has named pipes switched off, so on Windows the pipe is relayed through `LocalForward` (`src-tauri/src/forward.rs`), a 127.0.0.1 port that opens the pipe per connection. The SSH tunnel uses the same relay with `direct-tcpip` channels.
- SSH logs in with a password, a key file, or the SSH agent (`SshAuthMethod::Agent`, which offers each agent key in turn). The agent is `SSH_AUTH_SOCK` on Unix and the OpenSSH pipe, then Pageant, on Windows. `agentSocket` overrides it.
- SSH host keys are checked against `known_hosts.json` in the app data dir. An unknown or changed key fails the connect with `unknownHostKey` / `hostKeyChanged` carrying the SHA256 fingerprint. The frontend asks the user and calls `trust_host_key`, then retries.

### Secrets

- `Vault` (`src-tauri/src/vault/`) keeps every secret in `secrets.vault`, encrypted with XChaCha20-Poly1305 (header bytes as AAD). The 32-byte key lives only in the OS keychain (`keyring`, service `com.frandb.app`).
- Secret ids are `connection:<id>:<slot>`. Slots are declared by the driver (`password`, `sshPassword`, `sshKeyPassphrase`).
- Secrets go frontend to Rust and never back. The UI only sees which slots are stored (`get_stored_secrets`).
- A missing required secret returns `secretsRequired`. `withConnectPrompts` (`src/helpers/connectFlow.ts`) prompts, optionally saves to the vault, and retries. Use it for every connect-like call.
- If the keychain is unavailable the vault reports it. Never fall back to a weaker store.

### Editing and DDL

- All SQL that the app writes for the user is generated in the driver (`sqlgen.rs`, `design.rs` in each driver module) and shown in `ReviewSqlModal` before it runs. Its `safety` note says whether a failure rolls back: row changes run in a transaction. DDL does too when `ServerInfo.transactionalDdl` is set (Postgres). Otherwise `ServerInfo.atomicDdl` (set by the driver from the server version) says whether each DDL statement is all or nothing. Literals respect the session's quoting rules (`NO_BACKSLASH_ESCAPES` on MySQL, `standard_conforming_strings` on Postgres), read once at connect into `Dialect`.
- There's no separate table data view. Double-clicking a table, or Query table in its menu, calls `workspaceState.queryTable`: it opens a query tab with `selectRowsSql`, or focuses one whose text is still that SELECT (whitespace ignored), and runs it when the `runTableQueryOnOpen` setting is on (the tab's unsaved `runOnOpen` flag, so restored tabs don't rerun).
- Row editing (`helpers/rowEdits.ts`, used by query results in `QueryTab`) keeps edits client-side, sends `RowChange`s to `preview_row_changes`, then `execute_batch` runs them in one transaction on a fresh connection. Rows are matched by primary key, tables without one are read only. Query results are editable only when every column is a plain column (`sourceName`) of one table and that table's whole primary key (`get_primary_key`) is selected. SQL is written with `sourceName`, so aliases are fine. The check also runs on empty results, so "Add row" works on an empty table.
- Query results filter client-side (`ResultGrid` with `filter`), and the grid maps display positions to source rows so edits keep working.
- `ValueViewer` picks a view by column: dates and times (`views/temporal.ts` also reads pasted Unix time, ISO 8601 and other formats, zoned input becomes UTC), numbers, binary, JSON, text, and ENUM/SET values from `get_column_choices`. Whether a cell edits inline or in the viewer by default is the `cellEditMode` setting.
- The table designer edits a `TableDesign`. Every column, index and foreign key carries `originalName` so `alter_sql` can tell renames from drops and adds. FK drops and adds go in their own ALTER statements around the main one. `design.rs` has tests for the diff, extend them when changing it.
- "Copy SQL" templates come from `get_object_sql` / `get_database_sql`.

### Agents (MCP server)

- `src-tauri/src/agent/` serves MCP over streamable HTTP (`rmcp` behind `axum`) at `http://127.0.0.1:<port>/mcp`, off until turned on in the Agents tab or Settings > Agents. It binds to 127.0.0.1, accepts only loopback `Host` headers and refuses any request with an `Origin` header. No token.
- `AgentHost` (managed as `Arc<AgentHost>`) owns the server, agent sessions, pending approvals and history. It reaches the window through the `Notifier` trait, so tests run it without Tauri. Server settings and the global switches (`allowRawSql`, `allowChanges`, history) live in `agent.json`, owned by Rust.
- Access is per connection: `ConnectionProfile.agentAccess` with mode `off` (default), `readOnly` or `ask`, and an optional database list. Where a session is bound to one database (`DatabaseSession::catalog`, Postgres and D1) the list names those databases, elsewhere the ones tools take. Every call re-reads the profile, and saving or deleting a connection closes its agent sessions.
- Agents call `open_session` first and pass its id to every other tool. Agent sessions are their own `DatabaseSession`s, kept apart from the UI's `SessionManager`, with one read-only context per database. They close on `close_session`, from the Agents tab, after 10 minutes idle, when the server is turned off, and on exit.
- Reads run in read-only contexts (`open_context(.., ContextAccess::ReadOnly)`): MySQL sets `SET SESSION TRANSACTION READ ONLY` on every (re)connect, Postgres starts the connection with `default_transaction_read_only=on`, SQLite opens the file read only, and D1 has no such mode. Raw SQL is checked first by `DatabaseSession::classify`, built on each driver's splitter (`classify.rs` per driver, shared tokenizer in `db/classify.rs`). Structured reads are written by `DatabaseSession::read_sql` (`db/read_query.rs` plus each driver's `SqlWriter`), so agents rarely need raw SQL.
- Changes (`ask` mode) build their SQL with the same driver code as the UI (`row_changes_sql`, `table_design_sql`, `object_sql`), then wait in `Approvals` until the user answers in `AgentApprovalModal`. The request carries the call's arguments, and row tools add a `RowPreview` (current values read by key next to the new ones). A new request opens the review once. Closing it leaves the request pending in the Agents tab. No answer in 5 minutes rejects. Approved SQL runs with `execute_batch`.
- The UI is the Agents global tab (`pages/Agents/`), opened from the title bar button: Activity (open sessions, sessions closed that day from history, pending approvals and the day's calls), Access (every connection's `agentAccess`, saved with `save_connection`) and Server (`AgentConfig`). Settings > Agents only has the on switch.
- Every tool call goes through `AgentServer::call_tool`, which hides and refuses tools switched off globally (`tool_enabled`) and writes a `HistoryEntry` with the exact reply. History is one file per UTC day in `agent_history/`, each record encrypted with XChaCha20-Poly1305 under a key stored in the vault (`agent:historyKey`). A vault reset deletes it. With history on and the vault unavailable, the server doesn't start.
- Events to the frontend: `agent-status-changed`, `agent-sessions-changed`, `agent-approvals-changed`, `agent-history-added`, read by `states/agentState.ts`.

### Editor

- `SqlEditor` takes a `SchemaCatalog` (`helpers/catalog.ts`) for the schema checker (`schemaLinter.ts`) and a lang-sql namespace for completion. Both come from the session's cached `objects` and `columns`, loaded per database by `ensureCatalog` and refreshed by `refreshCatalog` after DDL.
- The checker walks lang-sql's syntax tree per statement. It must stay lenient: when any table in a statement can't be resolved (CTE, derived table, unloaded database) it skips unqualified column checks rather than guess.

### Files

All in the Tauri app data dir: `connections.json` (profiles, no secrets), `known_hosts.json`, `saved_queries.json`, `tab_state.json`, `settings.json`, `secrets.vault`, `agent.json`, `agent_history/`.

App settings belong to the frontend (`settingsState.ts` owns the shape and defaults, Rust only stores the JSON). Unknown or missing keys fall back to the defaults in `fromStored`. The settings window opens from the gear at the right of the top tab bar.

Open tabs persist per connection in `tab_state.json`. The frontend owns the shape (`PersistedSession` in `workspaceState.ts`, versioned) and writes it about 400ms after any tab change or keystroke. Live query text lives in `queryTexts`, outside the store.

Saved queries and tabs are separate. Opening a saved query puts a copy of its SQL in a new tab, and nothing in a tab ever changes a saved query. Ctrl+S in a tab saves its text as a new saved query. Saved queries are only edited in `SavedQueryEditor`, from the Saved queries panel. `JsonStore` refuses to overwrite a file it failed to parse.

### Frontend

- Global state is `createRoot(useXStateInner)` singletons in `src/states/`. `workspaceState` holds the global tabs (Connections + one per open session) and, per session, a tab scope per database. Workspace and tab components stay mounted while hidden so editors keep their state.
- Every backend call goes through `safeInvoke` (`src/helpers/safeInvoke.ts`). When adding or changing a `#[tauri::command]`, register it in `lib.rs` and update the `Commands` map.
- Context menus go through `contextMenuState.openContextMenu(event, items)` (items can have `children` for submenus). The webview's native menu is suppressed globally in `helpers/globalContextMenu.ts`, text fields get a Cut/Copy/Paste menu there. Clipboard access goes through `helpers/clipboard.ts` (Tauri clipboard plugin). Tooltips are `Button`'s `tooltip` prop, or `ref={element => tooltip(element, () => text)}` (`components/Tooltip/Tooltip.tsx`) on any other element, shown by the one `TooltipHost`. Never use the native `title` attribute. Confirmations, text prompts, credential prompts and host key prompts go through `promptState`.
- Colors, radii, font sizes and shadows come from the tokens in `src/styles/global.scss`. Don't hardcode hex values in component styles except for syntax highlight colors.
- The UI should look polished and intentionally designed, not like a generic template. Avoid stock patterns such as an accent stripe on the active tab or on cards, but don't strip styling to get there.
- The `@core` alias maps to `src/`.
- There's no native title bar. The global `TabStrip` is the title bar (`dragRegion`), with `WindowControls` on Windows/Linux and the native traffic lights on macOS (`tauri.macos.conf.json`, which replaces the whole `windows` list, so keep both window entries in sync). Nothing may block dragging it: modal backdrops start below `--titlebar-height`, and while any `Modal` is open (`modalState`) the tabs and the settings button ignore the mouse so presses on them drag instead. Check the OS with `IS_MAC` (`helpers/platform.ts`, OS plugin).
- The main window starts hidden. A splash window (`public/splash.html`, `src-tauri/src/splash.rs`) shows until `App` calls `show_main_window` after its first paint.

## Conventions

- Default to no comments. One short line only for a non-obvious constraint.
- Rust structs crossing IPC use `#[serde(rename_all = "camelCase")]`. On tagged enums, struct variants with multi-word fields need their own `#[serde(rename_all = "camelCase")]`, the enum-level one only renames variant tags.
- Every derived reactive value in a Solid component is a `createMemo`, not a plain arrow function. This includes conditional props passed to components (`editing={cond ? a : b}`): Solid compiles those into a getter that creates a memo on each read, and reading it from an event handler logs "computations created outside a `createRoot`".
- Multi-line functions use `function` declarations, not `const` arrows.
- Application code never creates or alters schema in any datastore.
