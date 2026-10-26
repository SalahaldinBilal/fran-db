# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Each database (or schema on PostgreSQL) shows its total size next to its name in the tree.
- Query results that can be edited have an Add row button, including results from an empty table.
- Query results from Cloudflare D1 can be edited when the query reads one table and lists its columns by name or with `*`.

### Changed

- The separate table data view is gone. Double-clicking a table, or Query table in its menu, opens a query tab with a SELECT for it and runs it. If that table already has a tab whose SELECT you haven't changed, that tab comes to the front instead. Settings > Results has a switch to open the SELECT without running it.

## [0.1.2] - 2026-10-25

### Added

- Port forwarding for SSH tunnel connections. The cable button on a connection card, or in the schema browser header once connected, opens a dialog where you pick a local port. While the forward runs, other programs on your machine reach the database at `127.0.0.1:<port>` through the tunnel, with or without a session open. Stop it from the same dialog. The port you used last is remembered per connection, but a forward never starts on its own.
- SQLite databases. Open a database file, optionally read only, browse and edit its tables, and run queries. Attached databases show in the tree next to `main`. Foreign keys are enforced by default, and you can turn that off per connection. When a change fails because an index or trigger calls a function that only the app that created the database has, the error names that index or trigger and explains why.
- Cloudflare D1 databases, through Cloudflare's API with an account ID and an API token. Browse, query and edit them like SQLite, and switch to the account's other D1 databases without reconnecting. D1 doesn't allow BEGIN and COMMIT, so saved edits are sent as one batch that applies all together or not at all.
- The table designer works with SQLite tables. It can rename tables and columns, add and drop columns, and change indexes. SQLite can't change a column's type, NULL setting or default in place, so the designer tells you when a change would mean rebuilding the table.
- Connections can use a custom color, picked with the eyedropper swatch after the presets.
- A connection's color can tint its whole tab and connection card instead of just the dot and icon.

## [0.1.1] - 2026-10-24

### Added

- SSH tunnels can log in with your SSH agent. It uses `SSH_AUTH_SOCK` on macOS and Linux, and the OpenSSH agent or Pageant on Windows. Agents with their own socket, like 1Password, can be set per connection.
- Nix flake with a dev shell and a Linux package. Nix installs say when an update is out but leave installing it to Nix.

### Changed

- The tabs now sit in the window's title bar, with the window buttons next to them. There's always some empty space before the buttons for dragging the window, and double-clicking it maximizes. On macOS the usual window buttons stay where they are.
- Opening Fran DB while it's already running brings the open window to the front instead of starting a second copy.

### Fixed

- The app no longer opens to a blank white window while it starts. A small loading window shows instead, and the app replaces it once it's ready. You can drag the loading window around, and closing it quits the app.

## [0.1.0] - 2026-10-23

- Initial release
