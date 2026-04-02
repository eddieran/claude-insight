#![deny(clippy::expect_used, clippy::unwrap_used)]

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use claude_insight_capture::{
    hooks_router_with_config, BacklogProcessor, BacklogWriter, CaptureConfig, TranscriptTailer,
    TranscriptTailerConfig,
};
use claude_insight_storage::{Database, RawEvent};
use rusqlite::{params, Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    error::Error,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tempfile::TempDir;
use tower::util::ServiceExt;

const FIXTURE_TOOL_USE_ID: &str = "toolu_01MOT115fixture";

#[tokio::test]
async fn hook_events_round_trip_into_sqlite() -> Result<(), Box<dyn Error>> {
    let db_uri = shared_memory_uri("hook-roundtrip");
    let database = Database::new(&db_uri)?;
    let session_id = session_id_from_hook_fixture("SessionStart")?;
    let app = hooks_router_with_config(CaptureConfig::default().with_database_path(&db_uri));

    for fixture in ["SessionStart", "PreToolUse", "PostToolUse"] {
        post_hook(&app, &read_hook_fixture(fixture)?).await?;
    }

    let events = database.query_raw_events_by_session(&session_id)?;

    assert_eq!(events.len(), 3);
    assert_eq!(
        event_types(&events),
        vec!["SessionStart", "PreToolUse", "PostToolUse"]
    );
    assert!(events.iter().all(|event| event.source == "hook"));
    assert_eq!(
        events
            .iter()
            .filter_map(|event| event.tool_use_id.as_deref())
            .collect::<Vec<_>>(),
        vec![
            "toolu_01Bqr78WkjBpvgdnN3GGhDB1",
            "toolu_01Bqr78WkjBpvgdnN3GGhDB1",
        ]
    );

    Ok(())
}

#[test]
fn transcript_entries_round_trip_into_raw_events() -> Result<(), Box<dyn Error>> {
    let db_uri = shared_memory_uri("transcript-roundtrip");
    let database = Database::new(&db_uri)?;
    let workspace = TempDir::new()?;
    let transcript_root = workspace.path().join("transcripts");
    fs::create_dir_all(&transcript_root)?;
    let transcript_path = transcript_root.join("session-transcript.jsonl");
    let mut tailer = TranscriptTailer::new(TranscriptTailerConfig {
        transcript_root: transcript_root.clone(),
        positions_path: workspace.path().join("offsets.json"),
        database_path: PathBuf::from(&db_uri),
    })?;

    write_transcript(
        &transcript_path,
        &build_transcript_fixture_stream("session-transcript", 50)?,
    )?;

    let ingested = tailer.ingest_path(&transcript_path)?;
    let events = database.query_raw_events_by_session("session-transcript")?;

    assert_eq!(ingested, 50);
    assert_eq!(events.len(), 50);
    assert!(events.iter().all(|event| event.source == "transcript"));
    assert!(events
        .iter()
        .any(|event| event.tool_use_id.as_deref() == Some(FIXTURE_TOOL_USE_ID)));

    Ok(())
}

#[tokio::test]
async fn multi_source_correlation_links_events_by_tool_use_id() -> Result<(), Box<dyn Error>> {
    let db_uri = shared_memory_uri("correlation");
    let database = Database::new(&db_uri)?;
    let sql = open_query_connection(&db_uri)?;
    let app = hooks_router_with_config(CaptureConfig::default().with_database_path(&db_uri));
    let workspace = TempDir::new()?;
    let transcript_root = workspace.path().join("transcripts");
    fs::create_dir_all(&transcript_root)?;
    let transcript_path = transcript_root.join("session-correlation.jsonl");
    let mut tailer = TranscriptTailer::new(TranscriptTailerConfig {
        transcript_root: transcript_root.clone(),
        positions_path: workspace.path().join("offsets.json"),
        database_path: PathBuf::from(&db_uri),
    })?;
    let session_id = "session-correlation";

    post_hook(
        &app,
        &rewrite_hook_fixture("SessionStart", session_id, None, None)?,
    )
    .await?;
    post_hook(
        &app,
        &rewrite_hook_fixture(
            "PreToolUse",
            session_id,
            Some(FIXTURE_TOOL_USE_ID),
            Some("Read"),
        )?,
    )
    .await?;
    post_hook(
        &app,
        &rewrite_hook_fixture(
            "PostToolUse",
            session_id,
            Some(FIXTURE_TOOL_USE_ID),
            Some("Read"),
        )?,
    )
    .await?;

    write_transcript(
        &transcript_path,
        &rewrite_transcript_fixture("comprehensive.jsonl", session_id, Some(FIXTURE_TOOL_USE_ID))?,
    )?;
    assert_eq!(tailer.ingest_path(&transcript_path)?, 24);

    let stats = database.correlate_session(session_id)?;
    assert!(stats.links_created >= 6);

    let pre_tool = raw_event_id(
        &sql,
        session_id,
        "hook",
        "PreToolUse",
        Some(FIXTURE_TOOL_USE_ID),
    )?;
    let post_tool = raw_event_id(
        &sql,
        session_id,
        "hook",
        "PostToolUse",
        Some(FIXTURE_TOOL_USE_ID),
    )?;
    let assistant = raw_event_id(
        &sql,
        session_id,
        "transcript",
        "TranscriptAssistantMessage",
        Some(FIXTURE_TOOL_USE_ID),
    )?;
    let system = raw_event_id(
        &sql,
        session_id,
        "transcript",
        "TranscriptSystemMessage",
        Some(FIXTURE_TOOL_USE_ID),
    )?;

    assert_link(&sql, pre_tool, post_tool, "tool_use_id")?;
    assert_link(&sql, pre_tool, assistant, "tool_use_id")?;
    assert_link(&sql, pre_tool, system, "tool_use_id")?;
    assert_link(&sql, post_tool, assistant, "tool_use_id")?;
    assert_link(&sql, post_tool, system, "tool_use_id")?;
    assert_link(&sql, assistant, system, "tool_use_id")?;

    Ok(())
}

#[tokio::test]
async fn concurrent_sessions_do_not_cross_contaminate() -> Result<(), Box<dyn Error>> {
    let db_uri = shared_memory_uri("concurrent-sessions");
    let database = Database::new(&db_uri)?;
    let sql = open_query_connection(&db_uri)?;
    let app = hooks_router_with_config(CaptureConfig::default().with_database_path(&db_uri));
    let workspace = TempDir::new()?;
    let transcript_root = workspace.path().join("transcripts");
    fs::create_dir_all(&transcript_root)?;
    let mut tailer = TranscriptTailer::new(TranscriptTailerConfig {
        transcript_root: transcript_root.clone(),
        positions_path: workspace.path().join("offsets.json"),
        database_path: PathBuf::from(&db_uri),
    })?;

    for session_id in ["session-a", "session-b"] {
        post_hook(
            &app,
            &rewrite_hook_fixture("PreToolUse", session_id, Some("toolu_shared"), Some("Read"))?,
        )
        .await?;
        post_hook(
            &app,
            &rewrite_hook_fixture(
                "PostToolUse",
                session_id,
                Some("toolu_shared"),
                Some("Read"),
            )?,
        )
        .await?;

        let transcript_path = transcript_root.join(format!("{session_id}.jsonl"));
        write_transcript(
            &transcript_path,
            &tool_transcript_lines(session_id, "toolu_shared")?,
        )?;
        let _ = tailer.ingest_path(&transcript_path)?;
        let _ = database.correlate_session(session_id)?;
    }

    let cross_session_links: i64 = sql.query_row(
        "
        SELECT COUNT(*)
        FROM event_links
        JOIN raw_events AS source_events
          ON source_events.id = event_links.source_event_id
        JOIN raw_events AS target_events
          ON target_events.id = event_links.target_event_id
        WHERE source_events.session_id != target_events.session_id
        ",
        [],
        |row| row.get(0),
    )?;

    assert_eq!(cross_session_links, 0);
    assert_eq!(database.query_raw_events_by_session("session-a")?.len(), 4);
    assert_eq!(database.query_raw_events_by_session("session-b")?.len(), 4);

    Ok(())
}

#[tokio::test]
async fn normalization_populates_typed_tables() -> Result<(), Box<dyn Error>> {
    let db_uri = shared_memory_uri("normalization");
    let database = Database::new(&db_uri)?;
    let sql = open_query_connection(&db_uri)?;
    let app = hooks_router_with_config(CaptureConfig::default().with_database_path(&db_uri));

    for fixture in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "PermissionRequest",
        "PermissionDenied",
        "InstructionsLoaded",
        "SessionEnd",
    ] {
        post_hook(&app, &read_hook_fixture(fixture)?).await?;
    }

    let stats = database.normalize()?;
    let session_id = session_id_from_hook_fixture("SessionEnd")?;

    assert_eq!(stats.processed_events, 8);
    assert_eq!(count_rows(&sql, "SELECT COUNT(*) FROM sessions")?, 2);
    assert_eq!(count_rows(&sql, "SELECT COUNT(*) FROM prompts")?, 1);
    assert_eq!(
        count_rows(&sql, "SELECT COUNT(*) FROM tool_invocations")?,
        2
    );
    assert_eq!(
        count_rows(&sql, "SELECT COUNT(*) FROM permission_decisions")?,
        2
    );
    assert_eq!(
        count_rows(&sql, "SELECT COUNT(*) FROM instruction_loads")?,
        1
    );

    let end_reason: String = sql.query_row(
        "SELECT end_reason FROM sessions WHERE id = ?1",
        [session_id.as_str()],
        |row| row.get(0),
    )?;
    let tool_success: Option<bool> = sql.query_row(
        "SELECT success FROM tool_invocations WHERE id = ?1",
        ["toolu_01Bqr78WkjBpvgdnN3GGhDB1"],
        |row| row.get(0),
    )?;

    assert_eq!(end_reason, "prompt_input_exit");
    assert_eq!(tool_success, Some(true));

    Ok(())
}

