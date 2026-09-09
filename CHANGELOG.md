# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

dbM2 is a from-scratch rewrite of its predecessor `dbm`. This first public
release is the TEA-architecture terminal client for PostgreSQL.

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
- **Security**: credentials encrypted at rest (AES-256-GCM, local `master.key`,
  data dir `0700`), secrets zeroized after use.
- **Session restore**: tabs, buffers, tree expansion and layout persisted to the
  local SQLite store and restored on next launch.
- **CI**: fmt/clippy checks plus a macOS/Linux/Windows test matrix and
  `cargo audit`.
