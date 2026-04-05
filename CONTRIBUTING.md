# Contributing To Claude Insight

Thanks for contributing to Claude Insight. This repository is a Rust workspace
for a local observability and replay tool for Claude Code sessions.

## Prerequisites

- Rust stable toolchain
- Cargo
- A Unix-like shell for the helper scripts and release workflow
- Optional: Claude Code installed locally if you want to exercise real hook
  flows or future end-to-end tests

## Development Setup

Clone the repository and build the workspace:

```bash
git clone git@github.com:eddieran/claude-insight.git
cd claude-insight
cargo build --workspace
```

Run the full workspace test suite:

```bash
cargo test --workspace
```

Recommended day-to-day validation before opening a PR:

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

Useful local commands:

```bash
cargo run -p claude-insight -- --help
cargo run -p claude-insight -- init --help
cargo run -p claude-insight -- trace --help
```

## Workspace Overview

```text
crates/
  capture/   ingest hooks, backlog, transcript tailing
  cli/       clap binary entry point
  daemon/    daemon lifecycle and process management
  storage/   SQLite schema, normalization, search, retention
  tui/       Ratatui views and replay interactions
  types/     shared hook and transcript schemas
tests/       integration fixtures and end-to-end coverage
docs/        design, engineering, and test-planning notes
```

## Testing Notes

The repository mixes unit tests, integration tests, and snapshot tests:

- `cargo test --workspace` runs the main suite
- snapshot coverage in `crates/tui` checks render output for key layouts
- fixture-based tests under `tests/fixtures/` keep hook and transcript parsing
  grounded in realistic payloads

If you touch the TUI, storage schema, or hook normalization paths, update or
regenerate the relevant tests in the same change.

## PR Process

1. Start from a fresh branch based on the latest `origin/main`.
2. Keep the change focused on one ticket or one coherent fix.
3. Run the local validation commands before pushing:
   - `cargo build --workspace`
   - `cargo test --workspace`
   - `cargo clippy --workspace -- -D warnings`
   - `cargo fmt --all -- --check`
4. Update docs and tests when behavior, commands, storage, or UI output changes.
5. Open a pull request with a concise description of:
   - the user-visible change
   - the architectural impact, if any
   - the validation you ran
6. Resolve review comments with code changes or explicit technical pushback on
   the thread.

Agent-authored PRs should carry the `symphony` label.

## Code Style

- Rust 2021 edition
- No `unwrap()` or `expect()` in production paths
- Prefer small, focused changes over broad refactors
- Keep capture-path work lightweight
- Preserve append-only semantics for raw JSONL and `raw_events`
- Keep SQLite access compatible with concurrent daemon writes and TUI reads

## Documentation

User-facing command behavior should be documented in:

- `README.md` for installation and quick start
- `ARCHITECTURE.md` for system design and data flow
- `CONTRIBUTING.md` for development workflow

If you change command names, default paths, storage artifacts, or TUI
interaction patterns, update the relevant document in the same PR.