#[tokio::test]
async fn fts_search_returns_only_matching_sessions() -> Result<(), Box<dyn Error>> {
    let db_uri = shared_memory_uri("fts");
    let database = Database::new(&db_uri)?;
    let app = hooks_router_with_config(CaptureConfig::default().with_database_path(&db_uri));

    post_hook(
        &app,
        &rewrite_hook_fixture("PreToolUse", "fts-a", Some("toolu_fts_a"), Some("Bash"))?,
    )
    .await?;
    post_hook(
        &app,
        &rewrite_hook_fixture("PreToolUse", "fts-b", Some("toolu_fts_b"), Some("Read"))?,
    )
    .await?;
    post_hook(
        &app,
        &rewrite_hook_fixture("PreToolUse", "fts-c", Some("toolu_fts_c"), Some("Bash"))?,
    )
    .await?;

    let bash_sessions = database
        .search_fts("Bash")?
        .into_iter()
        .filter_map(|event| event.session_id)
        .collect::<BTreeSet<_>>();
    let read_sessions = database
        .search_fts("Read")?
        .into_iter()
        .filter_map(|event| event.session_id)
        .collect::<BTreeSet<_>>();

    assert_eq!(
        bash_sessions,
        BTreeSet::from([String::from("fts-a"), String::from("fts-c")])
    );
    assert_eq!(read_sessions, BTreeSet::from([String::from("fts-b")]));

    Ok(())
}

