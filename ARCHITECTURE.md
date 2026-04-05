# Claude Insight Architecture

This document explains how Claude Insight captures Claude Code activity,
normalizes it into a local evidence graph, and serves that data back to the CLI
and TUI.

## System Overview

Claude Insight is built around a raw-first capture pipeline:

1. Claude Code hook scripts emit structured JSON events.
2. The local daemon accepts those events over HTTP on the capture port.
3. Each event is preserved in append-only raw storage first.
4. The storage layer normalizes that evidence into typed SQLite tables.
5. The CLI and TUI query the resulting local evidence graph for replay,
   filtering, search, and cleanup.

The system is intentionally local-first:

- hook capture stays on the developer machine
- state lives under `~/.claude-insight/` unless overridden
- the SQLite database uses WAL mode for concurrent reads while writes continue
- the backlog file provides a fallback path when the daemon is unavailable

## Data Flow

```text
┌─────────────────────┐
│ Claude Code         │
│ sessions + hooks    │
└─────────┬───────────┘
          │ hook payloads
          v
┌─────────────────────┐
│ Hook scripts        │
│ HTTP POST to daemon │
│ fallback to JSONL   │
└─────────┬───────────┘
          │ capture port (default 4180)
          v
┌─────────────────────┐
│ claude-insight      │
│ daemon              │
│ axum receiver       │
└─────────┬───────────┘
          │ append raw evidence
          v
┌─────────────────────────────────────────────────────────────┐
│ SQLite: raw_events                                          │
│ - append-only capture stream                                │
│ - FTS trigger updates                                       │
│ - backlog replay feeds the same store                       │
└─────────┬───────────────────────────────────────────────────┘
          │ normalize / rebuild
          v
┌─────────────────────────────────────────────────────────────┐
│ Typed SQLite tables                                         │
│ sessions, prompts, tool_invocations, permission_decisions,  │
│ instruction_loads, config_snapshots, event_links, FTS       │
└─────────┬───────────────────────────────────────────────────┘
          │ query path
          v
┌─────────────────────────────────────────────────────────────┐
│ TUI + CLI                                                   │
│ replay, trace, search, evidence panes, retention commands   │
│ conceptual API boundary / future API port for UI queries    │
└─────────────────────────────────────────────────────────────┘
```

The ticket language references a separate TUI API port. The current code on
`main` exposes the capture port and reads SQLite directly for CLI/TUI queries,
but the diagram keeps the intended UI query boundary explicit because it is part
of the documented system shape.

## Crate Structure

### `crates/types`

Shared Rust types for hook payloads and transcript entries. This crate gives
the rest of the workspace a single schema layer for deserialization and tests.

### `crates/capture`

The ingestion layer:

- axum routes for hook capture
- backlog writer and processor for daemon-down recovery
- transcript tailer for `~/.claude/projects/`

The receiver path is intentionally lightweight so capture stays cheap and does
not block Claude Code longer than necessary.

### `crates/daemon`

The daemon process owns:

- capture-port binding
- PID-file lifecycle management
- startup backlog replay
- transcript tailing loop
- graceful shutdown behavior

### `crates/storage`

The storage layer is the evidence graph backbone:

- schema creation and connection setup
- WAL configuration
- append-only raw event storage
- normalization from raw events into typed tables
- event correlation and FTS search
- retention cleanup (`gc`)

### `crates/tui`

The Ratatui interface renders the local evidence graph into terminal workflows:

- session list with mood badges and sparklines
- replay layout with timeline, transcript, and evidence panes
- search overlay and keyboard help
- first-run wizard views

### `crates/cli`

The clap-based entry point wires everything together. Current commands include:

- `init`
- `serve`
- `trace`
- `search`
- `gc`
- `normalize`
- `daemon start|stop`

Running `claude-insight` without a subcommand is also a first-class launcher
path: it shows the first-run wizard before capture exists, and otherwise
renders the default session home screen.

## Storage Model

Claude Insight keeps multiple storage artifacts under `~/.claude-insight/` by
default:

```text
~/.claude-insight/
├── insight.db               # SQLite database
├── backlog.jsonl            # append-only fallback queue
├── daemon.pid               # daemon pid file
└── transcript_offsets.json  # transcript tailer checkpoints
```

### Raw Capture

The daemon writes incoming hook events into `raw_events` first. The raw layer is
the source of truth because it preserves the original payload JSON and lets the
normalizer be improved or replayed later.

Important properties:

- append-only event ingestion
- shared `session_id`, `tool_use_id`, `prompt_id`, and `agent_id` keys when
  available
- FTS trigger updates on insert and delete
- backlog replay uses the same insert path as live capture

### Typed Tables

Normalization expands raw evidence into higher-level relational tables:

- `sessions`
- `prompts`
- `tool_invocations`
- `permission_decisions`
- `instruction_loads`
- `config_snapshots`
- `event_links`
- `normalization_state`

These tables let the replay UI answer questions like:

- which prompt led to this tool call?
- which permission rule denied this action?
- which instruction file loaded before this event?
- which events are linked by `tool_use_id`, `prompt_id`, or time proximity?

### Full-Text Search

FTS5 indexes event type, tool name, file path, prompt text, and raw payload
content so `claude-insight search` can find evidence across sessions without a
separate search service.

## Capture And Replay Lifecycle

### 1. Hook capture

Claude Code hook scripts call the daemon over HTTP. If the daemon is down, the
same payload is appended to `backlog.jsonl` so evidence is not dropped.

### 2. Backlog replay

When the daemon starts, it replays any pending backlog entries into SQLite
before continuing normal service.

### 3. Transcript tailing

The daemon also tails transcript files under `~/.claude/projects/` and records
those entries into the same local database so transcript context can be linked
back to hook events.

### 4. Normalization

The storage layer can normalize incrementally or rebuild from scratch with
`claude-insight normalize --rebuild`.

### 5. Query

The CLI and TUI read from the evidence graph to render recent sessions, replay
one session, search across sessions, and prune old rows.

## Operational Defaults

- Capture port: `4180` by default
- App state root: `~/.claude-insight/`
- Transcript root: `~/.claude/projects/`
- Database mode: SQLite with WAL enabled for on-disk databases
- Home override: `CLAUDE_INSIGHT_HOME`
- Capture port override: `CLAUDE_INSIGHT_CAPTURE_PORT`

## Design Intent

Claude Insight prefers preserving evidence over eager interpretation. That is
why the architecture is raw-first, local-first, and normalization-friendly:

- raw capture lets the schema evolve without recapturing sessions
- WAL plus SQLite keeps operations simple for a single-user local tool
- the TUI focuses on replay, provenance, and auditability rather than opaque
  model introspection
