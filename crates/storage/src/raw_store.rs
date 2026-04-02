use crate::Database;
use rusqlite::{params, Row};

const RAW_EVENT_SELECT: &str = "
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
";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RawEvent {
    pub id: i64,
    pub session_id: Option<String>,
    pub source: String,
    pub event_type: String,
    pub ts: String,
    pub tool_use_id: Option<String>,
    pub prompt_id: Option<String>,
    pub agent_id: Option<String>,
    pub payload_json: String,
    pub claude_version: Option<String>,
    pub adapter_version: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct NewRawEvent<'a> {
    pub session_id: Option<&'a str>,
    pub source: &'a str,
    pub event_type: &'a str,
    pub ts: &'a str,
    pub tool_use_id: Option<&'a str>,
    pub prompt_id: Option<&'a str>,
    pub agent_id: Option<&'a str>,
    pub payload_json: &'a str,
    pub claude_version: Option<&'a str>,
    pub adapter_version: Option<&'a str>,
}

impl<'a> NewRawEvent<'a> {
    pub fn new(
        session_id: &'a str,
        source: &'a str,
        event_type: &'a str,
        ts: &'a str,
        payload_json: &'a str,
    ) -> Self {
        Self {
            session_id: Some(session_id),
            source,
            event_type,
            ts,
            tool_use_id: None,
            prompt_id: None,
            agent_id: None,
            payload_json,
            claude_version: None,
            adapter_version: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RawEventQuery<'a> {
    pub session_id: Option<&'a str>,
    pub event_type: Option<&'a str>,
    pub start_ts: Option<&'a str>,
    pub end_ts: Option<&'a str>,
}

impl Database {
    pub fn insert_raw_event(
        &self,
        session_id: &str,
        source: &str,
        event_type: &str,
        ts: &str,
        payload_json: &str,
    ) -> rusqlite::Result<i64> {
        self.insert_raw_event_record(&NewRawEvent::new(
            session_id,
            source,
            event_type,
            ts,
            payload_json,
        ))
    }

    pub fn insert_raw_event_record(&self, event: &NewRawEvent<'_>) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO raw_events (
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
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                event.session_id,
                event.source,
                event.event_type,
                event.ts,
                event.tool_use_id,
                event.prompt_id,
                event.agent_id,
                event.payload_json,
                event.claude_version,
                event.adapter_version,
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn query_raw_events(&self, query: RawEventQuery<'_>) -> rusqlite::Result<Vec<RawEvent>> {
        let sql = format!(
            "{RAW_EVENT_SELECT}
             WHERE (?1 IS NULL OR session_id = ?1)
               AND (?2 IS NULL OR event_type = ?2)
               AND (?3 IS NULL OR ts >= ?3)
               AND (?4 IS NULL OR ts <= ?4)
             ORDER BY ts ASC, id ASC"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(
            params![
                query.session_id,
                query.event_type,
                query.start_ts,
                query.end_ts
            ],
            map_raw_event,
        )?;

        rows.collect()
    }

    pub fn query_raw_events_by_session(&self, session_id: &str) -> rusqlite::Result<Vec<RawEvent>> {
        self.query_raw_events(RawEventQuery {
            session_id: Some(session_id),
            ..RawEventQuery::default()
        })
    }

    pub fn query_raw_events_by_event_type(
        &self,
        event_type: &str,
    ) -> rusqlite::Result<Vec<RawEvent>> {
        self.query_raw_events(RawEventQuery {
            event_type: Some(event_type),
            ..RawEventQuery::default()
        })
    }

    pub fn count_raw_events(&self) -> rusqlite::Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM raw_events", [], |row| row.get(0))
    }

}

pub(crate) fn map_raw_event(row: &Row<'_>) -> rusqlite::Result<RawEvent> {
    Ok(RawEvent {
        id: row.get(0)?,
        session_id: row.get(1)?,
        source: row.get(2)?,
        event_type: row.get(3)?,
        ts: row.get(4)?,
        tool_use_id: row.get(5)?,
        prompt_id: row.get(6)?,
        agent_id: row.get(7)?,
        payload_json: row.get(8)?,
        claude_version: row.get(9)?,
        adapter_version: row.get(10)?,
    })
}
