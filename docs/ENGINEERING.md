# Engineering Reference

This document consolidates all architectural decisions and implementation specs.
Agents: read this before starting any issue.

## Tech Stack

- **Language:** Rust (2021 edition, stable toolchain)
- **TUI:** ratatui + crossterm
- **HTTP daemon:** axum + tokio
- **Storage:** rusqlite (with `bundled` feature for zero-dep SQLite)
- **JSON:** serde + serde_json
- **CLI:** clap (derive API)
- **File watching:** notify crate
- **Syntax highlighting:** syntect
- **Logging:** tracing + tracing-subscriber

## Workspace Structure

```
Cargo.toml              (workspace root)
crates/
  types/                # Shared Rust types: hook events, transcript entries
    src/
      lib.rs
      hooks.rs          # All 27 hook event types (serde Deserialize)
      transcript.rs     # All 20+ transcript entry types
  storage/              # SQLite layer: schema, raw store, normalizer, correlator, FTS
    src/
      lib.rs
      schema.rs         # CREATE TABLE statements + migrations
      raw_store.rs      # raw_events CRUD
      normalizer.rs     # raw_events → typed tables
      correlator.rs     # Cross-source event linking
      fts.rs            # FTS5 search
  capture/              # Data ingestion: HTTP receiver, transcript tailer, backlog
    src/
      lib.rs
      hook_receiver.rs  # axum POST /hooks endpoint
      transcript_tailer.rs  # File watcher for ~/.claude/projects/
      backlog.rs        # JSONL fallback writer + processor
  daemon/               # Daemon lifecycle: start/stop/health/auto-launch
    src/
      lib.rs
      manager.rs        # DaemonManager struct
  cli/                  # Binary entry point + subcommands
    src/
      main.rs           # clap App
      init.rs           # Install hooks + start daemon
      trace.rs          # Terminal session trace
      search.rs         # FTS search
      gc.rs             # Retention cleanup
      normalize.rs      # On-demand normalization
  tui/                  # Ratatui TUI application
    src/
      lib.rs
      app.rs            # App state machine
      session_list.rs   # Default view: session list
      replay.rs         # Three-pane replay layout
      timeline.rs       # Left pane: event timeline + sparkline
      transcript.rs     # Center pane: conversation view
      evidence.rs       # Right pane: detail panel + JSON highlight
      causal_chain.rs   # Cross-pane highlighting with animation
      keyboard.rs       # Keybinding handler
      search_overlay.rs # Search within session
      wizard.rs         # First-run guided setup
      widgets/
        sparkline.rs    # Activity sparkline
        spinner.rs      # Braille spinner
        mood_badge.rs   # Session mood indicator
tests/
  fixtures/
    hooks/              # One JSON file per hook event type (27 files)
    transcripts/        # JSONL samples covering all entry types
    settings/           # Sample settings.json and .mcp.json
```

## Architecture Decisions

### A1: Daemon with separate ports
- **Capture port** (default 4180): receives hook events via HTTP POST. Lightweight axum handler.
- **TUI API port** (default 4181): serves data to TUI. Separate so UI crash doesn't kill capture.
- Both run in the same daemon process but on different ports.

### A2: Raw-first storage
- All events write to `raw_events` table ONLY on ingest.
- Normalized tables (sessions, tool_invocations, etc.) are materialized on demand via `claude-insight normalize` or when TUI queries need them.
- This allows correlation logic to improve without re-capturing data.
- `claude-insight normalize --rebuild` re-materializes everything from raw_events.

### A3: JSONL fallback
- Hook scripts try HTTP POST to daemon on capture port.
- If daemon is unreachable (connection refused), fall back to appending JSON line to `~/.claude-insight/backlog.jsonl`.
- Daemon processes backlog on startup before accepting new events.
- Events are never lost.

### A4: SessionStart auto-launch
- The SessionStart hook checks if daemon is running (PID file at `~/.claude-insight/daemon.pid`).
- If not running, spawns daemon in background before sending the event.
- First event on cold start is slower (daemon startup). All subsequent events hit running daemon (<50ms).

### A5: Corpus-first development
- Before finalizing Rust types, capture real events from all 27 hook types.
- Test fixtures in `tests/fixtures/` are real Claude Code payloads, not synthetic.
- Types are validated against real data, not just source code reading.

