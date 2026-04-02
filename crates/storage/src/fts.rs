use crate::{raw_store, Database, RawEvent};

impl Database {
    pub fn search_fts(&self, query: &str) -> rusqlite::Result<Vec<RawEvent>> {
        let mut statement = self.conn.prepare(
            "
            SELECT
                raw_events.id,
                raw_events.session_id,
                raw_events.source,
                raw_events.event_type,
                raw_events.ts,
                raw_events.tool_use_id,
                raw_events.prompt_id,
                raw_events.agent_id,
                raw_events.payload_json,
                raw_events.claude_version,
                raw_events.adapter_version
            FROM events_fts
            INNER JOIN raw_events ON raw_events.id = events_fts.rowid
            WHERE events_fts MATCH ?1
            ORDER BY bm25(events_fts), raw_events.id
            ",
        )?;
        let rows = statement.query_map([query], raw_store::map_raw_event)?;

        rows.collect()
    }
}
