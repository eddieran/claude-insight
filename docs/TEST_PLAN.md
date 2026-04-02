# Test Plan: Claude Insight (Rust)

## Test Framework
- **Unit + Integration:** `cargo test` (built-in Rust test framework)
- **Snapshot tests:** `insta` crate for TUI snapshot testing
- **E2E:** Custom Rust test harness launching `claude -p` sessions

## Test Fixtures
Real data from Claude Code sessions at `tests/fixtures/`:
- `hooks/` — one JSON file per hook event type (27 files, from CI-2 corpus)
- `transcripts/` — JSONL samples covering all 20+ entry types
- `settings/` — sample settings.json and .mcp.json (sanitized)

## Validation Commands

```bash
cargo test --workspace                    # all tests
cargo test -p types                       # types crate only
cargo test -p storage                     # storage crate only
cargo test -p capture                     # capture crate only
cargo test -p tui                         # TUI crate only
cargo test --test integration             # integration tests
cargo test --test e2e -- --test-threads=1 # real E2E (sequential, needs claude)
cargo clippy --workspace -- -D warnings   # lint
cargo fmt --all -- --check                # format check
```

---

## Layer 1: Unit Tests — Types (crates/types/)

| Test | File | What it verifies |
|------|------|-----------------|
| deserialize_session_start | hooks.rs | SessionStart fixture → struct with session_id, source, model |
| deserialize_session_end | hooks.rs | SessionEnd fixture → reason field |
| deserialize_pre_tool_use | hooks.rs | PreToolUse fixture → tool_name, tool_input, tool_use_id |
| deserialize_post_tool_use | hooks.rs | PostToolUse fixture → tool_response added |
| deserialize_post_tool_use_failure | hooks.rs | error, is_interrupt fields |
| deserialize_permission_request | hooks.rs | tool_name, permission_suggestions |
| deserialize_permission_denied | hooks.rs | reason field |
| deserialize_user_prompt_submit | hooks.rs | prompt text |
| deserialize_instructions_loaded | hooks.rs | file_path, memory_type, load_reason, trigger_file_path |
| deserialize_subagent_start | hooks.rs | agent_id, agent_type |
| deserialize_subagent_stop | hooks.rs | agent_transcript_path |
| deserialize_config_change | hooks.rs | source, file_path |
| deserialize_all_27_events | hooks.rs | Loop through all fixtures, assert no deserialization errors |
| unknown_fields_ignored | hooks.rs | Extra fields in JSON don't cause errors |
| deserialize_transcript_message | transcript.rs | uuid, parentUuid, promptId, sessionId, content blocks |
| deserialize_summary_message | transcript.rs | Compaction summary |
| deserialize_file_history | transcript.rs | FileHistorySnapshotMessage |
| deserialize_all_entry_types | transcript.rs | Loop through fixture JSONL, assert no errors |
| unknown_entry_type | transcript.rs | Deserializes to Unknown(Value) |

## Layer 2: Unit Tests — Storage (crates/storage/)

| Test | File | What it verifies |
|------|------|-----------------|
| create_fresh_db | schema.rs | All tables + FTS + triggers created |
| insert_raw_event | raw_store.rs | Insert → query round-trip |
| query_by_session | raw_store.rs | Filter by session_id |
| query_by_event_type | raw_store.rs | Filter by event_type |
| fts_search_tool_name | fts.rs | Insert event with tool_name="Bash", search "Bash" → match |
| fts_search_file_path | fts.rs | Search by file path → match |
| fts_search_prompt | fts.rs | Search by prompt text → match |
| fts_trigger_fires | fts.rs | Insert into raw_events, FTS row auto-created |
| normalize_session | normalizer.rs | SessionStart → sessions row |
| normalize_tool | normalizer.rs | PreToolUse + PostToolUse → tool_invocations row |
| normalize_permission | normalizer.rs | PermissionDenied → permission_decisions row |
| normalize_instructions | normalizer.rs | InstructionsLoaded → instruction_loads row |
| normalize_incremental | normalizer.rs | Run twice, no duplicates |
| correlate_tool_use_id | correlator.rs | Hook + transcript linked by tool_use_id |
| correlate_prompt_id | correlator.rs | UserPromptSubmit + transcript linked by promptId |
| correlate_timestamp | correlator.rs | InstructionsLoaded linked by proximity |
| correlate_subagent | correlator.rs | agent_id links parent-child |
| gc_old_events | raw_store.rs | Events older than N days deleted |

## Layer 3: Unit Tests — Capture (crates/capture/)

