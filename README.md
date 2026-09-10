# dbm2

dbm2 is an integrated **database lifecycle management** tool for the terminal:
database discovery, instance lifecycle management, connection & data
management, and — on the roadmap — backup & recovery, monitoring and more,
across multiple database engines. It ships as a fast TUI (`dbm i`) plus a
scriptable CLI, built on ratatui/crossterm with a strict TEA (The Elm
Architecture) design. dbm2 is a from-scratch rewrite of its predecessor dbm by
the same author.

> **Alpha** — this `0.1.0-alpha` release realizes only the first slice of the
> vision (the PostgreSQL foundation: discovery, instance & connection
> management, SQL data management). Expect rough edges, breaking changes and a
> rapidly evolving feature set.

## Status & scope

**Currently implemented (all PostgreSQL):** local instance discovery and
registration, per-instance connections with live prechecks, a terminal SQL
workspace (vim-native editor, results grid with row editing, SQL history) and
matching CLI commands. The architecture deliberately isolates engines behind
the `dbm-discovery` / `dbm-driver-*` crates so more engines and lifecycle
capabilities can be added without reshaping the TUI.

**Planned next:** additional database engines, lifecycle operations
(start/stop/restart, upgrade, failover), backup & recovery, export, and
monitoring & parameter management — see [Roadmap](#roadmap).

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

## Roadmap

The long-term goal is full-lifecycle management across database engines:

- **More engines** — generalize the discovery/driver crates beyond PostgreSQL
  (MySQL, SQLite, …).
- **Instance lifecycle** — deploy, start/stop/restart, upgrade, primary/replica
  switch, and parameter management.
- **Backup & recovery** — schedule and run backups, restores, and point-in-time
  recovery.
- **Data management** — export/import, richer data editors, and monitoring
  metrics.

Roadmap items are tracked as GitHub issues; priorities reflect community
demand.

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

### Prebuilt binaries

Release artifacts are published for:

```text
dbm-<version>-x86_64-unknown-linux-musl.tar.gz    # static, no libc/OpenSSL dependency
dbm-<version>-aarch64-unknown-linux-musl.tar.gz   # static
dbm-<version>-aarch64-apple-darwin.tar.gz         # macOS (Apple Silicon)
dbm-<version>-x86_64-pc-windows-msvc.tar.gz       # Windows (64-bit)
```

Each archive contains the `dbm` binary plus `README.md`/`LICENSE` and ships
with a `.sha256` checksum.

> **Unsigned binaries** — the macOS and Windows builds are not code-signed yet,
> so the OS warns on first launch. To allow them:
>
> - **macOS**: right-click the `dbm` binary and choose *Open* (or run
>   `xattr -d com.apple.quarantine ./dbm`)
> - **Windows**: on the SmartScreen prompt choose *More info* → *Run anyway*

### Build from source

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

## Connections & TLS

Each connection carries a libpq-style `sslmode` (`disable`, `allow`, `prefer`,
`require`, `verify-ca`, `verify-full`), stored with the connection and honored
when connecting:

- `prefer` (the default) tries TLS and transparently falls back to plaintext
  when the server does not support it.
- `disable` connects without TLS.
- `require` / `verify-ca` / `verify-full` require TLS; the server certificate is
  always verified against the bundled Mozilla root set (`webpki-roots`), so a
  self-signed server certificate is currently rejected on these modes.

TLS is provided by `rustls` (ring backend), keeping static musl binaries free of
OpenSSL and system certificate-store dependencies.

## Architecture

dbm2 follows **TEA**: `input → msg → update → view`, with a single state
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

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for contribution guidelines.

## License

Licensed under the [Apache License, Version 2.0](LICENSE). The vendored
`third_party/edtui` retains its own MIT license. dbm2 is a rewrite of its
predecessor dbm — both Apache-2.0, same author.
