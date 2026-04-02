use crate::{raw_store::map_raw_event, Database, RawEvent};
use claude_insight_types::{
    BaseHookInput, HookEvent, InstructionsLoadedInput, PermissionDeniedInput,
    PermissionRequestInput, PostToolUseFailureInput, PostToolUseInput, PreToolUseInput,
    SessionEndInput, SessionStartInput, UserPromptSubmitInput,
};
use rusqlite::{params, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizationStats {
    pub processed_events: usize,
    pub last_raw_event_id: i64,
}

pub(crate) fn normalize(database: &Database) -> rusqlite::Result<NormalizationStats> {
    let tx = database.conn.unchecked_transaction()?;
    let last_watermark = read_watermark(&tx)?;
    let events = load_events_after(&tx, last_watermark)?;
    let mut last_processed_id = last_watermark;

    for event in &events {
        normalize_event(&tx, event)?;
        last_processed_id = event.id;
    }

    if last_processed_id != last_watermark {
        tx.execute(
            "UPDATE normalization_state
             SET last_raw_event_id = ?1
             WHERE id = 1",
            params![last_processed_id],
        )?;
    }

    tx.commit()?;

    Ok(NormalizationStats {
        processed_events: events.len(),
        last_raw_event_id: last_processed_id,
    })
}

pub(crate) fn rebuild(database: &Database) -> rusqlite::Result<NormalizationStats> {
    let tx = database.conn.unchecked_transaction()?;

    tx.execute_batch(
        "
        DELETE FROM event_links;
        DELETE FROM permission_decisions;
        DELETE FROM instruction_loads;
        DELETE FROM config_snapshots;
        DELETE FROM tool_invocations;
        DELETE FROM prompts;
        DELETE FROM sessions;
        UPDATE normalization_state
        SET last_raw_event_id = 0
        WHERE id = 1;
        ",
    )?;

    tx.commit()?;

    normalize(database)
}

fn read_watermark(tx: &Transaction<'_>) -> rusqlite::Result<i64> {
    tx.query_row(
        "SELECT last_raw_event_id
         FROM normalization_state
         WHERE id = 1",
        [],
        |row| row.get(0),
    )
}

fn load_events_after(
    tx: &Transaction<'_>,
    last_raw_event_id: i64,
) -> rusqlite::Result<Vec<RawEvent>> {
    let mut statement = tx.prepare(
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
    let rows = statement.query_map(params![last_raw_event_id], map_raw_event)?;

    rows.collect()
}

fn normalize_event(tx: &Transaction<'_>, event: &RawEvent) -> rusqlite::Result<()> {
    match event.source.as_str() {
        "hook" => normalize_hook_event(tx, event),
        "transcript" => {
            if is_known_transcript_event(event.event_type.as_str()) {
                Ok(())
            } else {
                tracing::debug!(
                    raw_event_id = event.id,
                    source = event.source,
                    event_type = event.event_type,
                    "skipping unknown transcript event during normalization"
                );
                Ok(())
            }
        }
        _ => {
            tracing::debug!(
                raw_event_id = event.id,
                source = event.source,
                event_type = event.event_type,
                "skipping unsupported raw event source during normalization"
            );
            Ok(())
        }
    }
}

fn normalize_hook_event(tx: &Transaction<'_>, event: &RawEvent) -> rusqlite::Result<()> {
    match event.event_type.as_str() {
        "SessionStart" => match parse_hook_event(event)? {
            HookEvent::SessionStart(input) => normalize_session_start(tx, event, &input),
            _ => unreachable_variant(event.event_type.as_str()),
        },
        "SessionEnd" => match parse_hook_event(event)? {
            HookEvent::SessionEnd(input) => normalize_session_end(tx, event, &input),
            _ => unreachable_variant(event.event_type.as_str()),
        },
        "PreToolUse" => match parse_hook_event(event)? {
            HookEvent::PreToolUse(input) => normalize_pre_tool_use(tx, event, &input),
            _ => unreachable_variant(event.event_type.as_str()),
        },
        "PostToolUse" => match parse_hook_event(event)? {
            HookEvent::PostToolUse(input) => normalize_post_tool_use(tx, event, &input),
            _ => unreachable_variant(event.event_type.as_str()),
        },
        "PostToolUseFailure" => match parse_hook_event(event)? {
            HookEvent::PostToolUseFailure(input) => {
                normalize_post_tool_use_failure(tx, event, &input)
            }
            _ => unreachable_variant(event.event_type.as_str()),
        },
        "PermissionRequest" => match parse_hook_event(event)? {
            HookEvent::PermissionRequest(input) => normalize_permission_request(tx, event, &input),
            _ => unreachable_variant(event.event_type.as_str()),
        },
        "PermissionDenied" => match parse_hook_event(event)? {
            HookEvent::PermissionDenied(input) => normalize_permission_denied(tx, event, &input),
            _ => unreachable_variant(event.event_type.as_str()),
        },
        "InstructionsLoaded" => match parse_hook_event(event)? {
            HookEvent::InstructionsLoaded(input) => normalize_instruction_load(tx, event, &input),
            _ => unreachable_variant(event.event_type.as_str()),
        },
        "UserPromptSubmit" => match parse_hook_event(event)? {
            HookEvent::UserPromptSubmit(input) => normalize_prompt(tx, event, &input),
            _ => unreachable_variant(event.event_type.as_str()),
        },
        "Notification" | "Stop" | "StopFailure" | "SubagentStart" | "SubagentStop"
        | "PreCompact" | "PostCompact" | "Setup" | "TeammateIdle" | "TaskCreated"
        | "TaskCompleted" | "Elicitation" | "ElicitationResult" | "ConfigChange"
        | "WorktreeCreate" | "WorktreeRemove" | "CwdChanged" | "FileChanged" => Ok(()),
        _ => {
            tracing::debug!(
                raw_event_id = event.id,
                event_type = event.event_type,
                "skipping unknown hook event during normalization"
            );
            Ok(())
        }
    }
}

fn parse_hook_event(event: &RawEvent) -> rusqlite::Result<HookEvent> {
    let mut value =
        serde_json::from_str::<serde_json::Value>(&event.payload_json).map_err(|error| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(format!(
                "failed to parse hook payload for {} (raw_event_id={}): {error}",
                event.event_type, event.id
            ))))
        })?;

    if let Some(object) = value.as_object_mut() {
        object
            .entry("hook_event_name")
            .or_insert_with(|| serde_json::Value::String(event.event_type.clone()));
        object.entry("transcript_path").or_insert_with(|| "".into());
        object.entry("cwd").or_insert_with(|| "".into());
        object
            .entry("permission_mode")
            .or_insert(serde_json::Value::Null);

        if let Some(session_id) = &event.session_id {
            object
                .entry("session_id")
                .or_insert_with(|| serde_json::Value::String(session_id.clone()));
        }
        if let Some(agent_id) = &event.agent_id {
            object
                .entry("agent_id")
                .or_insert_with(|| serde_json::Value::String(agent_id.clone()));
        }
    }

    serde_json::from_value(value).map_err(|error| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(format!(
            "failed to deserialize hook payload for {} (raw_event_id={}): {error}",
            event.event_type, event.id
        ))))
    })
}

