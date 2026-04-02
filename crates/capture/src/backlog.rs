use std::{
    fmt,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use claude_insight_storage::{Database, NewRawEvent};
use fs2::FileExt;
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const BACKLOG_DIR: &str = ".claude-insight";
const BACKLOG_FILE: &str = "backlog.jsonl";
static BACKLOG_MUTEX: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogWriter {
    path: PathBuf,
}

impl BacklogWriter {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn from_default_path() -> Result<Self, BacklogError> {
        Ok(Self::new(default_backlog_path()?))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, event_json: &str) -> Result<(), BacklogError> {
        let _guard = backlog_guard()?;
        let mut file = open_locked_backlog_file(&self.path)?;
        let json_line = normalize_json_line(event_json)?;

        file.seek(SeekFrom::End(0))?;
        file.write_all(json_line.as_bytes())?;
        file.sync_data()?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogProcessor {
    path: PathBuf,
}

impl BacklogProcessor {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn from_default_path() -> Result<Self, BacklogError> {
        Ok(Self::new(default_backlog_path()?))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn process(&self, db: &Database) -> Result<usize, BacklogError> {
        let _guard = backlog_guard()?;
        let mut file = open_locked_backlog_file(&self.path)?;
        let mut contents = String::new();

        file.seek(SeekFrom::Start(0))?;
        file.read_to_string(&mut contents)?;

        let records = contents
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(parse_backlog_record(trimmed, index + 1))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        for record in &records {
            db.insert_raw_event_record(&record.as_new_raw_event())?;
        }

        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.sync_all()?;

        Ok(records.len())
    }
}

pub fn append_to_backlog(event_json: &str) -> Result<(), BacklogError> {
    BacklogWriter::from_default_path()?.append(event_json)
}

pub fn process_backlog(db: &Database) -> Result<usize, BacklogError> {
    BacklogProcessor::from_default_path()?.process(db)
}

#[derive(Debug)]
pub enum BacklogError {
    MissingHomeDirectory,
    EmptyEvent,
    InvalidField {
        line_number: usize,
        field: &'static str,
    },
    InvalidObject {
        line_number: usize,
    },
    LockPoisoned,
    Io(std::io::Error),
    Json(serde_json::Error),
    Database(rusqlite::Error),
    Timestamp(time::error::Format),
}

impl fmt::Display for BacklogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHomeDirectory => f.write_str("HOME is not set for backlog path"),
            Self::EmptyEvent => f.write_str("backlog event payload was empty"),
            Self::InvalidField { line_number, field } => {
                write!(f, "backlog line {line_number} is missing field `{field}`")
            }
            Self::InvalidObject { line_number } => {
                write!(f, "backlog line {line_number} is not a JSON object")
            }
            Self::LockPoisoned => f.write_str("backlog mutex was poisoned"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
            Self::Database(err) => write!(f, "{err}"),
            Self::Timestamp(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for BacklogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Json(err) => Some(err),
            Self::Database(err) => Some(err),
            Self::Timestamp(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for BacklogError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for BacklogError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<rusqlite::Error> for BacklogError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

impl From<time::error::Format> for BacklogError {
    fn from(value: time::error::Format) -> Self {
        Self::Timestamp(value)
    }
}

#[derive(Debug)]
struct ParsedBacklogRecord {
    session_id: String,
    event_type: String,
    ts: String,
    tool_use_id: Option<String>,
    prompt_id: Option<String>,
    agent_id: Option<String>,
    payload_json: String,
    claude_version: Option<String>,
    adapter_version: Option<String>,
}

impl ParsedBacklogRecord {
    fn as_new_raw_event(&self) -> NewRawEvent<'_> {
        NewRawEvent {
            session_id: Some(&self.session_id),
            source: "hook",
            event_type: &self.event_type,
            ts: &self.ts,
            tool_use_id: self.tool_use_id.as_deref(),
            prompt_id: self.prompt_id.as_deref(),
            agent_id: self.agent_id.as_deref(),
            payload_json: &self.payload_json,
            claude_version: self.claude_version.as_deref(),
            adapter_version: self.adapter_version.as_deref(),
        }
    }
}

fn default_backlog_path() -> Result<PathBuf, BacklogError> {
    let home = std::env::var_os("HOME").ok_or(BacklogError::MissingHomeDirectory)?;

    Ok(PathBuf::from(home).join(BACKLOG_DIR).join(BACKLOG_FILE))
}

fn backlog_guard() -> Result<MutexGuard<'static, ()>, BacklogError> {
    BACKLOG_MUTEX.lock().map_err(|_| BacklogError::LockPoisoned)
}

fn open_locked_backlog_file(path: &Path) -> Result<File, BacklogError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;

    file.lock_exclusive()?;

    Ok(file)
}

fn normalize_json_line(event_json: &str) -> Result<String, BacklogError> {
    let trimmed = event_json.trim_end_matches(['\r', '\n']);

    if trimmed.is_empty() {
        return Err(BacklogError::EmptyEvent);
    }

    Ok(format!("{trimmed}\n"))
}

fn parse_backlog_record(
    line: &str,
    line_number: usize,
) -> Result<ParsedBacklogRecord, BacklogError> {
    let payload: Value = serde_json::from_str(line)?;
    let object = payload
        .as_object()
        .ok_or(BacklogError::InvalidObject { line_number })?;

    Ok(ParsedBacklogRecord {
        session_id: required_string(object, "session_id", line_number)?,
        event_type: required_string(object, "hook_event_name", line_number)?,
        ts: optional_string(object, "ts")
            .or_else(|| optional_string(object, "timestamp"))
            .unwrap_or_else(now_rfc3339),
        tool_use_id: optional_string(object, "tool_use_id"),
        prompt_id: optional_string(object, "prompt_id"),
        agent_id: optional_string(object, "agent_id"),
        payload_json: payload.to_string(),
        claude_version: optional_string(object, "claude_version"),
        adapter_version: optional_string(object, "adapter_version"),
    })
}

fn required_string(
    object: &serde_json::Map<String, Value>,
    field: &'static str,
    line_number: usize,
) -> Result<String, BacklogError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or(BacklogError::InvalidField { line_number, field })
}

fn optional_string(object: &serde_json::Map<String, Value>, field: &'static str) -> Option<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{collections::BTreeSet, error::Error, sync::Arc, thread};

    fn temp_backlog_root(test_name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();

        std::env::temp_dir().join(format!(
            "claude-insight-backlog-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn backlog_concurrent_appends_are_not_interleaved() -> Result<(), Box<dyn Error>> {
        let temp_root = temp_backlog_root("concurrent-appends");
        std::fs::create_dir_all(&temp_root)?;
        let backlog_path = temp_root.join("backlog.jsonl");
        let writer = Arc::new(BacklogWriter::new(&backlog_path));
        let handles: Vec<_> = (0..10)
            .map(|index| {
                let writer = Arc::clone(&writer);

                thread::spawn(move || writer.append(&sample_backlog_event(index)))
            })
            .collect();

        for handle in handles {
            let append_result = handle
                .join()
                .map_err(|_| std::io::Error::other("append thread panicked"))?;
            append_result?;
        }

        let contents = std::fs::read_to_string(&backlog_path)?;
        let mut sequences = BTreeSet::new();
        let lines: Vec<_> = contents.lines().collect();

        assert_eq!(lines.len(), 10);

        for line in lines {
            let payload: Value = serde_json::from_str(line)?;
            let sequence = payload
                .get("sequence")
                .and_then(Value::as_u64)
                .ok_or_else(|| std::io::Error::other("missing sequence field"))?;

            sequences.insert(sequence);
        }

        assert_eq!(sequences.len(), 10);
        std::fs::remove_dir_all(&temp_root)?;

        Ok(())
    }

    #[test]
    fn backlog_append_rejects_empty_payload() {
        let temp_root = temp_backlog_root("empty-payload");
        let backlog_path = temp_root.join("backlog.jsonl");
        let writer = BacklogWriter::new(&backlog_path);

        let error = match writer.append("\n") {
            Ok(()) => panic!("empty payload should be rejected"),
            Err(error) => error,
        };

        assert!(matches!(error, BacklogError::EmptyEvent));
    }

    #[test]
    fn backlog_process_rejects_malformed_records() -> Result<(), Box<dyn Error>> {
        let temp_root = temp_backlog_root("malformed-record");
        let backlog_path = temp_root.join(".claude-insight").join("backlog.jsonl");
        if let Some(parent) = backlog_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&backlog_path, "{\"hook_event_name\":\"Notification\"}\n")?;

        let db = Database::new(":memory:")?;
        let processor = BacklogProcessor::new(&backlog_path);
        let error = match processor.process(&db) {
            Ok(_) => panic!("malformed backlog line should fail"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            BacklogError::InvalidField {
                line_number: 1,
                field: "session_id"
            }
        ));
        std::fs::remove_dir_all(&temp_root)?;

        Ok(())
    }

    #[test]
    fn backlog_process_moves_events_into_database_and_clears_file() -> Result<(), Box<dyn Error>> {
        let temp_root = temp_backlog_root("process");
        let backlog_path = temp_root.join(".claude-insight").join("backlog.jsonl");
        let writer = BacklogWriter::new(&backlog_path);

        for index in 0..10 {
            writer.append(&sample_backlog_event(index))?;
        }

        let db = Database::new(":memory:")?;
        let processor = BacklogProcessor::new(&backlog_path);
        let processed = processor.process(&db)?;
        let events = db.query_raw_events_by_session("session-backlog")?;

        assert_eq!(processed, 10);
        assert!(backlog_path.exists());
        assert_eq!(std::fs::metadata(&backlog_path)?.len(), 0);
        assert_eq!(events.len(), 10);
        assert!(events.iter().all(|event| event.source == "hook"));
        assert!(events
            .iter()
            .all(|event| event.event_type == "Notification"));
        std::fs::remove_dir_all(&temp_root)?;

        Ok(())
    }

    fn sample_backlog_event(index: usize) -> String {
        json!({
            "cwd": "/workspace/claude-insight",
            "hook_event_name": "Notification",
            "message": format!("event-{index}"),
            "notification_type": "info",
            "permission_mode": "acceptEdits",
            "sequence": index,
            "session_id": "session-backlog",
            "title": "Claude Code notification",
            "transcript_path": format!("/workspace/.claude/projects/claude-insight/{index}.jsonl"),
        })
        .to_string()
    }
}
