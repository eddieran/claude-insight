---
name: debug
description:
  Investigate stuck runs and execution failures by tracing Symphony and Codex
  logs with issue/session identifiers; use when runs stall, retry repeatedly, or
  fail unexpectedly.
---

# Debug

## Goals

- Find why a run is stuck, retrying, or failing.
- Correlate Linear issue identity to a Codex session quickly.
- Read the right logs in the right order to isolate root cause.

## Log Sources

- Primary runtime log: `log/symphony.log`
  - Includes orchestrator, agent runner, and Codex app-server lifecycle logs.
- Rotated runtime logs: `log/symphony.log*`
  - Check these when the relevant run is older.
- Application logs (when debugging claude-insight itself):
  - JSONL event log: check the configured data directory for `.jsonl` files.
  - SQLite database: query the evidence graph with `sqlite3`.
  - Daemon stderr: axum server logs.
  - Use `RUST_LOG=debug cargo run -- serve` for verbose output.

## Correlation Keys

- `issue_identifier`: human ticket key (example: `CI-42`)
- `issue_id`: Linear UUID (stable internal ID)
- `session_id`: Codex thread-turn pair (`<thread_id>-<turn_id>`)

## Quick Triage (Stuck Run)

1. Confirm scheduler/worker symptoms for the ticket.
2. Find recent lines for the ticket (`issue_identifier` first).
3. Extract `session_id` from matching lines.
4. Trace that `session_id` across start, stream, completion/failure, and stall
   handling logs.
5. Decide class of failure: timeout/stall, app-server startup failure, turn
   failure, or orchestrator retry loop.

## Commands

```bash
# 1) Narrow by ticket key (fastest entry point)
rg -n "issue_identifier=CI-42" log/symphony.log*

# 2) If needed, narrow by Linear UUID
rg -n "issue_id=<linear-uuid>" log/symphony.log*

# 3) Pull session IDs seen for that ticket
rg -o "session_id=[^ ;]+" log/symphony.log* | sort -u

# 4) Trace one session end-to-end
rg -n "session_id=<thread>-<turn>" log/symphony.log*

# 5) Focus on stuck/retry signals
rg -n "Issue stalled|scheduling retry|turn_timeout|turn_failed|Codex session failed|Codex session ended with error" log/symphony.log*
```

## Application-Specific Debugging

When debugging claude-insight itself (not just Symphony orchestration):

```bash
# Run with debug logging
RUST_LOG=debug cargo run -- serve

# Check captured events in JSONL
head -20 data/*.jsonl

# Query SQLite evidence graph
sqlite3 data/insight.db "SELECT * FROM events ORDER BY created_at DESC LIMIT 10;"

# Run specific test with output
cargo test <test_name> -- --nocapture

# Run tests with backtrace on panic
RUST_BACKTRACE=1 cargo test
```

## Investigation Flow

1. Locate the ticket slice:
    - Search by `issue_identifier=<KEY>`.
    - If noise is high, add `issue_id=<UUID>`.
2. Establish timeline:
    - Identify first `Codex session started ... session_id=...`.
    - Follow with `Codex session completed`, `ended with error`, or worker exit
      lines.
3. Classify the problem:
    - Stall loop: `Issue stalled ... restarting with backoff`.
    - App-server startup: `Codex session failed ...`.
    - Turn execution failure: `turn_failed`, `turn_cancelled`, `turn_timeout`, or
      `ended with error`.
    - Worker crash: `Agent task exited ... reason=...`.
4. Validate scope:
    - Check whether failures are isolated to one issue/session or repeating across
      multiple tickets.
5. Capture evidence:
    - Save key log lines with timestamps, `issue_identifier`, `issue_id`, and
      `session_id`.
    - Record probable root cause and the exact failing stage.

## Notes

- Prefer `rg` over `grep` for speed on large logs.
- Check rotated logs (`log/symphony.log*`) before concluding data is missing.
