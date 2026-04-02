use std::collections::BTreeSet;

use crate::{Database, RawEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizationReport {
    pub events_processed: usize,
    pub sessions_touched: usize,
    pub last_raw_event_id: i64,
    pub rebuild: bool,
}

impl Database {
    pub fn normalize(&self, rebuild: bool) -> rusqlite::Result<NormalizationReport> {
        if rebuild {
            self.conn.execute_batch(
                "
                DELETE FROM sessions;
                DELETE FROM prompts;
                DELETE FROM tool_invocations;
                DELETE FROM permission_decisions;
                DELETE FROM instruction_loads;
                DELETE FROM config_snapshots;
                DELETE FROM event_links;
                UPDATE normalization_state SET last_raw_event_id = 0 WHERE id = 1;
                ",
            )?;
        }

        let last_raw_event_id: i64 = self.conn.query_row(
            "SELECT last_raw_event_id FROM normalization_state WHERE id = 1",
            [],
            |row| row.get(0),
        )?;

        let mut statement = self.conn.prepare(
            "
            SELECT
                id,
                session_id,
                source,
                event_type,
                ts,
                tool_use_id,
                prompt_id,
                agent_id,
                payload_json,
                claude_version,
                adapter_version
            FROM raw_events
            WHERE id > ?1
            ORDER BY id ASC
            ",
        )?;
        let events = statement
            .query_map([last_raw_event_id], crate::raw_store::map_raw_event)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut touched_sessions = BTreeSet::new();
        let mut max_raw_event_id = last_raw_event_id;

        for event in &events {
            max_raw_event_id = event.id;

            if let Some(session_id) = event.session_id.as_deref() {
                touched_sessions.insert(session_id.to_string());
                self.touch_session(session_id, &event.ts)?;

                match event.event_type.as_str() {
                    "SessionStart" => self.apply_session_start(session_id, event)?,
                    "SessionEnd" | "Stop" | "StopFailure" => {
                        self.apply_session_end(session_id, event)?
                    }
                    _ => {}
                }
            }
        }

        self.conn.execute(
            "UPDATE normalization_state SET last_raw_event_id = ?1 WHERE id = 1",
            [max_raw_event_id],
        )?;

        Ok(NormalizationReport {
            events_processed: events.len(),
            sessions_touched: touched_sessions.len(),
            last_raw_event_id: max_raw_event_id,
            rebuild,
        })
    }

    fn touch_session(&self, session_id: &str, ts: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, start_ts, end_ts)
             VALUES (?1, ?2, ?2)
             ON CONFLICT(id) DO NOTHING",
            (session_id, ts),
        )?;
        Ok(())
    }

    fn apply_session_start(&self, session_id: &str, event: &RawEvent) -> rusqlite::Result<()> {
        let payload = parse_payload(&event.payload_json);

        self.conn.execute(
            "
            INSERT INTO sessions (
                id,
                transcript_path,
                cwd,
                project_dir,
                claude_version,
                model,
                permission_mode,
                start_ts,
                source,
                raw_artifact_path
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                transcript_path = COALESCE(excluded.transcript_path, sessions.transcript_path),
                cwd = COALESCE(excluded.cwd, sessions.cwd),
                project_dir = COALESCE(excluded.project_dir, sessions.project_dir),
                claude_version = COALESCE(excluded.claude_version, sessions.claude_version),
                model = COALESCE(excluded.model, sessions.model),
                permission_mode = COALESCE(excluded.permission_mode, sessions.permission_mode),
                start_ts = COALESCE(sessions.start_ts, excluded.start_ts),
                source = COALESCE(excluded.source, sessions.source),
                raw_artifact_path = COALESCE(excluded.raw_artifact_path, sessions.raw_artifact_path)
            ",
            (
                session_id,
                payload_string(&payload, &["transcript_path", "transcriptPath"]),
                payload_string(&payload, &["cwd"]),
                payload_string(&payload, &["project_dir", "projectDir"]),
                event
                    .claude_version
                    .as_deref()
                    .or_else(|| payload_string(&payload, &["claude_version", "claudeVersion"])),
                payload_string(&payload, &["model"]),
                payload_string(&payload, &["permission_mode", "permissionMode"]),
                event.ts.as_str(),
                payload_string(&payload, &["source"]),
                payload_string(&payload, &["raw_artifact_path", "rawArtifactPath"]),
            ),
        )?;

        Ok(())
    }

    fn apply_session_end(&self, session_id: &str, event: &RawEvent) -> rusqlite::Result<()> {
        let payload = parse_payload(&event.payload_json);

        self.conn.execute(
            "
            UPDATE sessions
            SET end_ts = ?2,
                end_reason = COALESCE(?3, end_reason)
            WHERE id = ?1
            ",
            (
                session_id,
                event.ts.as_str(),
                payload_string(&payload, &["reason", "end_reason", "endReason"]),
            ),
        )?;

        Ok(())
    }
}

fn parse_payload(payload_json: &str) -> serde_json::Value {
    serde_json::from_str(payload_json).unwrap_or(serde_json::Value::Null)
}

fn payload_string<'a>(payload: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| payload.get(key).and_then(serde_json::Value::as_str))
}
