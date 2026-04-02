# Claude Insight

Local observability and audit system for Claude Code. Evidence-chain TUI, not a reasoning-extraction system. Lazygit energy for AI agent sessions.

## Repo layout

```text
claude-insight/
├── src/
│   ├── main.rs        # CLI entry point (clap)
│   ├── daemon/        # axum HTTP server (hook event receiver)
│   ├── storage/       # rusqlite + JSONL storage layer
│   ├── tui/           # Ratatui three-pane replay UI
│   └── models/        # Shared types (serde)
├── tests/             # Integration tests
├── scripts/           # Utility scripts
├── Cargo.toml         # Rust project config
├── WORKFLOW.md        # Symphony orchestration config
└── .codex/            # Codex agent skills
```

## Quick reference

```bash
# Build
cargo build

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings

# Format check
cargo fmt --check

# Format fix
cargo fmt

# Run locally
cargo run -- serve
cargo run -- trace <session-id>
```

## Conventions

- Rust 2021 edition, stable toolchain
- Error handling: `thiserror` for library errors, `anyhow` for CLI/binary
- No `unwrap()`/`expect()` in production paths
- Commit format: `type(scope): short summary`
- Branch naming: `<issue-key>-short-description` (e.g., `CI-42-add-fts-search`)
- PR labels: `symphony` for agent-created PRs
- Zero clippy warnings policy

## Read first

- This file (CLAUDE.md)
- WORKFLOW.md (Symphony orchestration)
- Cargo.toml (dependencies and features)