## SQLite Schema

Database path: `~/.claude-insight/insight.db`

```sql
CREATE TABLE raw_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT,
  source TEXT NOT NULL,          -- 'hook' | 'transcript' | 'stream_json' | 'config'
  event_type TEXT NOT NULL,      -- e.g. 'SessionStart', 'PreToolUse', 'TranscriptMessage'
  ts TEXT NOT NULL,              -- ISO 8601 timestamp
  tool_use_id TEXT,              -- nullable join key
  prompt_id TEXT,                -- nullable join key
  agent_id TEXT,
  payload_json TEXT NOT NULL,
  claude_version TEXT,
  adapter_version TEXT
);

CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  transcript_path TEXT, cwd TEXT, project_dir TEXT,
  claude_version TEXT, model TEXT, permission_mode TEXT,
  start_ts TEXT, end_ts TEXT, end_reason TEXT,
  source TEXT, cost_usd REAL,
  input_tokens INTEGER, output_tokens INTEGER
);

CREATE TABLE prompts (
  id TEXT PRIMARY KEY,
  session_id TEXT REFERENCES sessions(id),
  prompt_text TEXT, prompt_hash TEXT, ts TEXT
);

CREATE TABLE tool_invocations (
  id TEXT PRIMARY KEY,
  session_id TEXT REFERENCES sessions(id),
  prompt_id TEXT REFERENCES prompts(id),
  tool_name TEXT NOT NULL,
  tool_input_json TEXT, tool_input_hash TEXT,
  tool_response_json TEXT, tool_response_hash TEXT,
  is_mcp BOOLEAN DEFAULT 0, mcp_server_name TEXT,
  agent_id TEXT,
  pre_hook_ts TEXT, post_hook_ts TEXT,
  duration_ms INTEGER, success BOOLEAN, error_text TEXT
);

CREATE TABLE permission_decisions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT REFERENCES sessions(id),
  tool_invocation_id TEXT REFERENCES tool_invocations(id),
  decision TEXT NOT NULL, source TEXT, rule_text TEXT,
  permission_mode TEXT, ts TEXT
);

CREATE TABLE instruction_loads (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT REFERENCES sessions(id),
  file_path TEXT NOT NULL, memory_type TEXT NOT NULL,
  load_reason TEXT NOT NULL, trigger_file_path TEXT,
  parent_file_path TEXT, content_hash TEXT, ts TEXT
);

CREATE TABLE config_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT REFERENCES sessions(id),
  file_path TEXT NOT NULL, file_hash TEXT NOT NULL,
  scope TEXT, content_json TEXT, ts TEXT
);

CREATE TABLE event_links (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  source_event_id INTEGER REFERENCES raw_events(id),
  target_event_id INTEGER REFERENCES raw_events(id),
  link_type TEXT NOT NULL,        -- 'tool_use_id' | 'prompt_id' | 'timestamp' | 'agent_id'
  confidence REAL DEFAULT 1.0     -- 1.0 for ID match, <1.0 for timestamp proximity
);

CREATE TABLE normalization_state (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  last_raw_event_id INTEGER DEFAULT 0
);

CREATE VIRTUAL TABLE events_fts USING fts5(
  session_id, event_type, tool_name, file_path, prompt_text, content,
  content=raw_events, content_rowid=id
);

CREATE TRIGGER raw_events_ai AFTER INSERT ON raw_events BEGIN
  INSERT INTO events_fts(rowid, session_id, event_type, tool_name, file_path, prompt_text, content)
  VALUES (new.id, new.session_id, new.event_type,
          json_extract(new.payload_json, '$.tool_name'),
          json_extract(new.payload_json, '$.file_path'),
          json_extract(new.payload_json, '$.prompt'),
          new.payload_json);
END;
```

## Correlation Matrix

How events from different sources are linked:

| Event Type | session_id | tool_use_id | prompt_id | agent_id | Fallback |
|-----------|:---:|:---:|:---:|:---:|----------|
| PreToolUse | Y | Y | - | Y | - |
| PostToolUse | Y | Y | - | Y | - |
| PermissionRequest | Y | - | - | - | tool_name + timestamp |
| PermissionDenied | Y | Y | - | - | - |
| UserPromptSubmit | Y | - | - | - | timestamp → promptId |
| InstructionsLoaded | Y | - | - | - | timestamp proximity |
| ConfigChange | Y | - | - | - | timestamp proximity |
| FileChanged | Y | - | - | - | timestamp proximity |
| SubagentStart/Stop | Y | - | - | Y | - |
| Transcript entries | Y | Y* | Y* | Y* | uuid/parentUuid chain |

Timestamp proximity: events within 500ms of a tool invocation are linked to it.

## TUI Design Spec

### Color Palette
```
Background:    #0d1117     Surface:       #161b22
Border:        #30363d     Border Active: #58a6ff
Text Primary:  #c9d1d9     Text Dim:      #8b949e
Accent Cyan:   #58a6ff     Accent Green:  #3fb950
Accent Red:    #f85149     Accent Amber:  #d29922
Accent Purple: #bc8cff     Accent Pink:   #f778ba
```

### Event Emoji Map
```
📋 SessionStart/End       🔧 Tool Call (Pre/Post)
💬 UserPromptSubmit        🛡️ Permission Allow
📖 InstructionsLoaded      🚫 Permission Denied
🤖 SubagentStart/Stop      ❓ Permission Ask
🗜️ PreCompact/PostCompact  ⚠️ Error/Failure
🔌 MCP/Elicitation        📁 FileChanged
⚙️ ConfigChange            📂 CwdChanged
🌳 WorktreeCreate/Remove   📋 TaskCreated/Completed
🔔 Notification            ⏳ TeammateIdle
```

### Session Mood Badge
- 🟢 Clean: no errors, no denials
- 🟡 Friction: >2 permission asks or retries
- 🔴 Errors: any PostToolUseFailure, StopFailure, or denied actions

### Keyboard Navigation
```
GLOBAL:
  q / Ctrl+C     Quit
  ?               Help overlay
  /               Search (FTS)
  Tab             Cycle focus between panes
  1 / 2 / 3      Jump to pane 1/2/3
  Esc             Back / close overlay

SESSION LIST:
  j / ↓           Next session
  k / ↑           Previous session
  Enter           Open session replay
  s               Sort (date/cost/events)
  f               Filter by branch/mood

REPLAY VIEW:
  j / k           Navigate timeline events
  Enter           Select event (show in evidence pane)
  c               Toggle causal chain highlight
  e               Expand/collapse tool call in transcript
  p               Jump to parent prompt of selected tool
  n / N           Next/previous tool call
  [ / ]           Previous/next prompt boundary
  g / G           First/last event
  /               Search within session

EVIDENCE PANE:
  j / k           Scroll evidence content
  y               Copy raw JSON to clipboard
  o               Open file path in editor ($EDITOR)
  l               Show linked events tree
```

### Responsive Layout
- **>160 cols:** Three panes (40% | 35% | 25%)
- **80-160 cols:** Two panes (timeline + transcript). Evidence as overlay.
- **<80 cols:** Single pane, tabbed navigation.
- **<24 rows:** Collapse bottom bar into title bar.

### Interaction States

| Feature | Loading | Empty | Error |
|---------|---------|-------|-------|
| Session List | Braille spinner + "Scanning..." | "No sessions found. Run `claude-insight init`" | Red: "Failed to read: {error}" |
| Timeline | "Loading events..." | "No events in this session" | "Parse error at line {N}" |
| Transcript | "Loading transcript..." | "No transcript found" | "Transcript parse error" |
| Evidence | "Loading..." | "Select an event to see details" | "Failed to load event data" |
| Search | "Searching..." | "No results for '{query}'" | "FTS index unavailable" |

### Causal Chain Highlighting
- Events BEFORE selected: dim cyan background
- Selected event: bright white on accent background
- Events AFTER selected: dim green background
- Propagation: 50ms delay per event, sequential highlighting

## Validation Commands

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

## Distribution

- Single binary via `cargo install --git https://github.com/eddieran/claude-insight --bin claude-insight` or GitHub Releases
- 6 targets: linux-x86_64, linux-aarch64, darwin-x86_64, darwin-aarch64, windows-x86_64, windows-aarch64
- No runtime dependencies