| Test | File | What it verifies |
|------|------|-----------------|
| hook_receiver_valid | hook_receiver.rs | POST valid SessionStart → 200 + event in DB |
| hook_receiver_malformed | hook_receiver.rs | POST bad JSON → 400 |
| hook_receiver_unknown_type | hook_receiver.rs | POST unknown event → stored in raw_events |
| health_check | hook_receiver.rs | GET /health → JSON status |
| backlog_append | backlog.rs | Append event to JSONL file |
| backlog_concurrent | backlog.rs | 10 concurrent appends, no corruption |
| backlog_process | backlog.rs | Write 10 events, process, all in DB, file cleared |
| tailer_new_lines | transcript_tailer.rs | Append line to JSONL → appears in raw_events |
| tailer_malformed_line | transcript_tailer.rs | Bad line skipped, good lines processed |
| tailer_subagent | transcript_tailer.rs | Subagent JSONL file detected and tailed |

## Layer 4: Unit Tests — TUI (crates/tui/)

| Test | File | What it verifies |
|------|------|-----------------|
| session_list_render | session_list.rs | Snapshot: populated list with sparklines + mood |
| session_list_empty | session_list.rs | Snapshot: empty state message |
| session_list_navigation | session_list.rs | j/k moves selection |
| replay_layout_wide | replay.rs | Snapshot: 3 panes at 180 cols |
| replay_layout_medium | replay.rs | Snapshot: 2 panes at 120 cols |
| replay_layout_narrow | replay.rs | Snapshot: 1 pane at 60 cols |
| timeline_events | timeline.rs | Snapshot: events with emoji markers |
| timeline_sparkline | timeline.rs | Sparkline renders activity density |
| transcript_messages | transcript.rs | Snapshot: user/assistant messages |
| transcript_tool_card | transcript.rs | Snapshot: expanded tool call |
| evidence_json | evidence.rs | Snapshot: syntax-highlighted JSON |
| evidence_permission | evidence.rs | Snapshot: permission chain display |
| causal_chain_highlight | causal_chain.rs | Linked events get background colors |
| keyboard_global | keyboard.rs | q quits, Tab cycles, ? opens help |
| search_overlay | search_overlay.rs | / opens, typing filters, Esc closes |

## Layer 5: Integration Tests (tests/integration.rs)

| Test | What it verifies |
|------|-----------------|
| hook_to_sqlite_roundtrip | POST SessionStart + PreToolUse + PostToolUse → query typed tables |
| transcript_to_sqlite_roundtrip | Ingest 50-line transcript JSONL → all in raw_events |
| multi_source_correlation | Hook events + transcript for same session linked by tool_use_id |
| concurrent_sessions | Events from 2 session_ids stay isolated |
| normalize_full_session | raw_events → all typed tables populated correctly |
| fts_cross_session | Ingest 3 sessions, search by tool → correct session returned |
| backlog_recovery | Write events to backlog, start daemon, verify all processed |

## Layer 6: Real E2E Tests (tests/e2e.rs)

Requires Claude Code installed. Run with `--test-threads=1`.

| Test | Claude -p command | What it verifies |
|------|-------------------|-----------------|
| e2e_simple_prompt | `echo "what is 2+2" \| claude -p` | SessionStart + UserPromptSubmit + Stop captured |
| e2e_tool_usage | `echo "read package.json" \| claude -p` | PreToolUse + PostToolUse for Read tool |
| e2e_file_edit | `echo "create test.txt" \| claude -p` | Write tool captured |
| e2e_multi_tool | `echo "read and summarize" \| claude -p` | Multiple tool_invocations, correct order |
| e2e_instruction_loading | Run in dir with CLAUDE.md | InstructionsLoaded event captured |
| e2e_transcript_correlation | Any session | Hook events match transcript by tool_use_id |
| e2e_session_metadata | Any session | sessions table has cwd, model, version |
| e2e_daemon_crash_recovery | Kill daemon mid-session | JSONL fallback, backlog processed on restart |

## Layer 7: TUI Snapshot Tests (crates/tui/tests/snapshots/)

Uses `insta` crate with Ratatui's `TestBackend`.

| Snapshot | Terminal size | What it captures |
|----------|--------------|-----------------|
| session_list_populated | 120x40 | List with 5 sessions, sparklines, mood badges |
| session_list_empty | 120x40 | "No sessions found" message |
| replay_3pane | 180x50 | Full replay with events, transcript, evidence |
| replay_2pane | 120x40 | Timeline + transcript only |
| replay_1pane | 60x30 | Single pane tabbed view |
| timeline_selected | 40x30 | Event selected with highlight |
| transcript_tool_expanded | 80x30 | Tool call card expanded |
| evidence_json | 50x30 | Syntax-highlighted JSON |
| wizard_step1 | 120x40 | First-run "Install hooks?" prompt |
| loading_spinner | 120x5 | Braille spinner animation frame |

## CI Integration

```yaml
# .github/workflows/ci.yml
- cargo test --workspace          # on every PR
- cargo clippy -- -D warnings     # on every PR
- cargo fmt --all -- --check      # on every PR
- cargo test --test e2e           # on release tags only (needs claude)
```
