use std::collections::HashMap;

use crate::Database;
use rusqlite::Row;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionSummary {
    pub session_id: String,
    pub start_ts: String,
    pub end_ts: String,
    pub event_count: i64,
    pub last_event_type: Option<String>,
    pub project_dir: Option<String>,
    pub cost_usd: Option<f64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventSummary {
    pub id: i64,
    pub session_id: String,
    pub event_type: String,
    pub ts: String,
    pub tool_use_id: Option<String>,
}

impl Database {
    pub fn list_recent_sessions(&self, limit: usize) -> rusqlite::Result<Vec<SessionSummary>> {
        let mut statement = self.conn.prepare(
            "
            SELECT
                re.session_id,
                MIN(re.ts) AS start_ts,
                MAX(re.ts) AS end_ts,
                COUNT(*) AS event_count,
                (
                    SELECT re2.event_type
                    FROM raw_events re2
                    WHERE re2.session_id = re.session_id
                    ORDER BY re2.ts DESC, re2.id DESC
                    LIMIT 1
                ) AS last_event_type,
                s.project_dir,
                s.cost_usd,
                s.input_tokens,
                s.output_tokens,
                s.model
            FROM raw_events re
            LEFT JOIN sessions s ON s.id = re.session_id
            WHERE re.session_id IS NOT NULL AND re.session_id != ''
            GROUP BY re.session_id
            ORDER BY MAX(re.ts) DESC
            LIMIT ?1
            ",
        )?;
        let rows = statement.query_map([limit as i64], map_session_summary)?;

        rows.collect()
    }

    pub fn query_event_summaries_for_sessions(
        &self,
        session_ids: &[&str],
    ) -> rusqlite::Result<HashMap<String, Vec<EventSummary>>> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders: Vec<String> = (1..=session_ids.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT id, session_id, event_type, ts, tool_use_id
             FROM raw_events
             WHERE session_id IN ({})
             ORDER BY ts ASC, id ASC",
            placeholders.join(", ")
        );

        let mut statement = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = session_ids
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = statement.query_map(params.as_slice(), |row| {
            Ok(EventSummary {
                id: row.get(0)?,
                session_id: row.get(1)?,
                event_type: row.get(2)?,
                ts: row.get(3)?,
                tool_use_id: row.get(4)?,
            })
        })?;

        let mut result: HashMap<String, Vec<EventSummary>> = HashMap::new();
        for row in rows {
            let summary = row?;
            result
                .entry(summary.session_id.clone())
                .or_default()
                .push(summary);
        }

        Ok(result)
    }

    pub fn normalized_session_exists(&self, session_id: &str) -> rusqlite::Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
                [session_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|exists| exists == 1)
    }
}

fn map_session_summary(row: &Row<'_>) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        session_id: row.get(0)?,
        start_ts: row.get(1)?,
        end_ts: row.get(2)?,
        event_count: row.get(3)?,
        last_event_type: row.get(4)?,
        project_dir: row.get(5)?,
        cost_usd: row.get(6)?,
        input_tokens: row.get(7)?,
        output_tokens: row.get(8)?,
        model: row.get(9)?,
    })
}