fn normalize_session_start(
    tx: &Transaction<'_>,
    event: &RawEvent,
    input: &SessionStartInput,
) -> rusqlite::Result<()> {
    upsert_session(
        tx,
        &input.base,
        event.claude_version.as_deref(),
        input.model.as_deref(),
        Some(event.ts.as_str()),
        None,
        None,
        Some(input.source.as_str()),
    )
}

fn normalize_session_end(
    tx: &Transaction<'_>,
    event: &RawEvent,
    input: &SessionEndInput,
) -> rusqlite::Result<()> {
    upsert_session(
        tx,
        &input.base,
        event.claude_version.as_deref(),
        None,
        None,
        Some(event.ts.as_str()),
        Some(input.reason.as_str()),
        Some("hook"),
    )
}

fn normalize_pre_tool_use(
    tx: &Transaction<'_>,
    event: &RawEvent,
    input: &PreToolUseInput,
) -> rusqlite::Result<()> {
    upsert_session(
        tx,
        &input.base,
        event.claude_version.as_deref(),
        None,
        None,
        None,
        None,
        Some("hook"),
    )?;

    ensure_prompt_exists(
        tx,
        &input.base.session_id,
        event.prompt_id.as_deref(),
        event.ts.as_str(),
    )?;

    let tool_input_json = input.tool_input.to_string();
    let (is_mcp, mcp_server_name) = tool_metadata(&input.tool_name);

    tx.execute(
        "INSERT INTO tool_invocations (
            id,
            session_id,
            prompt_id,
            tool_name,
            tool_input_json,
            tool_input_hash,
            is_mcp,
            mcp_server_name,
            agent_id,
            pre_hook_ts
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
            session_id = excluded.session_id,
            prompt_id = COALESCE(excluded.prompt_id, tool_invocations.prompt_id),
            tool_name = excluded.tool_name,
            tool_input_json = COALESCE(tool_invocations.tool_input_json, excluded.tool_input_json),
            tool_input_hash = COALESCE(tool_invocations.tool_input_hash, excluded.tool_input_hash),
            is_mcp = excluded.is_mcp,
            mcp_server_name = COALESCE(excluded.mcp_server_name, tool_invocations.mcp_server_name),
            agent_id = COALESCE(excluded.agent_id, tool_invocations.agent_id),
            pre_hook_ts = COALESCE(tool_invocations.pre_hook_ts, excluded.pre_hook_ts)",
        params![
            input.tool_use_id,
            input.base.session_id,
            event.prompt_id,
            input.tool_name,
            tool_input_json,
            stable_hash(tool_input_json.as_str()),
            is_mcp,
            mcp_server_name,
            event.agent_id.as_deref().or(input.base.agent_id.as_deref()),
            event.ts,
        ],
    )?;

    Ok(())
}

