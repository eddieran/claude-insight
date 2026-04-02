use crate::{Database, RawEvent};
use claude_insight_types::{
    ContentBlock, HookEvent, TranscriptContent, TranscriptEntry, TranscriptMessageKind,
};
use rusqlite::{params, Connection, Row};
use std::collections::{HashMap, HashSet};

const TIMESTAMP_WINDOW_MS: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorrelationStats {
    pub links_deleted: usize,
    pub links_created: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventLink {
    pub id: i64,
    pub source_event_id: i64,
    pub target_event_id: i64,
    pub link_type: String,
    pub confidence: f64,
}

pub struct Correlator<'db> {
    database: &'db Database,
}

#[derive(Debug, Clone)]
struct SessionEvent {
    raw: RawEvent,
    ts_ms: i64,
    tool_use_ids: Vec<String>,
    prompt_id: Option<String>,
    agent_id: Option<String>,
    prompt_text: Option<String>,
    is_prompt_event: bool,
    is_transcript_user_message: bool,
    is_instructions_loaded: bool,
    is_timestamp_tool_candidate: bool,
}

#[derive(Debug, Clone)]
struct PendingLink {
    source_event_id: i64,
    target_event_id: i64,
    link_type: &'static str,
    confidence: f64,
}

#[derive(Debug, Default)]
struct PendingLinks {
    links: HashMap<(i64, i64, &'static str), f64>,
}

impl PendingLinks {
    fn insert(
        &mut self,
        source_event_id: i64,
        target_event_id: i64,
        link_type: &'static str,
        confidence: f64,
    ) {
        if source_event_id == target_event_id {
            return;
        }

        let (source_event_id, target_event_id) = canonical_pair(source_event_id, target_event_id);
        let confidence = confidence.clamp(0.0, 1.0);
        let entry = self
            .links
            .entry((source_event_id, target_event_id, link_type))
            .or_insert(confidence);
        if confidence > *entry {
            *entry = confidence;
        }
    }

    fn insert_pairwise(&mut self, event_ids: &[i64], link_type: &'static str, confidence: f64) {
        for (index, source_event_id) in event_ids.iter().enumerate() {
            for target_event_id in &event_ids[index + 1..] {
                self.insert(*source_event_id, *target_event_id, link_type, confidence);
            }
        }
    }

    fn into_vec(self) -> Vec<PendingLink> {
        let mut links = self
            .links
            .into_iter()
            .map(
                |((source_event_id, target_event_id, link_type), confidence)| PendingLink {
                    source_event_id,
                    target_event_id,
                    link_type,
                    confidence,
                },
            )
            .collect::<Vec<_>>();
        links.sort_by(|left, right| {
            (left.link_type, left.source_event_id, left.target_event_id).cmp(&(
                right.link_type,
                right.source_event_id,
                right.target_event_id,
            ))
        });
        links
    }
}

impl Database {
    pub fn correlator(&self) -> Correlator<'_> {
        Correlator { database: self }
    }

    pub fn correlate_session(&self, session_id: &str) -> rusqlite::Result<CorrelationStats> {
        self.correlator().correlate_session(session_id)
    }

    pub fn query_event_links_by_session(
        &self,
        session_id: &str,
    ) -> rusqlite::Result<Vec<EventLink>> {
        let mut statement = self.conn.prepare(
            "
            SELECT
                event_links.id,
                event_links.source_event_id,
                event_links.target_event_id,
                event_links.link_type,
                event_links.confidence
            FROM event_links
            JOIN raw_events AS source_events
              ON source_events.id = event_links.source_event_id
            JOIN raw_events AS target_events
              ON target_events.id = event_links.target_event_id
            WHERE source_events.session_id = ?1
               OR target_events.session_id = ?1
            ORDER BY event_links.link_type ASC, event_links.source_event_id ASC, event_links.target_event_id ASC
            ",
        )?;
        let rows = statement.query_map([session_id], map_event_link)?;

        rows.collect()
    }
}

