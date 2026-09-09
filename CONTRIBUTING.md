# Contributing to dbm2

Thanks for your interest. This document covers how to build, test, and submit
changes. For questions or proposals, open an issue first so we can align before
you invest in a large diff.

## Development setup

```sh
# Requires Rust stable (see rust-toolchain.toml)
cargo build --release --workspace   # produces target/release/dbm
cargo run -p dbm-cli -- i           # start the interactive TUI

# Quality gates (all enforced by CI)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Architecture notes — read before changing code

dbm2 follows a strict **TEA (The Elm Architecture)** split, and the codebase
enforces it. Please read [`docs/tea-architecture-principles.md`](docs/tea-architecture-principles.md)
first; the short version:

- State changes happen **only** in `update` handlers (`msg → (state, effects)`).
  `input` layers (key/mouse) are read-only decoders; `view` is pure
  `state → view`.
- Feature modules under `crates/dbm-tui2/src/features/` are self-contained
  (their own `state`/`update`/`view`/`input`); application routing lives in
  `app/`. `app_shell/` is a framework layer with no business logic.
- Layout and hit-testing must mirror between input and view — when you change
  geometry in the renderer, update the matching hit-test helpers in `input`.

The other TEA documents are referenced from the README.

## Conventions

- **Formatting/lints**: `cargo fmt` output and zero clippy warnings
  (`-D warnings`) are required.
- **Comments**: write them in English.
- **Tests**: new behavior ships with tests, placed next to the code under test.
- **`third_party/edtui`**: avoid structural changes. Any patch must be
  documented in `third_party/edtui/PATCH.md`.
- **Commits**: use [Conventional Commits](https://www.conventionalcommits.org/)
  (e.g. `fix(results): …`, `feat(detail): …`), one logical change per commit,
  with a body explaining the *why* when non-obvious.

## Pull requests

- Keep PRs small and focused; describe what and why.
- The CI matrix (fmt, clippy, tests on macOS/Linux/Windows, cargo audit) must be
  green before merging.
- If your change alters visible behavior or geometry, mention it in the PR
  description so reviewers can test the TUI flows.