fn normalize_post_tool_use(
    tx: &Transaction<'_>,
    event: &RawEvent,
    input: &PostToolUseInput,
) -> rusqlite::Result<()> {
    upsert_session(
        tx,
        &input.base,
        event.claude_version.as_deref(),
        None,
        None,
        None,
        None,
        Some("hook"),
    )?;

    ensure_prompt_exists(
        tx,
        &input.base.session_id,
        event.prompt_id.as_deref(),
        event.ts.as_str(),
    )?;

    let tool_input_json = input.tool_input.to_string();
    let tool_response_json = input.tool_response.to_string();
    let (is_mcp, mcp_server_name) = tool_metadata(&input.tool_name);

    tx.execute(
        "INSERT INTO tool_invocations (
            id,
            session_id,
            prompt_id,
            tool_name,
            tool_input_json,
            tool_input_hash,
            tool_response_json,
            tool_response_hash,
            is_mcp,
            mcp_server_name,
            agent_id,
            post_hook_ts,
            success
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1)
         ON CONFLICT(id) DO UPDATE SET
            session_id = excluded.session_id,
            prompt_id = COALESCE(excluded.prompt_id, tool_invocations.prompt_id),
            tool_name = excluded.tool_name,
            tool_input_json = COALESCE(tool_invocations.tool_input_json, excluded.tool_input_json),
            tool_input_hash = COALESCE(tool_invocations.tool_input_hash, excluded.tool_input_hash),
            tool_response_json = excluded.tool_response_json,
            tool_response_hash = excluded.tool_response_hash,
            is_mcp = excluded.is_mcp,
            mcp_server_name = COALESCE(excluded.mcp_server_name, tool_invocations.mcp_server_name),
            agent_id = COALESCE(excluded.agent_id, tool_invocations.agent_id),
            post_hook_ts = excluded.post_hook_ts,
            success = 1,
            error_text = NULL",
        params![
            input.tool_use_id,
            input.base.session_id,
            event.prompt_id,
            input.tool_name,
            tool_input_json,
            stable_hash(tool_input_json.as_str()),
            tool_response_json,
            stable_hash(tool_response_json.as_str()),
            is_mcp,
            mcp_server_name,
            event.agent_id.as_deref().or(input.base.agent_id.as_deref()),
            event.ts,
        ],
    )?;

    Ok(())
}

