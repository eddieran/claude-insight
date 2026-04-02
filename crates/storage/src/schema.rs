use rusqlite::{Connection, OpenFlags};
use std::path::Path;

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  transcript_path TEXT,
  cwd TEXT,
  project_dir TEXT,
  claude_version TEXT,
  model TEXT,
  permission_mode TEXT,
  start_ts TEXT,
  end_ts TEXT,
  end_reason TEXT,
  source TEXT,
  cost_usd REAL,
  input_tokens INTEGER,
  output_tokens INTEGER,
  raw_artifact_path TEXT
);

CREATE TABLE IF NOT EXISTS prompts (
  id TEXT PRIMARY KEY,
  session_id TEXT REFERENCES sessions(id),
  prompt_text TEXT,
  prompt_hash TEXT,
  ts TEXT
);

CREATE TABLE IF NOT EXISTS tool_invocations (
  id TEXT PRIMARY KEY,
  session_id TEXT REFERENCES sessions(id),
  prompt_id TEXT REFERENCES prompts(id),
  tool_name TEXT NOT NULL,
  tool_input_json TEXT,
  tool_input_hash TEXT,
  tool_response_json TEXT,
  tool_response_hash TEXT,
  is_mcp BOOLEAN DEFAULT 0,
  mcp_server_name TEXT,
  agent_id TEXT,
  pre_hook_ts TEXT,
  post_hook_ts TEXT,
  duration_ms INTEGER,
  success BOOLEAN,
  error_text TEXT
);

CREATE TABLE IF NOT EXISTS permission_decisions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT REFERENCES sessions(id),
  tool_invocation_id TEXT REFERENCES tool_invocations(id),
  decision TEXT NOT NULL,
  source TEXT,
  rule_text TEXT,
  permission_mode TEXT,
  ts TEXT
);

CREATE TABLE IF NOT EXISTS instruction_loads (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT REFERENCES sessions(id),
  file_path TEXT NOT NULL,
  memory_type TEXT NOT NULL,
  load_reason TEXT NOT NULL,
  trigger_file_path TEXT,
  parent_file_path TEXT,
  content_hash TEXT,
  ts TEXT
);

CREATE TABLE IF NOT EXISTS config_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT REFERENCES sessions(id),
  file_path TEXT NOT NULL,
  file_hash TEXT NOT NULL,
  scope TEXT,
  content_json TEXT,
  ts TEXT
);

CREATE TABLE IF NOT EXISTS raw_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT,
  source TEXT NOT NULL,
  event_type TEXT NOT NULL,
  ts TEXT NOT NULL,
  tool_use_id TEXT,
  prompt_id TEXT,
  agent_id TEXT,
  payload_json TEXT NOT NULL,
  claude_version TEXT,
  adapter_version TEXT
);

CREATE TABLE IF NOT EXISTS event_links (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  source_event_id INTEGER REFERENCES raw_events(id),
  target_event_id INTEGER REFERENCES raw_events(id),
  link_type TEXT NOT NULL,
  confidence REAL DEFAULT 1.0
);

CREATE TABLE IF NOT EXISTS normalization_state (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  last_raw_event_id INTEGER DEFAULT 0
);

CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
  session_id,
  event_type,
  tool_name,
  file_path,
  prompt_text,
  content,
  content=raw_events,
  content_rowid=id
);

CREATE TRIGGER IF NOT EXISTS raw_events_ai AFTER INSERT ON raw_events BEGIN
  INSERT INTO events_fts(rowid, session_id, event_type, tool_name, file_path, prompt_text, content)
  VALUES (
    new.id,
    new.session_id,
    new.event_type,
    json_extract(new.payload_json, '$.tool_name'),
    json_extract(new.payload_json, '$.file_path'),
    json_extract(new.payload_json, '$.prompt'),
    new.payload_json
  );
END;

CREATE TRIGGER IF NOT EXISTS raw_events_ad AFTER DELETE ON raw_events BEGIN
  INSERT INTO events_fts(events_fts, rowid, session_id, event_type, tool_name, file_path, prompt_text, content)
  VALUES (
    'delete',
    old.id,
    old.session_id,
    old.event_type,
    json_extract(old.payload_json, '$.tool_name'),
    json_extract(old.payload_json, '$.file_path'),
    json_extract(old.payload_json, '$.prompt'),
    old.payload_json
  );
END;
"#;

pub(crate) fn configure_connection(connection: &Connection, path: &Path) -> rusqlite::Result<()> {
    connection.pragma_update(None, "foreign_keys", true)?;

    if !is_in_memory_database_path(path) {
        connection.pragma_update(None, "journal_mode", "WAL")?;
    }

    Ok(())
}

pub(crate) fn open_connection(path: &Path) -> rusqlite::Result<Connection> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;

    if is_sqlite_uri(path) {
        flags |= OpenFlags::SQLITE_OPEN_URI;
    }

    Connection::open_with_flags(path, flags)
}

pub(crate) fn should_create_parent_dir(path: &Path) -> bool {
    !is_in_memory_database_path(path) && !is_sqlite_uri(path)
}

pub(crate) fn is_in_memory_database_path(path: &Path) -> bool {
    let path = path.to_string_lossy();

    path == ":memory:" || (path.starts_with("file:") && path.contains("mode=memory"))
}

fn is_sqlite_uri(path: &Path) -> bool {
    path.to_string_lossy().starts_with("file:")
}

pub(crate) fn create_tables(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(SCHEMA_SQL)?;
    connection.execute(
        "INSERT OR IGNORE INTO normalization_state (id, last_raw_event_id)
         VALUES (1, 0)",
        [],
    )?;

    Ok(())
}
