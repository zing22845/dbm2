# dbM2

An interactive **PostgreSQL database manager for the terminal**: a fast TUI
(`dbm i`) backed by a scriptable CLI, built on ratatui/crossterm with a strict
TEA (The Elm Architecture) design. dbM2 is a from-scratch rewrite of
[dbm](https://github.com/zing22845/dbm) by the same author.

> **Work in progress** — pre-1.0. Expect rough edges and breaking changes.

## Highlights

- **Manage instances & connections from the terminal**: scan for local
  PostgreSQL servers (process/port discovery), register the ones you want to
  manage, and add user/password connections — every save runs a live precheck
  against the real server first.
- **Vim-native SQL editor** per tab, with syntax highlighting, table/column
  completion and a database/schema context picker.
- **Results grid** with paging, cell search, column resizing, optimistic
  row-edit sessions (UPDATE/DELETE/INSERT), and a per-cell detail editor.
- **Per-connection SQL history** with keyboard recall and a scrollable preview
  detail pane.
- **Secure by default**: credentials are encrypted at rest (AES-256-GCM under a
  local `master.key`) and zeroized after use; the data directory defaults to
  `0700`.
- **Cross-platform** terminal UI, CI-tested on macOS, Linux and Windows.

## Features

### Explorer

The left-hand tree keeps your instances (with their connections) and the
schema object tree (databases → schemas → tables/views/functions/…) both
visible. Hit `Enter` or double-click a connection to open a SQL workspace, or a
table to run `SELECT * FROM "schema"."table"` and jump straight to the results.

### Instance Workspace

Per-instance `Overview` / `Connections` tabs: inspect the instance (version,
endpoints, lifecycle), refresh, unregister, and manage its connections through
small inline forms. Editing an existing connection tracks dirty fields and
refuses to leave the pane while unsaved changes exist; leaving the password
empty keeps the stored one.

### Discover

A modal that scans loopback/TCP targets for PostgreSQL instances, shows
confidence and registration state, and registers them — with a confirmation
dialog and a warning-level precheck that `force` registration can bypass.

### SQL Workspace

One parent pane per active connection holding up to 9 query tabs:

- **Editor** — edtui-based buffer with `Insert`/`Normal`/`Visual` modes and vim
  motions, completion popups (`Shift+Tab` to force), a database/schema context
  picker, in-editor search, and history recall via `Ctrl+r`.
- **Results** — virtualized grid with pagination (50–1000 rows/page), vim-style
  paging/jumps, editable sessions that generate optimistic single-row DML, and a
  per-cell detail editor (Save with `Ctrl+s`, discard with `Ctrl+u`).
- **History** — newest-first, deduplicated per connection, with searchable
  list + detail preview; `Enter`/double-click applies an entry back to the
  editor.
- **Detail** — the SQL history/result-detail preview owns its vertical
  scrollbar and its own list/detail splitter.

### Shell & navigation

Tab between panes, move focus with `Ctrl+h/j/k/l`, drag or key-resize
splitters, click scrollbars, scroll with the wheel, copy/paste with the
platform shortcuts (`Cmd+C/V` on macOS, `Ctrl+C/V` elsewhere), toggle dark/light
themes, and quit with `Ctrl+D`. Opened tabs, editor buffers, tree expansion and
layout are persisted per session and restored on the next launch. A footer
perf readout (`fps` + wasted-redraw ratio) is always visible.

## CLI

A single `dbm` binary exposes both the TUI and the non-interactive commands:

```text
dbm interact | i        Interactive terminal UI (alias: dbm i)
dbm ping                Verify connectivity, print server version
dbm query "SQL"         Run a single statement
dbm tables              List tables in a schema
dbm discover scan|list  Scan for / list local PostgreSQL instances
dbm instance            list | precheck | register | unregister | connection
dbm instance connection list|test|add|remove|update
```

Connection targets are selected by `--url` (`DBM_DATABASE_URL`), or by
`--instance` + `--connection` against the local store.

```sh
# Non-interactive
dbm ping --url "postgres://user:pass@localhost/db"
dbm query --url "postgres://user:pass@localhost/db" "select version();"

# Discover → manage → add a connection → use it
dbm discover scan
dbm instance register <discovery-id> --name local
dbm instance connection add --instance local --name dev --user postgres
dbm i   # pick the connection in the tree and start typing SQL
```

## Installation

Build from source (Rust `stable`, MSRV 1.88):

```sh
cargo build --release --workspace
# binary at target/release/dbm
```

## Data & security

State lives in a local data directory (default `{executable}/data`, overridable
with `--data-dir` or `DBM_DATA_DIR`):

- `config.db` — SQLite store: instances, connections, discovery results, SQL
  history, TUI session state.
- `master.key` — 32-byte random AES-256 master key, created on first use.

Passwords are encrypted with AES-256-GCM (random nonce per record) and never
stored in plaintext; secrets are zeroized in memory after use.

## Architecture

dbM2 follows **TEA**: `input → msg → update → view`, with a single state
transition point and pure rendering. The application layer is a central message
router around an `AppState`; business features live under
`dbm-tui2/src/features/` (explorer, discover, instance workspace, sql
workspace) as self-contained nested state/update/view modules; the `app_shell`
layer is a framework that owns the event loop and pluggable intent/effect slots
with no business logic. Design notes live in [`docs/`](docs/):

- [`docs/tea-architecture-principles.md`](docs/tea-architecture-principles.md)
- [`docs/tea-feature-inventory.md`](docs/tea-feature-inventory.md)

### Workspace layout

| Crate | Purpose |
| --- | --- |
| `dbm-core` | Shared types and errors |
| `dbm-discovery` | Local PostgreSQL instance discovery |
| `dbm-driver-pg` | PostgreSQL driver |
| `dbm-store` | Local SQLite store (config + encrypted secrets) |
| `dbm-cli` | The unified `dbm` binary (CLI + TUI entry) |
| `dbm-tui2` | The terminal UI (this repo's main effort) |
| `third_party/edtui` | Vendored MIT editor component used by the SQL editor |

## Development

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs these on every push/PR — format and clippy on Linux, the test suite on
macOS, Linux and Windows.

## License

Licensed under the [Apache License, Version 2.0](LICENSE). The vendored
`third_party/edtui` retains its own MIT license. dbM2 is a rewrite of its
predecessor [dbm](https://github.com/zing22845/dbm) — both Apache-2.0, same
author.