fn normalize_post_tool_use_failure(
    tx: &Transaction<'_>,
    event: &RawEvent,
    input: &PostToolUseFailureInput,
) -> rusqlite::Result<()> {
    upsert_session(
        tx,
        &input.base,
        event.claude_version.as_deref(),
        None,
        None,
        None,
        None,
        Some("hook"),
    )?;

    ensure_prompt_exists(
        tx,
        &input.base.session_id,
        event.prompt_id.as_deref(),
        event.ts.as_str(),
    )?;

    let tool_input_json = input.tool_input.to_string();
    let (is_mcp, mcp_server_name) = tool_metadata(&input.tool_name);

    tx.execute(
        "INSERT INTO tool_invocations (
            id,
            session_id,
            prompt_id,
            tool_name,
            tool_input_json,
            tool_input_hash,
            is_mcp,
            mcp_server_name,
            agent_id,
            post_hook_ts,
            success,
            error_text
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11)
         ON CONFLICT(id) DO UPDATE SET
            session_id = excluded.session_id,
            prompt_id = COALESCE(excluded.prompt_id, tool_invocations.prompt_id),
            tool_name = excluded.tool_name,
            tool_input_json = COALESCE(tool_invocations.tool_input_json, excluded.tool_input_json),
            tool_input_hash = COALESCE(tool_invocations.tool_input_hash, excluded.tool_input_hash),
            is_mcp = excluded.is_mcp,
            mcp_server_name = COALESCE(excluded.mcp_server_name, tool_invocations.mcp_server_name),
            agent_id = COALESCE(excluded.agent_id, tool_invocations.agent_id),
            post_hook_ts = excluded.post_hook_ts,
            success = 0,
            error_text = excluded.error_text",
        params![
            input.tool_use_id,
            input.base.session_id,
            event.prompt_id,
            input.tool_name,
            tool_input_json,
            stable_hash(tool_input_json.as_str()),
            is_mcp,
            mcp_server_name,
            event.agent_id.as_deref().or(input.base.agent_id.as_deref()),
            event.ts,
            input.error,
        ],
    )?;

    Ok(())
}

fn normalize_permission_request(
    tx: &Transaction<'_>,
    event: &RawEvent,
    input: &PermissionRequestInput,
) -> rusqlite::Result<()> {
    upsert_session(
        tx,
        &input.base,
        event.claude_version.as_deref(),
        None,
        None,
        None,
        None,
        Some("hook"),
    )?;

    let rule_text = input.permission_suggestions.as_ref().map(|suggestions| {
        suggestions
            .iter()
            .map(|suggestion| suggestion.rule.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    });

    tx.execute(
        "INSERT INTO permission_decisions (
            session_id,
            tool_invocation_id,
            decision,
            source,
            rule_text,
            permission_mode,
            ts
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            input.base.session_id,
            Option::<String>::None,
            "request",
            "hook",
            rule_text,
            input.base.permission_mode,
            event.ts,
        ],
    )?;

    Ok(())
}