#[test]
fn backlog_recovery_moves_events_into_the_database() -> Result<(), Box<dyn Error>> {
    let db_uri = shared_memory_uri("backlog");
    let database = Database::new(&db_uri)?;
    let workspace = TempDir::new()?;
    let backlog_path = workspace.path().join("backlog.jsonl");
    let writer = BacklogWriter::new(&backlog_path);
    let processor = BacklogProcessor::new(&backlog_path);

    writer.append(&rewrite_hook_fixture(
        "SessionStart",
        "session-backlog",
        None,
        None,
    )?)?;
    writer.append(&rewrite_hook_fixture(
        "PreToolUse",
        "session-backlog",
        Some("toolu_backlog"),
        Some("Bash"),
    )?)?;

    let processed = processor.process(&database)?;
    let events = database.query_raw_events_by_session("session-backlog")?;

    assert_eq!(processed, 2);
    assert_eq!(events.len(), 2);
    assert_eq!(fs::metadata(&backlog_path)?.len(), 0);

    Ok(())
}

fn shared_memory_uri(test_name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    format!(
        "file:{test_name}-{}-{nanos}?mode=memory&cache=shared",
        std::process::id()
    )
}

fn open_query_connection(uri: &str) -> Result<Connection, rusqlite::Error> {
    Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
}