impl<'db> Correlator<'db> {
    pub fn correlate_session(&self, session_id: &str) -> rusqlite::Result<CorrelationStats> {
        let tx = self.database.conn.unchecked_transaction()?;
        let events = load_session_events(&tx, session_id)?;
        let links_deleted = clear_session_links(&tx, session_id)?;

        let mut pending_links = PendingLinks::default();
        correlate_tool_groups(&events, &mut pending_links);
        correlate_prompt_groups(&events, &mut pending_links);
        correlate_inferred_prompt_pairs(&events, &mut pending_links);
        correlate_timestamp_fallbacks(&events, &mut pending_links);
        correlate_agent_groups(&events, &mut pending_links);

        let pending = pending_links.into_vec();
        for link in &pending {
            tx.execute(
                "INSERT INTO event_links (
                    source_event_id,
                    target_event_id,
                    link_type,
                    confidence
                ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    link.source_event_id,
                    link.target_event_id,
                    link.link_type,
                    link.confidence
                ],
            )?;
        }

        tx.commit()?;

        Ok(CorrelationStats {
            links_deleted,
            links_created: pending.len(),
        })
    }
}

fn load_session_events(
    connection: &Connection,
    session_id: &str,
) -> rusqlite::Result<Vec<SessionEvent>> {
    let mut statement = connection.prepare(
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
            adapter_version,
            CAST(ROUND((julianday(ts) - 2440587.5) * 86400000.0) AS INTEGER) AS ts_ms
        FROM raw_events
        WHERE session_id = ?1
        ORDER BY ts ASC, id ASC
        ",
    )?;
    let rows = statement.query_map([session_id], map_session_event)?;

    rows.collect()
}

fn map_session_event(row: &Row<'_>) -> rusqlite::Result<SessionEvent> {
    let raw = RawEvent {
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
    };
    let ts_ms = row.get(11)?;

    Ok(SessionEvent::from_raw_event(raw, ts_ms))
}

fn map_event_link(row: &Row<'_>) -> rusqlite::Result<EventLink> {
    Ok(EventLink {
        id: row.get(0)?,
        source_event_id: row.get(1)?,
        target_event_id: row.get(2)?,
        link_type: row.get(3)?,
        confidence: row.get(4)?,
    })
}

fn clear_session_links(connection: &Connection, session_id: &str) -> rusqlite::Result<usize> {
    connection.execute(
        "
        DELETE FROM event_links
        WHERE source_event_id IN (
            SELECT id
            FROM raw_events
            WHERE session_id = ?1
        )
        OR target_event_id IN (
            SELECT id
            FROM raw_events
            WHERE session_id = ?1
        )
        ",
        [session_id],
    )
}

fn correlate_tool_groups(events: &[SessionEvent], pending_links: &mut PendingLinks) {
    let groups = build_tool_groups(events);
    for event_ids in groups.values() {
        pending_links.insert_pairwise(event_ids, "tool_use_id", 1.0);
    }
}

fn correlate_prompt_groups(events: &[SessionEvent], pending_links: &mut PendingLinks) {
    let groups = build_prompt_groups(events);
    for event_ids in groups.values() {
        pending_links.insert_pairwise(event_ids, "prompt_id", 1.0);
    }
}

fn correlate_inferred_prompt_pairs(events: &[SessionEvent], pending_links: &mut PendingLinks) {
    let transcript_prompt_events = events
        .iter()
        .filter(|event| event.is_transcript_user_message && event.prompt_id.is_some())
        .collect::<Vec<_>>();

    for event in events
        .iter()
        .filter(|event| event.raw.source == "hook" && event.raw.event_type == "UserPromptSubmit")
    {
        if event.prompt_id.is_some() {
            continue;
        }

        let same_text = transcript_prompt_events
            .iter()
            .filter(|candidate| {
                event.prompt_text.is_some()
                    && event.prompt_text == candidate.prompt_text
                    && within_window(event.ts_ms, candidate.ts_ms)
            })
            .copied()
            .collect::<Vec<_>>();

        let fallback_candidates = if same_text.is_empty() {
            transcript_prompt_events
                .iter()
                .filter(|candidate| within_window(event.ts_ms, candidate.ts_ms))
                .copied()
                .collect::<Vec<_>>()
        } else {
            same_text
        };

        if let Some(candidate) = nearest_event(event, &fallback_candidates) {
            pending_links.insert(event.raw.id, candidate.raw.id, "prompt_id", 0.95);
        }
    }
}