fn normalize_permission_denied(
    tx: &Transaction<'_>,
    event: &RawEvent,
    input: &PermissionDeniedInput,
) -> rusqlite::Result<()> {
    upsert_session(
        tx,
        &input.base,
        event.claude_version.as_deref(),
        None,
        None,
        None,
        None,
        Some("hook"),
    )?;

    upsert_tool_invocation_stub(
        tx,
        &input.base,
        event.prompt_id.as_deref(),
        &input.tool_use_id,
        &input.tool_name,
        &input.tool_input,
        Some(event.ts.as_str()),
        None,
        Some(false),
        Some(input.reason.as_str()),
    )?;

    tx.execute(
        "INSERT INTO permission_decisions (
            session_id,
            tool_invocation_id,
            decision,
            source,
            rule_text,
            permission_mode,
            ts
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            input.base.session_id,
            input.tool_use_id,
            "denied",
            "hook",
            input.reason,
            input.base.permission_mode,
            event.ts,
        ],
    )?;

    Ok(())
}

fn normalize_instruction_load(
    tx: &Transaction<'_>,
    event: &RawEvent,
    input: &InstructionsLoadedInput,
) -> rusqlite::Result<()> {
    upsert_session(
        tx,
        &input.base,
        event.claude_version.as_deref(),
        None,
        None,
        None,
        None,
        Some("hook"),
    )?;

    tx.execute(
        "INSERT INTO instruction_loads (
            session_id,
            file_path,
            memory_type,
            load_reason,
            trigger_file_path,
            parent_file_path,
            content_hash,
            ts
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            input.base.session_id,
            input.file_path,
            input.memory_type,
            input.load_reason,
            input.trigger_file_path,
            input.parent_file_path,
            stable_hash(input.file_path.as_str()),
            event.ts,
        ],
    )?;

    Ok(())
}

fn normalize_prompt(
    tx: &Transaction<'_>,
    event: &RawEvent,
    input: &UserPromptSubmitInput,
) -> rusqlite::Result<()> {
    upsert_session(
        tx,
        &input.base,
        event.claude_version.as_deref(),
        None,
        None,
        None,
        None,
        Some("hook"),
    )?;

    let prompt_id = event
        .prompt_id
        .clone()
        .unwrap_or_else(|| format!("prompt-raw-{}", event.id));

    tx.execute(
        "INSERT INTO prompts (
            id,
            session_id,
            prompt_text,
            prompt_hash,
            ts
         ) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
            session_id = excluded.session_id,
            prompt_text = excluded.prompt_text,
            prompt_hash = excluded.prompt_hash,
            ts = excluded.ts",
        params![
            prompt_id,
            input.base.session_id,
            input.prompt,
            stable_hash(input.prompt.as_str()),
            event.ts,
        ],
    )?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn upsert_session(
    tx: &Transaction<'_>,
    base: &BaseHookInput,
    claude_version: Option<&str>,
    model: Option<&str>,
    start_ts: Option<&str>,
    end_ts: Option<&str>,
    end_reason: Option<&str>,
    source: Option<&str>,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO sessions (
            id,
            transcript_path,
            cwd,
            project_dir,
            claude_version,
            model,
            permission_mode,
            start_ts,
            end_ts,
            end_reason,
            source
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(id) DO UPDATE SET
            transcript_path = COALESCE(excluded.transcript_path, sessions.transcript_path),
            cwd = COALESCE(excluded.cwd, sessions.cwd),
            project_dir = COALESCE(excluded.project_dir, sessions.project_dir),
            claude_version = COALESCE(excluded.claude_version, sessions.claude_version),
            model = COALESCE(excluded.model, sessions.model),
            permission_mode = COALESCE(excluded.permission_mode, sessions.permission_mode),
            start_ts = COALESCE(sessions.start_ts, excluded.start_ts),
            end_ts = COALESCE(excluded.end_ts, sessions.end_ts),
            end_reason = COALESCE(excluded.end_reason, sessions.end_reason),
            source = COALESCE(excluded.source, sessions.source)",
        params![
            base.session_id,
            base.transcript_path,
            base.cwd,
            base.cwd,
            claude_version,
            model,
            base.permission_mode,
            start_ts,
            end_ts,
            end_reason,
            source,
        ],
    )?;

    Ok(())
}