fn event_types(events: &[RawEvent]) -> Vec<&str> {
    events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect()
}

async fn post_hook(app: &Router, payload: &str) -> Result<(), Box<dyn Error>> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/hooks")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_owned()))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

fn session_id_from_hook_fixture(name: &str) -> Result<String, Box<dyn Error>> {
    let payload: Value = serde_json::from_str(&read_hook_fixture(name)?)?;
    let session_id = payload
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| std::io::Error::other(format!("{name} fixture missing session_id")))?;

    Ok(session_id.to_owned())
}

fn read_hook_fixture(name: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(
        fixture_root().join("hooks").join(format!("{name}.json")),
    )?)
}

fn rewrite_hook_fixture(
    name: &str,
    session_id: &str,
    tool_use_id: Option<&str>,
    tool_name: Option<&str>,
) -> Result<String, Box<dyn Error>> {
    let mut value: Value = serde_json::from_str(&read_hook_fixture(name)?)?;
    let object = value
        .as_object_mut()
        .ok_or("hook fixture should deserialize to an object")?;

    object.insert("session_id".to_owned(), json!(session_id));
    object.insert(
        "transcript_path".to_owned(),
        json!(format!("/workspace/.claude/projects/{session_id}.jsonl")),
    );
    object.insert("cwd".to_owned(), json!("/workspace/claude-insight"));

    if let Some(tool_use_id) = tool_use_id {
        replace_string_fields(&mut value, &["tool_use_id"], tool_use_id);
    }

    if let Some(tool_name) = tool_name {
        replace_string_fields(&mut value, &["tool_name"], tool_name);
    }

    Ok(serde_json::to_string(&value)?)
}

fn build_transcript_fixture_stream(
    session_id: &str,
    count: usize,
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut base = rewrite_transcript_fixture("comprehensive.jsonl", session_id, None)?;
    base.extend(rewrite_transcript_fixture(
        "auth-failure.observed.jsonl",
        session_id,
        None,
    )?);

    Ok(base
        .iter()
        .cycle()
        .take(count)
        .enumerate()
        .map(|(index, line)| rewrite_transcript_line(line, session_id, None, index))
        .collect::<Result<Vec<_>, _>>()?)
}

fn rewrite_transcript_fixture(
    file_name: &str,
    session_id: &str,
    tool_use_id: Option<&str>,
) -> Result<Vec<String>, Box<dyn Error>> {
    transcript_fixture_lines(file_name)?
        .into_iter()
        .enumerate()
        .map(|(index, line)| rewrite_transcript_line(&line, session_id, tool_use_id, index))
        .collect::<Result<Vec<_>, _>>()
}

