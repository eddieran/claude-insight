#![deny(clippy::expect_used, clippy::unwrap_used)]

mod fts;
mod normalizer;
mod raw_store;
mod schema;

use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub use normalizer::NormalizationStats;
pub use raw_store::{NewRawEvent, RawEvent, RawEventQuery};

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

    pub fn default_path() -> rusqlite::Result<PathBuf> {
        match std::env::var_os("HOME") {
            Some(home) => Ok(PathBuf::from(home)
                .join(DEFAULT_DATABASE_DIR)
                .join(DEFAULT_DATABASE_FILE)),
            None => Err(rusqlite::Error::InvalidPath(PathBuf::from(
                "~/.claude-insight/insight.db",
            ))),
        }
    }

    pub fn open_default() -> rusqlite::Result<Self> {
        Self::new(Self::default_path()?)
    }

    pub fn create_tables(&self) -> rusqlite::Result<()> {
        schema::create_tables(&self.conn)
    }

    pub fn normalize(&self) -> rusqlite::Result<NormalizationStats> {
        normalizer::normalize(self)
    }

    pub fn normalization_watermark(&self) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT last_raw_event_id
             FROM normalization_state
             WHERE id = 1",
            [],
            |row| row.get(0),
        )
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
            "raw_events_ad",
        ];

        for name in names {
            let object_type = if matches!(name, "raw_events_ai" | "raw_events_ad") {
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
    fn insert_raw_event_record_round_trips_full_crud_fields() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;
        let payload_json = serde_json::json!({
            "tool_name": "Bash",
            "command": "ls -la",
        })
        .to_string();

        let event_id = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some("session-1"),
            source: "hook",
            event_type: "PreToolUse",
            ts: "2026-04-03T15:00:00Z",
            tool_use_id: Some("toolu_123"),
            prompt_id: Some("prompt_123"),
            agent_id: Some("agent_123"),
            payload_json: &payload_json,
            claude_version: Some("2.0.0"),
            adapter_version: Some("1.0.0"),
        })?;

        assert!(event_id > 0);

        let events = db.query_raw_events_by_session("session-1")?;

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, event_id);
        assert_eq!(events[0].session_id.as_deref(), Some("session-1"));
        assert_eq!(events[0].source, "hook");
        assert_eq!(events[0].event_type, "PreToolUse");
        assert_eq!(events[0].tool_use_id.as_deref(), Some("toolu_123"));
        assert_eq!(events[0].prompt_id.as_deref(), Some("prompt_123"));
        assert_eq!(events[0].agent_id.as_deref(), Some("agent_123"));
        assert_eq!(events[0].payload_json, payload_json);
        assert_eq!(events[0].claude_version.as_deref(), Some("2.0.0"));
        assert_eq!(events[0].adapter_version.as_deref(), Some("1.0.0"));

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
    fn retention_gc_deletes_old_events_and_cleans_fts_rows() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;

        db.insert_raw_event(
            "session-1",
            "hook",
            "UserPromptSubmit",
            "2026-04-03T14:59:00Z",
            &serde_json::json!({
                "prompt": "old event that should be deleted",
            })
            .to_string(),
        )?;
        db.insert_raw_event(
            "session-1",
            "hook",
            "UserPromptSubmit",
            "2026-04-03T15:00:00Z",
            &serde_json::json!({
                "prompt": "new event that should stay searchable",
            })
            .to_string(),
        )?;

        let deleted = db.delete_raw_events_before("2026-04-03T15:00:00Z")?;
        let remaining = db.query_raw_events(RawEventQuery::default())?;

        assert_eq!(deleted, 1);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].ts, "2026-04-03T15:00:00Z");
        assert!(db.search_fts("deleted")?.is_empty());

        let retained = db.search_fts("searchable")?;
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].ts, "2026-04-03T15:00:00Z");

        Ok(())
    }
}