fn ensure_prompt_exists(
    tx: &Transaction<'_>,
    session_id: &str,
    prompt_id: Option<&str>,
    ts: &str,
) -> rusqlite::Result<()> {
    if let Some(prompt_id) = prompt_id {
        tx.execute(
            "INSERT INTO prompts (id, session_id, ts)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
                session_id = COALESCE(excluded.session_id, prompts.session_id),
                ts = COALESCE(prompts.ts, excluded.ts)",
            params![prompt_id, session_id, ts],
        )?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn upsert_tool_invocation_stub(
    tx: &Transaction<'_>,
    base: &BaseHookInput,
    prompt_id: Option<&str>,
    tool_use_id: &str,
    tool_name: &str,
    tool_input: &serde_json::Value,
    pre_hook_ts: Option<&str>,
    post_hook_ts: Option<&str>,
    success: Option<bool>,
    error_text: Option<&str>,
) -> rusqlite::Result<()> {
    ensure_prompt_exists(
        tx,
        &base.session_id,
        prompt_id,
        post_hook_ts.or(pre_hook_ts).unwrap_or(""),
    )?;

    let tool_input_json = tool_input.to_string();
    let (is_mcp, mcp_server_name) = tool_metadata(tool_name);

    tx.execute(
        "INSERT INTO tool_invocations (
            id,
            session_id,
            prompt_id,
            tool_name,
            tool_input_json,
            tool_input_hash,
            is_mcp,
            mcp_server_name,
            agent_id,
            pre_hook_ts,
            post_hook_ts,
            success,
            error_text
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(id) DO UPDATE SET
            session_id = excluded.session_id,
            prompt_id = COALESCE(excluded.prompt_id, tool_invocations.prompt_id),
            tool_name = excluded.tool_name,
            tool_input_json = COALESCE(tool_invocations.tool_input_json, excluded.tool_input_json),
            tool_input_hash = COALESCE(tool_invocations.tool_input_hash, excluded.tool_input_hash),
            is_mcp = excluded.is_mcp,
            mcp_server_name = COALESCE(excluded.mcp_server_name, tool_invocations.mcp_server_name),
            agent_id = COALESCE(excluded.agent_id, tool_invocations.agent_id),
            pre_hook_ts = COALESCE(tool_invocations.pre_hook_ts, excluded.pre_hook_ts),
            post_hook_ts = COALESCE(excluded.post_hook_ts, tool_invocations.post_hook_ts),
            success = COALESCE(excluded.success, tool_invocations.success),
            error_text = COALESCE(excluded.error_text, tool_invocations.error_text)",
        params![
            tool_use_id,
            base.session_id,
            prompt_id,
            tool_name,
            tool_input_json,
            stable_hash(tool_input_json.as_str()),
            is_mcp,
            mcp_server_name,
            base.agent_id,
            pre_hook_ts,
            post_hook_ts,
            success,
            error_text,
        ],
    )?;

    Ok(())
}

fn tool_metadata(tool_name: &str) -> (bool, Option<String>) {
    match tool_name.strip_prefix("mcp__") {
        Some(rest) => (
            true,
            rest.split("__")
                .next()
                .filter(|server_name| !server_name.is_empty())
                .map(str::to_owned),
        ),
        None => (false, None),
    }
}

fn is_known_transcript_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "user"
            | "assistant"
            | "system"
            | "attachment"
            | "progress"
            | "summary"
            | "custom-title"
            | "ai-title"
            | "last-prompt"
            | "task-summary"
            | "tag"
            | "agent-name"
            | "agent-color"
            | "agent-setting"
            | "pr-link"
            | "file-history-snapshot"
            | "attribution-snapshot"
            | "queue-operation"
            | "speculation-accept"
            | "mode"
            | "worktree-state"
            | "content-replacement"
            | "marble-origami-commit"
            | "marble-origami-snapshot"
    )
}

