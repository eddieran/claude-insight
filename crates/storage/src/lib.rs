#![deny(clippy::expect_used, clippy::unwrap_used)]

mod fts;
mod maintenance;
mod normalizer;
mod raw_store;
mod schema;
mod sessions;

use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub use maintenance::GcReport;
pub use normalizer::NormalizationReport;
pub use raw_store::{NewRawEvent, RawEvent, RawEventQuery};
pub use sessions::SessionSummary;

pub const CRATE_NAME: &str = "claude-insight-storage";
const DEFAULT_DATABASE_DIR: &str = ".claude-insight";
const DEFAULT_DATABASE_FILE: &str = "insight.db";

#[derive(Debug)]
pub struct Database {
    pub(crate) conn: Connection,
}

impl Database {
    pub fn new(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let path = path.as_ref();

        if path != Path::new(":memory:") {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)
                    .map_err(|_| rusqlite::Error::InvalidPath(path.to_path_buf()))?;
            }
        }

        let conn = Connection::open(path)?;
        schema::configure_connection(&conn, path)?;

        let database = Self { conn };
        database.create_tables()?;

        Ok(database)
    }

    pub fn default_dir() -> rusqlite::Result<PathBuf> {
        match std::env::var_os("CLAUDE_INSIGHT_HOME").or_else(|| std::env::var_os("HOME")) {
            Some(home) => Ok(PathBuf::from(home).join(DEFAULT_DATABASE_DIR)),
            None => Err(rusqlite::Error::InvalidPath(PathBuf::from(
                "~/.claude-insight/insight.db",
            ))),
        }
    }

    pub fn default_path() -> rusqlite::Result<PathBuf> {
        Ok(Self::default_dir()?.join(DEFAULT_DATABASE_FILE))
    }

    pub fn open_default() -> rusqlite::Result<Self> {
        Self::new(Self::default_path()?)
    }

    pub fn create_tables(&self) -> rusqlite::Result<()> {
        schema::create_tables(&self.conn)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StorageStub {
    pub database_url: String,
}

impl StorageStub {
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
        }
    }

    pub fn open_in_memory() -> rusqlite::Result<Database> {
        tracing::trace!("opening in-memory database");
        Database::new(":memory:")
    }

    pub fn sample_event() -> claude_insight_types::PlaceholderEvent {
        claude_insight_types::placeholder_event()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn storage_stub_keeps_database_url() {
        let stub = StorageStub::new("sqlite::memory:");

        assert_eq!(stub.database_url, "sqlite::memory:");
    }

    #[test]
    fn create_fresh_db_creates_required_objects() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;
        let names = [
            "sessions",
            "prompts",
            "tool_invocations",
            "permission_decisions",
            "instruction_loads",
            "config_snapshots",
            "raw_events",
            "event_links",
            "normalization_state",
            "events_fts",
            "raw_events_ai",
        ];

        for name in names {
            let object_type = if name == "raw_events_ai" {
                "trigger"
            } else {
                "table"
            };
            let exists: i64 = db.conn.query_row(
                "SELECT EXISTS(
                    SELECT 1
                    FROM sqlite_master
                    WHERE name = ?1 AND type = ?2
                )",
                params![name, object_type],
                |row| row.get(0),
            )?;

            assert_eq!(exists, 1, "{name} should exist");
        }

        Ok(())
    }

    #[test]
    fn insert_raw_event_round_trips_by_session() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;
        let payload_json = serde_json::json!({
            "tool_name": "Bash",
            "command": "ls -la",
        })
        .to_string();

        let event_id = db.insert_raw_event(
            "session-1",
            "hook",
            "SessionStart",
            "2026-04-03T15:00:00Z",
            &payload_json,
        )?;

        assert!(event_id > 0);

        let events = db.query_raw_events_by_session("session-1")?;

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, event_id);
        assert_eq!(events[0].session_id.as_deref(), Some("session-1"));
        assert_eq!(events[0].source, "hook");
        assert_eq!(events[0].event_type, "SessionStart");
        assert_eq!(events[0].payload_json, payload_json);

        Ok(())
    }

    #[test]
    fn query_raw_events_filters_by_event_type() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;

        db.insert_raw_event(
            "session-1",
            "hook",
            "SessionStart",
            "2026-04-03T15:00:00Z",
            &serde_json::json!({ "tool_name": "Bash" }).to_string(),
        )?;
        db.insert_raw_event(
            "session-1",
            "hook",
            "Notification",
            "2026-04-03T15:01:00Z",
            &serde_json::json!({ "message": "done" }).to_string(),
        )?;

        let events = db.query_raw_events_by_event_type("Notification")?;

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "Notification");

        Ok(())
    }

    #[test]
    fn query_raw_events_filters_by_time_range() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;

        db.insert_raw_event(
            "session-1",
            "hook",
            "SessionStart",
            "2026-04-03T15:00:00Z",
            &serde_json::json!({ "tool_name": "Bash" }).to_string(),
        )?;
        db.insert_raw_event(
            "session-1",
            "hook",
            "Notification",
            "2026-04-03T15:02:00Z",
            &serde_json::json!({ "message": "done" }).to_string(),
        )?;

        let events = db.query_raw_events(RawEventQuery {
            start_ts: Some("2026-04-03T15:01:00Z"),
            end_ts: Some("2026-04-03T15:03:00Z"),
            ..RawEventQuery::default()
        })?;

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "Notification");

        Ok(())
    }

    #[test]
    fn fts_search_tool_name_finds_matching_event() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;

        db.insert_raw_event(
            "session-1",
            "hook",
            "PreToolUse",
            "2026-04-03T15:00:00Z",
            &serde_json::json!({
                "tool_name": "Bash",
                "tool_input": { "command": "pwd" },
            })
            .to_string(),
        )?;

        let events = db.search_fts("Bash")?;

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "PreToolUse");

        Ok(())
    }

    #[test]
    fn fts_search_file_path_finds_matching_event() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;

        db.insert_raw_event(
            "session-1",
            "hook",
            "FileChanged",
            "2026-04-03T15:00:00Z",
            &serde_json::json!({
                "file_path": "/tmp/example.txt",
            })
            .to_string(),
        )?;

        let events = db.search_fts("example")?;

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "FileChanged");

        Ok(())
    }

    #[test]
    fn fts_search_prompt_finds_matching_event() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;

        db.insert_raw_event(
            "session-1",
            "hook",
            "UserPromptSubmit",
            "2026-04-03T15:00:00Z",
            &serde_json::json!({
                "prompt": "Search the Rust docs for WAL mode",
            })
            .to_string(),
        )?;

        let events = db.search_fts("Rust")?;

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "UserPromptSubmit");

        Ok(())
    }

    #[test]
    fn fts_trigger_fires_after_insert() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;

        let event_id = db.insert_raw_event(
            "session-1",
            "hook",
            "PreToolUse",
            "2026-04-03T15:00:00Z",
            &serde_json::json!({
                "tool_name": "Bash",
                "tool_input": { "command": "pwd" },
            })
            .to_string(),
        )?;

        let indexed_row_ids: Vec<i64> = db
            .conn
            .prepare(
                "SELECT rowid
                 FROM events_fts
                 WHERE events_fts MATCH ?1",
            )?
            .query_map(["Bash"], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        assert_eq!(indexed_row_ids, vec![event_id]);

        Ok(())
    }

    #[test]
    fn recent_sessions_are_grouped_from_raw_events() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;

        db.insert_raw_event(
            "session-1",
            "hook",
            "SessionStart",
            "2026-04-03T15:00:00Z",
            &serde_json::json!({ "source": "startup" }).to_string(),
        )?;
        db.insert_raw_event(
            "session-2",
            "hook",
            "SessionStart",
            "2026-04-03T15:05:00Z",
            &serde_json::json!({ "source": "startup" }).to_string(),
        )?;
        db.insert_raw_event(
            "session-2",
            "hook",
            "Notification",
            "2026-04-03T15:06:00Z",
            &serde_json::json!({ "message": "done" }).to_string(),
        )?;

        let sessions = db.list_recent_sessions(10)?;

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "session-2");
        assert_eq!(sessions[0].event_count, 2);
        assert_eq!(sessions[0].last_event_type.as_deref(), Some("Notification"));
        assert_eq!(sessions[1].session_id, "session-1");

        Ok(())
    }

    #[test]
    fn gc_prunes_old_raw_events_and_rebuilds_fts() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;

        db.insert_raw_event(
            "session-old",
            "hook",
            "PreToolUse",
            "2020-01-01T00:00:00Z",
            &serde_json::json!({ "tool_name": "Bash" }).to_string(),
        )?;
        db.insert_raw_event(
            "session-new",
            "hook",
            "PreToolUse",
            "2099-01-01T00:00:00Z",
            &serde_json::json!({ "tool_name": "Bash" }).to_string(),
        )?;

        let report = db.gc_raw_events(90)?;
        let events = db.search_fts("Bash")?;

        assert_eq!(report.deleted_events, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id.as_deref(), Some("session-new"));

        Ok(())
    }

    #[test]
    fn normalize_materializes_sessions_from_raw_events() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;

        db.insert_raw_event(
            "session-1",
            "hook",
            "SessionStart",
            "2026-04-03T15:00:00Z",
            &serde_json::json!({
                "source": "startup",
                "cwd": "/workspace/claude-insight",
                "model": "claude-sonnet",
            })
            .to_string(),
        )?;
        db.insert_raw_event(
            "session-1",
            "hook",
            "SessionEnd",
            "2026-04-03T15:05:00Z",
            &serde_json::json!({
                "reason": "completed",
            })
            .to_string(),
        )?;

        let report = db.normalize(false)?;
        let session: (String, Option<String>, Option<String>, Option<String>) = db.conn.query_row(
            "SELECT id, start_ts, end_ts, model FROM sessions WHERE id = ?1",
            ["session-1"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;

        assert_eq!(report.events_processed, 2);
        assert_eq!(report.sessions_touched, 1);
        assert_eq!(session.0, "session-1");
        assert_eq!(session.1.as_deref(), Some("2026-04-03T15:00:00Z"));
        assert_eq!(session.2.as_deref(), Some("2026-04-03T15:05:00Z"));
        assert_eq!(session.3.as_deref(), Some("claude-sonnet"));

        Ok(())
    }
}
