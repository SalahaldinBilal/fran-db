# Fran DB

A desktop SQL client for MySQL, MariaDB, PostgreSQL, SQLite and Cloudflare D1, built with [Tauri 2](https://tauri.app/) and [SolidJS](https://www.solidjs.com/).

## Download

| Platform | Download |
| --- | --- |
| Windows (x64) | [Installer](https://frandb.won.fyi/fran-db_latest_x64-setup.exe) |
| macOS (Apple Silicon and Intel) | [Disk image](https://frandb.won.fyi/fran-db_latest_universal.dmg) |
| Linux (x64) | [AppImage](https://frandb.won.fyi/fran-db_latest_amd64.AppImage) |

Older versions are on the [releases page](../../releases). The app checks for updates when it starts, and Settings > Updates installs them.

The macOS build isn't signed, so macOS blocks it the first time. Open System Settings > Privacy & Security and click Open Anyway. On Linux, make the AppImage executable with `chmod +x` before running it.

### NixOS

Add the flake to your system flake's inputs:

```nix
inputs.fran-db = {
  url = "github:SalahaldinBilal/fran-db";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Then add the package to your configuration, with `inputs` passed in through `specialArgs`:

```nix
environment.systemPackages = [
  inputs.fran-db.packages.${pkgs.stdenv.hostPlatform.system}.default
];
```

Rebuild with `nixos-rebuild switch`. To update, run `nix flake update fran-db` and rebuild. The app tells you when a new version is out.

Every release is built for x86_64 and aarch64 Linux and pushed to a binary cache. Add it so Nix downloads the build instead of compiling it:

```nix
nix.settings = {
  extra-substituters = ["https://cache.kirsi.dev"];
  extra-trusted-public-keys = ["cache.kirsi.dev-1:kobyAUhAcq9OMNdNJCX6R0zz5GZ7OGv8SP7uJxcH8Cw="];
};
```

## Features

**Connections**
- TCP, a Unix socket, a Windows named pipe (MySQL only), or TCP through an SSH tunnel with a password, a key file or your SSH agent
- SSL modes from "if available" up to full certificate and host name checks
- SSH host keys are checked against the ones you've trusted before, and a new or changed key asks first
- Passwords and key passphrases are kept in an encrypted file whose key lives in the OS keychain
- On PostgreSQL, switch to another database in the same connection without logging in again

**Queries**
- SQL editor with completion from the connected schema, and warnings for unknown tables and columns
- Run the statement under the cursor or the whole script, and cancel a query while it runs
- Each query tab has its own connection, so session variables and transactions stay in that tab
- Saved queries, and open tabs come back the next time you connect

**Data**
- Browse tables page by page and search across every column
- Filter query results, and edit them when they come from a single table with its primary key
- Edit cells in place or in a viewer made for dates, numbers, JSON, binary and ENUM/SET values
- Every change shows the SQL it will run first, and row edits run in one transaction

**Structure**
- Table structure, DDL and ready-made SELECT, INSERT, UPDATE and DELETE statements
- Table designer for columns, indexes and foreign keys that writes the ALTER TABLE for you to review

## Your data

Everything is stored in the app data folder:

- Windows: `%APPDATA%\com.frandb.app`
- macOS: `~/Library/Application Support/com.frandb.app`
- Linux: `~/.local/share/com.frandb.app`

Connection profiles, saved queries, open tabs and settings are JSON files there. Passwords only go in `secrets.vault`, which can't be read without the key in the OS keychain.

## Development

Needs Node 24 and a stable Rust toolchain, plus [Tauri's system dependencies](https://tauri.app/start/prerequisites/) for your OS. With Nix, `nix develop` gives a shell with all of them.

```sh
npm install
npm run tauri dev
```

This starts the Vite dev server and the Rust backend together. `npm run dev` runs the frontend alone, without the Tauri APIs.

After changing `app-icon.svg`, regenerate the icons with `npm run tauri icon app-icon.svg`.

### Tests

Inside `src-tauri/`:

```sh
cargo test                              # unit tests
cargo test --features e2e --test e2e    # end-to-end tests against real databases
```

The end-to-end tests start MySQL and PostgreSQL in Docker, run every test against each, and remove the containers afterwards. Add a filter to run part of them, like `-- postgres_17`.

On Windows without Docker Desktop, Docker can run inside WSL instead. Have dockerd also listen on `tcp://127.0.0.1:2375` and set `DOCKER_HOST=tcp://127.0.0.1:2375`. The tests start the WSL distro if it isn't running. To use a database you already have instead of a container, set `FRAN_E2E_<SERVER_ID>=host:port:user:password`, for example `FRAN_E2E_POSTGRES_17`.

### Releasing

1. Add the user-facing changes under `## [Unreleased]` in `CHANGELOG.md`.
2. Run `npm run version:bump <version>`, which sets the version everywhere and dates the changelog section.
3. Commit, tag `v<version>` and push the tag.

The release workflow builds all three platforms, uploads the installers, publishes the GitHub release, and points the in-app updater at the new version.
