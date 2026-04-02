use crate::Database;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcReport {
    pub retention_days: u32,
    pub deleted_events: usize,
    pub deleted_sessions: usize,
}

impl Database {
    pub fn delete_raw_events_before(&self, ts: &str) -> rusqlite::Result<usize> {
        let deleted = self.conn.execute(
            "DELETE FROM raw_events
             WHERE ts < ?1",
            [ts],
        )?;

        self.rebuild_fts_index()?;

        Ok(deleted)
    }

    pub fn gc_raw_events(&self, retention_days: u32) -> rusqlite::Result<GcReport> {
        let modifier = format!("-{retention_days} days");
        let deleted_events = self.conn.execute(
            "DELETE FROM raw_events
             WHERE datetime(ts) < datetime('now', ?1)",
            [modifier.as_str()],
        )?;

        self.rebuild_fts_index()?;

        let deleted_sessions = self.conn.execute(
            "DELETE FROM sessions
             WHERE id NOT IN (
                 SELECT DISTINCT session_id
                 FROM raw_events
                 WHERE session_id IS NOT NULL
             )",
            [],
        )?;

        Ok(GcReport {
            retention_days,
            deleted_events,
            deleted_sessions,
        })
    }

    pub(crate) fn rebuild_fts_index(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "
            DROP TRIGGER IF EXISTS raw_events_ai;
            DROP TABLE IF EXISTS events_fts;

            CREATE VIRTUAL TABLE events_fts USING fts5(
              session_id,
              event_type,
              tool_name,
              file_path,
              prompt_text,
              content,
              content=raw_events,
              content_rowid=id
            );

            CREATE TRIGGER raw_events_ai AFTER INSERT ON raw_events BEGIN
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
            ",
        )?;
        self.conn.execute(
            "
            INSERT INTO events_fts (
                rowid,
                session_id,
                event_type,
                tool_name,
                file_path,
                prompt_text,
                content
            )
            SELECT
                id,
                session_id,
                event_type,
                json_extract(payload_json, '$.tool_name'),
                json_extract(payload_json, '$.file_path'),
                json_extract(payload_json, '$.prompt'),
                payload_json
            FROM raw_events
            ",
            [],
        )?;
        Ok(())
    }
}
