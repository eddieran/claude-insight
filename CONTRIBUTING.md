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
./scripts/publish-crates.sh --validate
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

## Publishing Releases

The public distribution flow has three maintainer steps:

1. Publish the workspace crates to crates.io in dependency order:

```bash
./scripts/publish-crates.sh --validate
./scripts/publish-crates.sh --publish
```

`--validate` is safe for CI and local dry runs: it dry-runs each internal
`claude-insight*` crate and packages the top-level `claude-insight` crate until
those internals are already visible on crates.io. `--publish` performs the real
crates.io publish sequence and waits for each version to appear before
continuing.

2. Push the version tag to trigger the GitHub Release workflow:

```bash
git tag v0.1.0
git push origin v0.1.0
```

That workflow builds platform archives, emits `SHA256SUMS`, generates
`claude-insight.rb`, and runs the installed-binary smoke validation before
publishing the GitHub Release assets.

3. Update `eddieran/homebrew-tap` with the generated `claude-insight.rb`
   formula from the release assets once the tagged GitHub Release is live.

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