fn correlate_timestamp_fallbacks(events: &[SessionEvent], pending_links: &mut PendingLinks) {
    let tool_groups = build_tool_event_groups(events);
    let prompt_groups = build_prompt_event_groups(events);

    for event in events.iter().filter(|event| event.is_instructions_loaded) {
        if let Some(group) = nearest_group(event, &prompt_groups) {
            for target in group
                .iter()
                .filter(|target| within_window(event.ts_ms, target.ts_ms))
            {
                pending_links.insert(
                    event.raw.id,
                    target.raw.id,
                    "timestamp",
                    timestamp_confidence(timestamp_distance(event.ts_ms, target.ts_ms)),
                );
            }
        }
    }

    for event in events
        .iter()
        .filter(|event| event.is_timestamp_tool_candidate)
    {
        if let Some(group) = nearest_group(event, &tool_groups) {
            for target in group
                .iter()
                .filter(|target| within_window(event.ts_ms, target.ts_ms))
            {
                pending_links.insert(
                    event.raw.id,
                    target.raw.id,
                    "timestamp",
                    timestamp_confidence(timestamp_distance(event.ts_ms, target.ts_ms)),
                );
            }
        }
    }
}

fn correlate_agent_groups(events: &[SessionEvent], pending_links: &mut PendingLinks) {
    let mut groups: HashMap<&str, Vec<i64>> = HashMap::new();

    for event in events.iter().filter(|event| event.agent_id.is_some()) {
        if let Some(agent_id) = event.agent_id.as_deref() {
            groups.entry(agent_id).or_default().push(event.raw.id);
        }
    }

    for event_ids in groups.values_mut() {
        event_ids.sort_unstable();
        event_ids.dedup();
        pending_links.insert_pairwise(event_ids, "agent_id", 1.0);
    }
}

fn build_tool_groups(events: &[SessionEvent]) -> HashMap<&str, Vec<i64>> {
    let mut groups: HashMap<&str, Vec<i64>> = HashMap::new();

    for event in events {
        for tool_use_id in &event.tool_use_ids {
            groups
                .entry(tool_use_id.as_str())
                .or_default()
                .push(event.raw.id);
        }
    }

    for event_ids in groups.values_mut() {
        event_ids.sort_unstable();
        event_ids.dedup();
    }

    groups
}

fn build_prompt_groups(events: &[SessionEvent]) -> HashMap<&str, Vec<i64>> {
    let mut groups: HashMap<&str, Vec<i64>> = HashMap::new();

    for event in events
        .iter()
        .filter(|event| event.is_prompt_event && event.prompt_id.is_some())
    {
        if let Some(prompt_id) = event.prompt_id.as_deref() {
            groups.entry(prompt_id).or_default().push(event.raw.id);
        }
    }

    for event_ids in groups.values_mut() {
        event_ids.sort_unstable();
        event_ids.dedup();
    }

    groups
}

