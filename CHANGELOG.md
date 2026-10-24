# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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