fn stable_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;

    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    format!("{hash:016x}")
}

fn unreachable_variant(event_type: &str) -> rusqlite::Result<()> {
    Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
        std::io::Error::other(format!(
            "deserialized hook payload did not match expected event type {event_type}"
        )),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewRawEvent;
    use serde_json::Value;
    use std::{fs, path::PathBuf};

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
    }

    fn hook_fixture_dir() -> PathBuf {
        fixture_root().join("hooks")
    }

    #[test]
    fn normalize_materializes_relevant_hook_rows_and_tracks_watermark() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;
        let mut hook_fixtures = fs::read_dir(hook_fixture_dir())
            .unwrap_or_else(|error| panic!("failed to read hook fixtures: {error}"))
            .map(|entry| {
                entry
                    .unwrap_or_else(|error| panic!("failed to read fixture entry: {error}"))
                    .path()
            })
            .collect::<Vec<_>>();
        hook_fixtures.sort();

        let transcript_fixture = fixture_root().join("transcripts/comprehensive.jsonl");

        for (index, path) in hook_fixtures.iter().enumerate() {
            let payload = fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            insert_fixture_event(&db, "hook", &payload, index + 1)?;
        }

        for (index, line) in fs::read_to_string(&transcript_fixture)
            .unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", transcript_fixture.display())
            })
            .lines()
            .filter(|line| !line.trim().is_empty())
            .enumerate()
        {
            insert_fixture_event(&db, "transcript", line, hook_fixtures.len() + index + 1)?;
        }

        let stats = db.normalize()?;

        assert_eq!(stats.processed_events, hook_fixtures.len() + 24);
        assert_eq!(
            stats.last_raw_event_id,
            i64::try_from(hook_fixtures.len() + 24).unwrap_or(0)
        );
        assert_eq!(db.normalization_watermark()?, stats.last_raw_event_id);

        let session_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
        let prompt_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM prompts", [], |row| row.get(0))?;
        let tool_count: i64 =
            db.conn
                .query_row("SELECT COUNT(*) FROM tool_invocations", [], |row| {
                    row.get(0)
                })?;
        let permission_count: i64 =
            db.conn
                .query_row("SELECT COUNT(*) FROM permission_decisions", [], |row| {
                    row.get(0)
                })?;
        let instruction_count: i64 =
            db.conn
                .query_row("SELECT COUNT(*) FROM instruction_loads", [], |row| {
                    row.get(0)
                })?;

        assert_eq!(session_count, 2);
        assert_eq!(prompt_count, 1);
        assert_eq!(tool_count, 2);
        assert_eq!(permission_count, 2);
        assert_eq!(instruction_count, 1);

        let session_end = db.conn.query_row(
            "SELECT end_reason
             FROM sessions
             WHERE id = '11111111-1111-4111-8111-111111111111'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let tool_success = db.conn.query_row(
            "SELECT success
             FROM tool_invocations
             WHERE id = 'toolu_01Bqr78WkjBpvgdnN3GGhDB1'",
            [],
            |row| row.get::<_, Option<bool>>(0),
        )?;

        assert_eq!(session_end, "prompt_input_exit");
        assert_eq!(tool_success, Some(true));

        Ok(())
    }

    #[test]
    fn normalize_is_incremental_and_idempotent_across_reruns() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;
        let fixtures = [
            fixture_root().join("hooks/SessionStart.json"),
            fixture_root().join("hooks/UserPromptSubmit.json"),
            fixture_root().join("hooks/PreToolUse.json"),
            fixture_root().join("hooks/PostToolUse.json"),
            fixture_root().join("hooks/PermissionRequest.json"),
            fixture_root().join("hooks/PermissionDenied.json"),
            fixture_root().join("hooks/InstructionsLoaded.json"),
            fixture_root().join("hooks/SessionEnd.json"),
        ];

        for (index, path) in fixtures.iter().enumerate() {
            let payload = fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            insert_fixture_event(&db, "hook", &payload, index + 1)?;
        }

        let first = db.normalize()?;
        let second = db.normalize()?;

        assert_eq!(first.processed_events, fixtures.len());
        assert_eq!(second.processed_events, 0);
        assert_eq!(
            db.normalization_watermark()?,
            i64::try_from(fixtures.len()).unwrap_or(0)
        );

        let prompt_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM prompts", [], |row| row.get(0))?;
        let tool_count: i64 =
            db.conn
                .query_row("SELECT COUNT(*) FROM tool_invocations", [], |row| {
                    row.get(0)
                })?;
        let permission_count: i64 =
            db.conn
                .query_row("SELECT COUNT(*) FROM permission_decisions", [], |row| {
                    row.get(0)
                })?;
        let instruction_count: i64 =
            db.conn
                .query_row("SELECT COUNT(*) FROM instruction_loads", [], |row| {
                    row.get(0)
                })?;

        assert_eq!(prompt_count, 1);
        assert_eq!(tool_count, 2);
        assert_eq!(permission_count, 2);
        assert_eq!(instruction_count, 1);

        Ok(())
    }

    #[test]
    fn normalize_skips_unknown_events_but_advances_watermark() -> rusqlite::Result<()> {
        let db = Database::new(":memory:")?;

        let raw_id = db.insert_raw_event_record(&NewRawEvent {
            session_id: Some("session-1"),
            source: "hook",
            event_type: "FutureEvent",
            ts: "2026-04-03T15:00:00Z",
            tool_use_id: None,
            prompt_id: None,
            agent_id: None,
            payload_json: "{\"hook_event_name\":\"FutureEvent\",\"session_id\":\"session-1\"}",
            claude_version: None,
            adapter_version: None,
        })?;

        let stats = db.normalize()?;
        let session_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;

        assert_eq!(stats.processed_events, 1);
        assert_eq!(stats.last_raw_event_id, raw_id);
        assert_eq!(db.normalization_watermark()?, raw_id);
        assert_eq!(session_count, 0);

        Ok(())
    }

    fn insert_fixture_event(
        db: &Database,
        source: &str,
        payload: &str,
        ordinal: usize,
    ) -> rusqlite::Result<()> {
        let value: Value = serde_json::from_str(payload).unwrap_or_else(|error| {
            panic!("fixture payload should be valid JSON: {error}; payload={payload}")
        });
        let event_type = event_type_for_fixture(source, &value);
        let ts = value
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("2026-04-03T15:{:02}:00Z", ordinal % 60));

        db.insert_raw_event_record(&NewRawEvent {
            session_id: value
                .get("session_id")
                .or_else(|| value.get("sessionId"))
                .and_then(Value::as_str),
            source,
            event_type: event_type.as_str(),
            ts: ts.as_str(),
            tool_use_id: value
                .get("tool_use_id")
                .or_else(|| value.get("toolUseID"))
                .and_then(Value::as_str),
            prompt_id: value
                .get("prompt_id")
                .or_else(|| value.get("promptId"))
                .and_then(Value::as_str),
            agent_id: value
                .get("agent_id")
                .or_else(|| value.get("agentId"))
                .and_then(Value::as_str),
            payload_json: payload,
            claude_version: value
                .get("claude_version")
                .or_else(|| value.get("version"))
                .and_then(Value::as_str),
            adapter_version: None,
        })?;

        Ok(())
    }

    fn event_type_for_fixture(source: &str, value: &Value) -> String {
        match source {
            "hook" => value
                .get("hook_event_name")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("hook fixture missing hook_event_name: {value}"))
                .to_owned(),
            "transcript" => value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("transcript fixture missing type: {value}"))
                .to_owned(),
            other => panic!("unsupported fixture source: {other}"),
        }
    }
}