fn build_tool_event_groups(events: &[SessionEvent]) -> Vec<Vec<&SessionEvent>> {
    build_tool_groups(events)
        .into_values()
        .map(|event_ids| {
            let event_id_set = event_ids.into_iter().collect::<HashSet<_>>();
            events
                .iter()
                .filter(|event| event_id_set.contains(&event.raw.id))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn build_prompt_event_groups(events: &[SessionEvent]) -> Vec<Vec<&SessionEvent>> {
    build_prompt_groups(events)
        .into_values()
        .map(|event_ids| {
            let event_id_set = event_ids.into_iter().collect::<HashSet<_>>();
            events
                .iter()
                .filter(|event| event_id_set.contains(&event.raw.id))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn nearest_group<'a>(
    source: &SessionEvent,
    groups: &'a [Vec<&SessionEvent>],
) -> Option<&'a Vec<&'a SessionEvent>> {
    groups
        .iter()
        .filter_map(|group| {
            let distance = group
                .iter()
                .map(|event| timestamp_distance(source.ts_ms, event.ts_ms))
                .min()?;
            if distance > TIMESTAMP_WINDOW_MS {
                return None;
            }
            Some((distance, group))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, group)| group)
}

fn nearest_event<'a>(
    source: &SessionEvent,
    candidates: &'a [&SessionEvent],
) -> Option<&'a SessionEvent> {
    candidates
        .iter()
        .min_by_key(|candidate| timestamp_distance(source.ts_ms, candidate.ts_ms))
        .copied()
}

fn canonical_pair(left: i64, right: i64) -> (i64, i64) {
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

fn timestamp_distance(left: i64, right: i64) -> i64 {
    (left - right).abs()
}

fn within_window(left: i64, right: i64) -> bool {
    timestamp_distance(left, right) <= TIMESTAMP_WINDOW_MS
}

fn timestamp_confidence(delta_ms: i64) -> f64 {
    let normalized = 1.0 - (delta_ms as f64 / TIMESTAMP_WINDOW_MS as f64);
    normalized.clamp(0.0, 0.99)
}

impl SessionEvent {
    fn from_raw_event(raw: RawEvent, ts_ms: i64) -> Self {
        let mut tool_use_ids = raw.tool_use_id.iter().cloned().collect::<Vec<_>>();
        let mut prompt_id = raw.prompt_id.clone();
        let mut agent_id = raw.agent_id.clone();
        let mut prompt_text = None;
        let mut is_prompt_event = false;
        let mut is_transcript_user_message = false;

        if raw.source == "hook" {
            if let Ok(HookEvent::UserPromptSubmit(input)) =
                serde_json::from_str::<HookEvent>(&raw.payload_json)
            {
                prompt_text = Some(input.prompt);
                is_prompt_event = true;
            }
        }

        if raw.source == "transcript" {
            if let Ok(entry) = serde_json::from_str::<TranscriptEntry>(&raw.payload_json) {
                match entry {
                    TranscriptEntry::Message(message) => {
                        if prompt_id.is_none() {
                            prompt_id = message.prompt_id.clone();
                        }
                        if agent_id.is_none() {
                            agent_id = message.agent_id.clone();
                        }

                        if matches!(message.kind, TranscriptMessageKind::User) {
                            is_prompt_event = prompt_id.is_some();
                            is_transcript_user_message = true;
                            prompt_text = transcript_prompt_text(&message.message.content);
                        }

                        extend_tool_use_ids(
                            &mut tool_use_ids,
                            transcript_tool_use_ids(&message.message.content),
                        );
                    }
                    TranscriptEntry::Progress(progress) => {
                        if prompt_id.is_none() {
                            prompt_id = progress.prompt_id.clone();
                        }
                        if agent_id.is_none() {
                            agent_id = progress.agent_id.clone();
                        }
                        extend_tool_use_ids(&mut tool_use_ids, progress.tool_use_id);
                        extend_tool_use_ids(&mut tool_use_ids, progress.parent_tool_use_id);
                    }
                    TranscriptEntry::ContentReplacement(entry) => {
                        if agent_id.is_none() {
                            agent_id = Some(entry.agent_id);
                        }
                        extend_tool_use_ids(
                            &mut tool_use_ids,
                            entry
                                .replacements
                                .into_iter()
                                .filter_map(|replacement| replacement.tool_use_id),
                        );
                    }
                    _ => {}
                }
            }
        }

        tool_use_ids.sort();
        tool_use_ids.dedup();

        Self {
            is_instructions_loaded: raw.source == "hook" && raw.event_type == "InstructionsLoaded",
            is_timestamp_tool_candidate: raw.source == "hook"
                && matches!(
                    raw.event_type.as_str(),
                    "ConfigChange" | "CwdChanged" | "FileChanged"
                ),
            raw,
            ts_ms,
            tool_use_ids,
            prompt_id,
            agent_id,
            prompt_text,
            is_prompt_event,
            is_transcript_user_message,
        }
    }
}

fn transcript_prompt_text(content: &TranscriptContent) -> Option<String> {
    match content {
        TranscriptContent::Text(text) => Some(text.clone()),
        TranscriptContent::Blocks(blocks) => {
            let text = blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text(block) => Some(block.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
    }
}

fn transcript_tool_use_ids(content: &TranscriptContent) -> Vec<String> {
    match content {
        TranscriptContent::Text(_) => Vec::new(),
        TranscriptContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse(block) => Some(block.id.clone()),
                ContentBlock::ToolResult(block) => Some(block.tool_use_id.clone()),
                _ => None,
            })
            .collect(),
    }
}

fn extend_tool_use_ids<I>(tool_use_ids: &mut Vec<String>, values: I)
where
    I: IntoIterator<Item = String>,
{
    for value in values {
        tool_use_ids.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewRawEvent;
    use serde_json::json;

    #[test]
    fn correlate_session_links_tool_prompt_timestamp_and_agent_events() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;
        let session_id = "session-1";

        let prompt_hook = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some(session_id),
            source: "hook",
            event_type: "UserPromptSubmit",
            ts: "2026-04-03T15:00:00.000Z",
            tool_use_id: None,
            prompt_id: None,
            agent_id: None,
            payload_json: &json!({
                "session_id": session_id,
                "transcript_path": "/tmp/transcript.jsonl",
                "cwd": "/workspace/claude-insight",
                "hook_event_name": "UserPromptSubmit",
                "permission_mode": "acceptEdits",
                "prompt": "Correlate this prompt",
            })
            .to_string(),
            claude_version: None,
            adapter_version: None,
        })?;

        let prompt_transcript = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some(session_id),
            source: "transcript",
            event_type: "user",
            ts: "2026-04-03T15:00:00.050Z",
            tool_use_id: None,
            prompt_id: Some("prompt-1"),
            agent_id: None,
            payload_json: &json!({
                "type": "user",
                "uuid": "uuid-user-1",
                "parentUuid": null,
                "logicalParentUuid": null,
                "isSidechain": false,
                "gitBranch": "ticket/mot-126",
                "agentId": null,
                "teamName": null,
                "agentName": null,
                "agentColor": null,
                "promptId": "prompt-1",
                "cwd": "/workspace/claude-insight",
                "userType": "external",
                "entrypoint": "sdk-cli",
                "sessionId": session_id,
                "timestamp": "2026-04-03T15:00:00.050Z",
                "version": "2.1.81",
                "message": {
                    "role": "user",
                    "content": "Correlate this prompt"
                }
            })
            .to_string(),
            claude_version: Some("2.1.81"),
            adapter_version: None,
        })?;

        let instructions = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some(session_id),
            source: "hook",
            event_type: "InstructionsLoaded",
            ts: "2026-04-03T15:00:00.060Z",
            tool_use_id: None,
            prompt_id: None,
            agent_id: None,
            payload_json: &json!({
                "session_id": session_id,
                "transcript_path": "/tmp/transcript.jsonl",
                "cwd": "/workspace/claude-insight",
                "hook_event_name": "InstructionsLoaded",
                "permission_mode": "acceptEdits",
                "file_path": "/workspace/claude-insight/CLAUDE.md",
                "memory_type": "Project",
                "load_reason": "session_start",
                "trigger_file_path": "/workspace/claude-insight/docs/DESIGN.md",
                "parent_file_path": null
            })
            .to_string(),
            claude_version: None,
            adapter_version: None,
        })?;

        let pre_tool = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some(session_id),
            source: "hook",
            event_type: "PreToolUse",
            ts: "2026-04-03T15:00:00.100Z",
            tool_use_id: Some("toolu_1"),
            prompt_id: None,
            agent_id: None,
            payload_json: &json!({
                "session_id": session_id,
                "transcript_path": "/tmp/transcript.jsonl",
                "cwd": "/workspace/claude-insight",
                "hook_event_name": "PreToolUse",
                "permission_mode": "acceptEdits",
                "tool_name": "Read",
                "tool_input": { "file_path": "docs/DESIGN.md" },
                "tool_use_id": "toolu_1"
            })
            .to_string(),
            claude_version: None,
            adapter_version: None,
        })?;

        let file_changed = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some(session_id),
            source: "hook",
            event_type: "FileChanged",
            ts: "2026-04-03T15:00:00.120Z",
            tool_use_id: None,
            prompt_id: None,
            agent_id: None,
            payload_json: &json!({
                "session_id": session_id,
                "transcript_path": "/tmp/transcript.jsonl",
                "cwd": "/workspace/claude-insight",
                "hook_event_name": "FileChanged",
                "permission_mode": "acceptEdits",
                "file_path": "/workspace/claude-insight/docs/DESIGN.md",
                "event": "modify"
            })
            .to_string(),
            claude_version: None,
            adapter_version: None,
        })?;

        let tool_result = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some(session_id),
            source: "transcript",
            event_type: "system",
            ts: "2026-04-03T15:00:00.150Z",
            tool_use_id: None,
            prompt_id: None,
            agent_id: None,
            payload_json: &json!({
                "type": "system",
                "uuid": "uuid-system-1",
                "parentUuid": "uuid-assistant-1",
                "logicalParentUuid": null,
                "isSidechain": false,
                "gitBranch": "ticket/mot-126",
                "agentId": null,
                "teamName": null,
                "agentName": null,
                "agentColor": null,
                "cwd": "/workspace/claude-insight",
                "userType": "external",
                "entrypoint": "sdk-cli",
                "sessionId": session_id,
                "timestamp": "2026-04-03T15:00:00.150Z",
                "version": "2.1.81",
                "subtype": "tool_result",
                "message": {
                    "role": "system",
                    "content": [
                        {
                            "type": "tool_result",
                            "content": "ok",
                            "is_error": false,
                            "tool_use_id": "toolu_1"
                        }
                    ]
                }
            })
            .to_string(),
            claude_version: Some("2.1.81"),
            adapter_version: None,
        })?;

        let post_tool = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some(session_id),
            source: "hook",
            event_type: "PostToolUse",
            ts: "2026-04-03T15:00:00.200Z",
            tool_use_id: Some("toolu_1"),
            prompt_id: None,
            agent_id: None,
            payload_json: &json!({
                "session_id": session_id,
                "transcript_path": "/tmp/transcript.jsonl",
                "cwd": "/workspace/claude-insight",
                "hook_event_name": "PostToolUse",
                "permission_mode": "acceptEdits",
                "tool_name": "Read",
                "tool_input": { "file_path": "docs/DESIGN.md" },
                "tool_response": { "content": "ok" },
                "tool_use_id": "toolu_1"
            })
            .to_string(),
            claude_version: None,
            adapter_version: None,
        })?;

        let subagent_start = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some(session_id),
            source: "hook",
            event_type: "SubagentStart",
            ts: "2026-04-03T15:00:01.000Z",
            tool_use_id: None,
            prompt_id: None,
            agent_id: Some("agent-1"),
            payload_json: &json!({
                "session_id": session_id,
                "transcript_path": "/tmp/transcript.jsonl",
                "cwd": "/workspace/claude-insight",
                "hook_event_name": "SubagentStart",
                "permission_mode": "acceptEdits",
                "agent_id": "agent-1",
                "agent_type": "reviewer"
            })
            .to_string(),
            claude_version: None,
            adapter_version: None,
        })?;

        let subagent_message = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some(session_id),
            source: "transcript",
            event_type: "assistant",
            ts: "2026-04-03T15:00:01.050Z",
            tool_use_id: None,
            prompt_id: None,
            agent_id: Some("agent-1"),
            payload_json: &json!({
                "type": "assistant",
                "uuid": "uuid-assistant-agent-1",
                "parentUuid": "uuid-user-1",
                "logicalParentUuid": null,
                "isSidechain": true,
                "gitBranch": "ticket/mot-126",
                "agentId": "agent-1",
                "teamName": "review",
                "agentName": "Reviewer",
                "agentColor": "#58a6ff",
                "cwd": "/workspace/claude-insight",
                "userType": "external",
                "entrypoint": "sdk-cli",
                "sessionId": session_id,
                "timestamp": "2026-04-03T15:00:01.050Z",
                "version": "2.1.81",
                "message": {
                    "role": "assistant",
                    "content": "Subagent review complete"
                }
            })
            .to_string(),
            claude_version: Some("2.1.81"),
            adapter_version: None,
        })?;

        let subagent_stop = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some(session_id),
            source: "hook",
            event_type: "SubagentStop",
            ts: "2026-04-03T15:00:01.100Z",
            tool_use_id: None,
            prompt_id: None,
            agent_id: Some("agent-1"),
            payload_json: &json!({
                "session_id": session_id,
                "transcript_path": "/tmp/transcript.jsonl",
                "cwd": "/workspace/claude-insight",
                "hook_event_name": "SubagentStop",
                "permission_mode": "acceptEdits",
                "agent_id": "agent-1",
                "agent_type": "reviewer",
                "agent_transcript_path": "/tmp/subagent-transcript.jsonl",
                "stop_hook_active": false,
                "last_assistant_message": "Subagent review complete"
            })
            .to_string(),
            claude_version: None,
            adapter_version: None,
        })?;

        let stats = db.correlate_session(session_id)?;
        let links = db.query_event_links_by_session(session_id)?;

        assert!(stats.links_created >= 8);
        assert_has_link(&links, pre_tool, post_tool, "tool_use_id");
        assert_has_link(&links, pre_tool, tool_result, "tool_use_id");
        assert_has_link(&links, post_tool, tool_result, "tool_use_id");
        assert_has_link(&links, prompt_hook, prompt_transcript, "prompt_id");
        assert_has_link(&links, instructions, prompt_transcript, "timestamp");
        assert_has_link(&links, file_changed, pre_tool, "timestamp");
        assert_has_link(&links, subagent_start, subagent_message, "agent_id");
        assert_has_link(&links, subagent_start, subagent_stop, "agent_id");
        assert_has_link(&links, subagent_message, subagent_stop, "agent_id");

        Ok(())
    }

    #[test]
    fn correlate_session_is_idempotent_and_handles_partial_tool_groups() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;
        let session_id = "session-partial";

        let pre_tool = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some(session_id),
            source: "hook",
            event_type: "PreToolUse",
            ts: "2026-04-03T15:10:00.000Z",
            tool_use_id: Some("toolu_partial"),
            prompt_id: None,
            agent_id: None,
            payload_json: &json!({
                "session_id": session_id,
                "transcript_path": "/tmp/transcript.jsonl",
                "cwd": "/workspace/claude-insight",
                "hook_event_name": "PreToolUse",
                "permission_mode": "acceptEdits",
                "tool_name": "Read",
                "tool_input": { "file_path": "docs/DESIGN.md" },
                "tool_use_id": "toolu_partial"
            })
            .to_string(),
            claude_version: None,
            adapter_version: None,
        })?;

        let tool_result = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some(session_id),
            source: "transcript",
            event_type: "system",
            ts: "2026-04-03T15:10:00.150Z",
            tool_use_id: None,
            prompt_id: None,
            agent_id: None,
            payload_json: &json!({
                "type": "system",
                "uuid": "uuid-partial-system-1",
                "parentUuid": "uuid-partial-assistant-1",
                "logicalParentUuid": null,
                "isSidechain": false,
                "gitBranch": "ticket/mot-126",
                "agentId": null,
                "teamName": null,
                "agentName": null,
                "agentColor": null,
                "cwd": "/workspace/claude-insight",
                "userType": "external",
                "entrypoint": "sdk-cli",
                "sessionId": session_id,
                "timestamp": "2026-04-03T15:10:00.150Z",
                "version": "2.1.81",
                "subtype": "tool_result",
                "message": {
                    "role": "system",
                    "content": [
                        {
                            "type": "tool_result",
                            "content": "partial",
                            "is_error": false,
                            "tool_use_id": "toolu_partial"
                        }
                    ]
                }
            })
            .to_string(),
            claude_version: Some("2.1.81"),
            adapter_version: None,
        })?;

        let first = db.correlate_session(session_id)?;
        let second = db.correlate_session(session_id)?;
        let links = db.query_event_links_by_session(session_id)?;

        assert_eq!(first.links_created, 1);
        assert_eq!(second.links_deleted, 1);
        assert_eq!(second.links_created, 1);
        assert_eq!(links.len(), 1);
        assert_has_link(&links, pre_tool, tool_result, "tool_use_id");

        Ok(())
    }

    fn assert_has_link(links: &[EventLink], left: i64, right: i64, link_type: &str) {
        let (source_event_id, target_event_id) = canonical_pair(left, right);
        assert!(
            links.iter().any(|link| {
                link.source_event_id == source_event_id
                    && link.target_event_id == target_event_id
                    && link.link_type == link_type
            }),
            "expected link {source_event_id}->{target_event_id} of type {link_type}, got {links:?}",
        );
    }
}
