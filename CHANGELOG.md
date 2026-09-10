# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-alpha.2] - 2026-09-10

### Added

- **macOS (Apple Silicon) and Windows (x64) builds** — release artifacts now
  cover four targets: static Linux musl (`x86_64`/`aarch64-unknown-linux-musl`),
  `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc`. macOS and Windows
  binaries are not code-signed yet, so the OS may warn on first launch.

### Changed

- The release workflow uses the current (non-deprecated) artifact actions.

## [0.1.0-alpha.1] - 2026-09-10

First tagged preview of dbm2, a from-scratch rewrite of its predecessor `dbm`.
This **alpha** release is the PostgreSQL foundation of a database lifecycle
management platform: discovery, instance & connection management, and terminal
data management.

### Added

- **Terminal UI** (`dbm i`) built on ratatui/crossterm with a strict
  TEA architecture: `input → msg → update → view`, feature-local
  `state`/`update`/`view` modules, and a business-free `app_shell` framework.
- **Explorer**: instance tree with lazy connection loading and a schema object
  tree; `Enter`/double-click opens connections, runs `SELECT *` on tables.
- **Instance workspace**: per-instance Overview/Connections tabs with inline
  connection forms and live prechecks before saving.
- **Discover modal**: loopback/TCP scans for local PostgreSQL instances,
  registration with precheck + force.
- **SQL workspace**: per-connection tabs with a vim-native editor (edtui),
  table/column completion, context picker, search, and history recall.
- **Results grid**: paging, cell search, column resizing, optimistic row-edit
  sessions (UPDATE/DELETE/INSERT), and a per-cell detail editor.
- **CLI**: unified `dbm` binary — `interact|i`, `ping`, `query`, `tables`,
  `discover scan|list`, `instance list|precheck|register|unregister`,
  `instance connection list|test|add|remove|update`.
- **TLS**: rustls-based TLS driven by the per-connection libpq `sslmode`
  (`disable`/`allow`/`prefer`/`require`/`verify-ca`/`verify-full`); `prefer`
  falls back to plaintext when the server has no TLS, `require`+ verify the
  server certificate against bundled Mozilla roots.
- **Static Linux builds**: the release workflow produces static Linux
  (`x86_64`/`aarch64-unknown-linux-musl`, no libc/OpenSSL dependency) archives
  with `.sha256` checksums.
- **Security**: credentials encrypted at rest (AES-256-GCM, local `master.key`,
  data dir `0700`), secrets zeroized after use.
- **Session restore**: tabs, buffers, tree expansion and layout persisted to the
  local SQLite store and restored on next launch.
- **CI**: fmt/clippy checks plus a macOS/Linux/Windows test matrix and
  `cargo audit`.