fn tool_transcript_lines(
    session_id: &str,
    tool_use_id: &str,
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut selected = Vec::new();

    for line in transcript_fixture_lines("comprehensive.jsonl")? {
        let value: Value = serde_json::from_str(&line)?;
        let entry_type = value.get("type").and_then(Value::as_str);
        if line.contains(FIXTURE_TOOL_USE_ID) && matches!(entry_type, Some("assistant" | "system"))
        {
            selected.push(line);
        }
    }

    selected
        .into_iter()
        .enumerate()
        .map(|(index, line)| rewrite_transcript_line(&line, session_id, Some(tool_use_id), index))
        .collect::<Result<Vec<_>, _>>()
}

fn transcript_fixture_lines(file_name: &str) -> Result<Vec<String>, Box<dyn Error>> {
    Ok(
        fs::read_to_string(fixture_root().join("transcripts").join(file_name))?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(ToOwned::to_owned)
            .collect(),
    )
}

fn rewrite_transcript_line(
    line: &str,
    session_id: &str,
    tool_use_id: Option<&str>,
    index: usize,
) -> Result<String, Box<dyn Error>> {
    let mut value: Value = serde_json::from_str(line)?;
    let object = value
        .as_object_mut()
        .ok_or("transcript fixture should deserialize to an object")?;

    object.insert("sessionId".to_owned(), json!(session_id));
    object.insert(
        "timestamp".to_owned(),
        json!(format!(
            "2026-04-03T15:{:02}:{:02}.000Z",
            (index / 60) % 60,
            index % 60
        )),
    );

    if let Some(tool_use_id) = tool_use_id {
        replace_string_fields(
            &mut value,
            &[
                "id",
                "toolUseID",
                "toolUseId",
                "parentToolUseID",
                "tool_use_id",
            ],
            tool_use_id,
        );
    }

    Ok(serde_json::to_string(&value)?)
}

fn replace_string_fields(value: &mut Value, keys: &[&str], replacement: &str) {
    match value {
        Value::Object(object) => {
            for (key, child) in object.iter_mut() {
                if keys.contains(&key.as_str()) {
                    *child = Value::String(replacement.to_owned());
                } else {
                    replace_string_fields(child, keys, replacement);
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                replace_string_fields(child, keys, replacement);
            }
        }
        _ => {}
    }
}

fn write_transcript(path: &Path, lines: &[String]) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents = format!("{}\n", lines.join("\n"));
    fs::write(path, contents)?;

    Ok(())
}

fn raw_event_id(
    connection: &Connection,
    session_id: &str,
    source: &str,
    event_type: &str,
    tool_use_id: Option<&str>,
) -> Result<i64, Box<dyn Error>> {
    Ok(connection.query_row(
        "
        SELECT id
        FROM raw_events
        WHERE session_id = ?1
          AND source = ?2
          AND event_type = ?3
          AND (?4 IS NULL OR tool_use_id = ?4)
        ORDER BY id ASC
        LIMIT 1
        ",
        params![session_id, source, event_type, tool_use_id],
        |row| row.get(0),
    )?)
}

fn assert_link(
    connection: &Connection,
    left: i64,
    right: i64,
    link_type: &str,
) -> Result<(), Box<dyn Error>> {
    let (source_event_id, target_event_id) = canonical_pair(left, right);
    let count: i64 = connection.query_row(
        "
        SELECT COUNT(*)
        FROM event_links
        WHERE source_event_id = ?1
          AND target_event_id = ?2
          AND link_type = ?3
        ",
        params![source_event_id, target_event_id, link_type],
        |row| row.get(0),
    )?;

    assert_eq!(count, 1);
    Ok(())
}

fn canonical_pair(left: i64, right: i64) -> (i64, i64) {
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

fn count_rows(connection: &Connection, sql: &str) -> Result<i64, Box<dyn Error>> {
    Ok(connection.query_row(sql, [], |row| row.get(0))?)
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}
