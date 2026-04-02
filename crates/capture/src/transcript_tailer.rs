use claude_insight_storage::{Database, NewRawEvent};
use claude_insight_types::{
    ContentBlock, TranscriptEntry, TranscriptMessage, TranscriptMessageKind,
};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

const DEFAULT_TRANSCRIPT_DIR: &str = ".claude/projects";
const DEFAULT_STATE_DIR: &str = ".claude-insight";
const DEFAULT_STATE_FILE: &str = "transcript_offsets.json";
const TRANSCRIPT_SOURCE: &str = "transcript";
const UNKNOWN_TIMESTAMP: &str = "1970-01-01T00:00:00Z";

#[derive(Debug)]
pub enum TranscriptTailerError {
    HomeDirectoryUnavailable,
    Io(std::io::Error),
    Notify(notify::Error),
    Storage(rusqlite::Error),
    StateSerde(serde_json::Error),
}

impl fmt::Display for TranscriptTailerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HomeDirectoryUnavailable => {
                write!(f, "HOME is not set; cannot resolve transcript tailer paths")
            }
            Self::Io(error) => write!(f, "{error}"),
            Self::Notify(error) => write!(f, "{error}"),
            Self::Storage(error) => write!(f, "{error}"),
            Self::StateSerde(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for TranscriptTailerError {}

impl From<std::io::Error> for TranscriptTailerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<notify::Error> for TranscriptTailerError {
    fn from(value: notify::Error) -> Self {
        Self::Notify(value)
    }
}

impl From<rusqlite::Error> for TranscriptTailerError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Storage(value)
    }
}

impl From<serde_json::Error> for TranscriptTailerError {
    fn from(value: serde_json::Error) -> Self {
        Self::StateSerde(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptTailerConfig {
    pub transcript_root: PathBuf,
    pub positions_path: PathBuf,
    pub database_path: PathBuf,
}

impl TranscriptTailerConfig {
    pub fn from_home() -> Result<Self, TranscriptTailerError> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or(TranscriptTailerError::HomeDirectoryUnavailable)?;

        Ok(Self {
            transcript_root: home.join(DEFAULT_TRANSCRIPT_DIR),
            positions_path: home.join(DEFAULT_STATE_DIR).join(DEFAULT_STATE_FILE),
            database_path: Database::default_path()?,
        })
    }
}

impl Default for TranscriptTailerConfig {
    fn default() -> Self {
        Self::from_home().unwrap_or_else(|_| Self {
            transcript_root: PathBuf::from(DEFAULT_TRANSCRIPT_DIR),
            positions_path: PathBuf::from(DEFAULT_STATE_DIR).join(DEFAULT_STATE_FILE),
            database_path: PathBuf::from(DEFAULT_STATE_DIR).join("insight.db"),
        })
    }
}

pub struct TranscriptTailer {
    config: TranscriptTailerConfig,
    database: Database,
    positions: BTreeMap<PathBuf, u64>,
    last_timestamps: BTreeMap<PathBuf, String>,
    _watcher: RecommendedWatcher,
    events_rx: Receiver<notify::Result<Event>>,
}

impl TranscriptTailer {
    pub fn new(config: TranscriptTailerConfig) -> Result<Self, TranscriptTailerError> {
        std::fs::create_dir_all(&config.transcript_root)?;

        let (events_tx, events_rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = events_tx.send(event);
        })?;
        watcher.watch(&config.transcript_root, RecursiveMode::Recursive)?;

        let database = Database::new(&config.database_path)?;
        let positions = load_positions(&config.positions_path)?;

        let mut tailer = Self {
            config,
            database,
            positions,
            last_timestamps: BTreeMap::new(),
            _watcher: watcher,
            events_rx,
        };

        tailer.initialize_existing_files()?;

        Ok(tailer)
    }

    pub fn process_pending(&mut self) -> Result<usize, TranscriptTailerError> {
        let mut processed = 0;

        loop {
            match self.events_rx.try_recv() {
                Ok(event) => {
                    processed += self.handle_notify_event(event?)?;
                }
                Err(TryRecvError::Empty) => return Ok(processed),
                Err(TryRecvError::Disconnected) => return Ok(processed),
            }
        }
    }

    pub fn wait_for_events(&mut self, timeout: Duration) -> Result<usize, TranscriptTailerError> {
        let mut processed = 0;

        match self.events_rx.recv_timeout(timeout) {
            Ok(event) => {
                processed += self.handle_notify_event(event?)?;
            }
            Err(RecvTimeoutError::Timeout) => return Ok(0),
            Err(RecvTimeoutError::Disconnected) => return Ok(0),
        }

        processed += self.process_pending()?;

        Ok(processed)
    }

    pub fn ingest_path(&mut self, path: impl AsRef<Path>) -> Result<usize, TranscriptTailerError> {
        self.ingest_file(path.as_ref(), true)
    }

    pub fn tracked_offset(&self, path: impl AsRef<Path>) -> Option<u64> {
        self.positions.get(path.as_ref()).copied()
    }

    fn initialize_existing_files(&mut self) -> Result<(), TranscriptTailerError> {
        for path in discover_transcript_files(&self.config.transcript_root)? {
            let existing_len = file_len(&path)?;

            match self.positions.get_mut(&path) {
                Some(offset) if *offset > existing_len => {
                    tracing::warn!(
                        path = %path.display(),
                        old_offset = *offset,
                        new_len = existing_len,
                        "persisted offset exceeded transcript length; resetting to start"
                    );
                    *offset = 0;
                }
                Some(_) => {}
                None => {
                    self.positions.insert(path, existing_len);
                }
            }
        }

        self.persist_positions()
    }

    fn handle_notify_event(&mut self, event: Event) -> Result<usize, TranscriptTailerError> {
        let paths = event
            .paths
            .into_iter()
            .filter(|path| is_transcript_jsonl(path))
            .collect::<BTreeSet<_>>();

        let mut processed = 0;

        for path in paths {
            processed += self.ingest_file(&path, true)?;
        }

        Ok(processed)
    }

    fn ingest_file(
        &mut self,
        path: &Path,
        runtime_discovery: bool,
    ) -> Result<usize, TranscriptTailerError> {
        if !is_transcript_jsonl(path) || !path.is_file() {
            return Ok(0);
        }

        if runtime_discovery {
            self.positions.entry(path.to_path_buf()).or_insert(0);
        }

        let mut offset = self.positions.get(path).copied().unwrap_or_default();
        let len = file_len(path)?;
        if offset > len {
            tracing::warn!(
                path = %path.display(),
                old_offset = offset,
                new_len = len,
                "transcript file was truncated; restarting from the beginning"
            );
            offset = 0;
        }

        let fallbacks = PathFallback::from_path(&self.config.transcript_root, path);

        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(offset))?;

        let mut ingested = 0;
        let mut next_offset = offset;

        loop {
            let line_start = reader.stream_position()?;
            let mut line = String::new();
            let bytes = reader.read_line(&mut line)?;

            if bytes == 0 {
                break;
            }

            if !line.ends_with('\n') {
                next_offset = line_start;
                break;
            }

            next_offset = line_start + bytes as u64;
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                continue;
            }

            if self.ingest_line(path, &fallbacks, trimmed)? {
                ingested += 1;
            }
        }

        self.positions.insert(path.to_path_buf(), next_offset);
        self.persist_positions()?;

        Ok(ingested)
    }

    fn ingest_line(
        &mut self,
        path: &Path,
        fallbacks: &PathFallback,
        line: &str,
    ) -> Result<bool, TranscriptTailerError> {
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %error,
                    "skipping malformed transcript line"
                );
                return Ok(false);
            }
        };

        let entry = match serde_json::from_value::<TranscriptEntry>(value.clone()) {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %error,
                    "skipping malformed transcript entry"
                );
                return Ok(false);
            }
        };

        let metadata = ExtractedEvent::new(
            &entry,
            &value,
            fallbacks,
            self.last_timestamps.get(path).map(String::as_str),
        );

        let record = NewRawEvent {
            session_id: metadata.session_id.as_deref(),
            source: TRANSCRIPT_SOURCE,
            event_type: metadata.event_type,
            ts: &metadata.timestamp,
            tool_use_id: metadata.tool_use_id.as_deref(),
            prompt_id: metadata.prompt_id.as_deref(),
            agent_id: metadata.agent_id.as_deref(),
            payload_json: line,
            claude_version: metadata.claude_version.as_deref(),
            adapter_version: None,
        };

        self.database.insert_raw_event_record(&record)?;

        if metadata.had_explicit_timestamp {
            self.last_timestamps
                .insert(path.to_path_buf(), metadata.timestamp);
        }

        Ok(true)
    }

    fn persist_positions(&self) -> Result<(), TranscriptTailerError> {
        if let Some(parent) = self.config.positions_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let persisted = PersistedPositions::from_paths(&self.positions);
        let serialized = serde_json::to_vec_pretty(&persisted)?;
        let temp_path = self.config.positions_path.with_extension("json.tmp");

        std::fs::write(&temp_path, serialized)?;
        std::fs::rename(temp_path, &self.config.positions_path)?;

        Ok(())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedPositions {
    positions: BTreeMap<String, u64>,
}

impl PersistedPositions {
    fn from_paths(paths: &BTreeMap<PathBuf, u64>) -> Self {
        let positions = paths
            .iter()
            .map(|(path, offset)| (path.to_string_lossy().into_owned(), *offset))
            .collect();

        Self { positions }
    }

    fn into_paths(self) -> BTreeMap<PathBuf, u64> {
        self.positions
            .into_iter()
            .map(|(path, offset)| (PathBuf::from(path), offset))
            .collect()
    }
}

#[derive(Debug, Default, Clone)]
struct PathFallback {
    session_id: Option<String>,
    agent_id: Option<String>,
}

impl PathFallback {
    fn from_path(root: &Path, path: &Path) -> Self {
        let relative = path.strip_prefix(root).unwrap_or(path);
        let stem = path.file_stem().and_then(|value| value.to_str());

        let is_subagent = relative
            .iter()
            .any(|component| component == std::ffi::OsStr::new("subagents"));

        if is_subagent {
            let session_id = path
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|value| value.to_str())
                .map(ToOwned::to_owned);
            let agent_id = stem
                .and_then(|value| value.strip_prefix("agent-"))
                .map(ToOwned::to_owned);

            return Self {
                session_id,
                agent_id,
            };
        }

        Self {
            session_id: stem.map(ToOwned::to_owned),
            agent_id: None,
        }
    }
}

struct ExtractedEvent {
    session_id: Option<String>,
    event_type: &'static str,
    timestamp: String,
    tool_use_id: Option<String>,
    prompt_id: Option<String>,
    agent_id: Option<String>,
    claude_version: Option<String>,
    had_explicit_timestamp: bool,
}

impl ExtractedEvent {
    fn new(
        entry: &TranscriptEntry,
        value: &Value,
        fallbacks: &PathFallback,
        previous_timestamp: Option<&str>,
    ) -> Self {
        let explicit_timestamp = value_string(value, "timestamp");
        let timestamp = explicit_timestamp
            .clone()
            .or_else(|| previous_timestamp.map(ToOwned::to_owned))
            .unwrap_or_else(|| UNKNOWN_TIMESTAMP.to_owned());

        Self {
            session_id: value_string(value, "sessionId").or_else(|| fallbacks.session_id.clone()),
            event_type: transcript_event_type(entry),
            timestamp,
            tool_use_id: extract_tool_use_id(value, entry),
            prompt_id: value_string(value, "promptId"),
            agent_id: value_string(value, "agentId").or_else(|| fallbacks.agent_id.clone()),
            claude_version: value_string(value, "version"),
            had_explicit_timestamp: explicit_timestamp.is_some(),
        }
    }
}

fn load_positions(path: &Path) -> Result<BTreeMap<PathBuf, u64>, TranscriptTailerError> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let contents = std::fs::read_to_string(path)?;
    match serde_json::from_str::<PersistedPositions>(&contents) {
        Ok(state) => Ok(state.into_paths()),
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                error = %error,
                "failed to parse transcript offset state; starting with an empty state"
            );
            Ok(BTreeMap::new())
        }
    }
}

fn discover_transcript_files(root: &Path) -> Result<Vec<PathBuf>, TranscriptTailerError> {
    let mut discovered = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();

            if entry.file_type()?.is_dir() {
                stack.push(path);
            } else if is_transcript_jsonl(&path) {
                discovered.push(path);
            }
        }
    }

    Ok(discovered)
}

fn extract_tool_use_id(value: &Value, entry: &TranscriptEntry) -> Option<String> {
    value_string(value, "toolUseID")
        .or_else(|| value_string(value, "toolUseId"))
        .or_else(|| value_string(value, "parentToolUseID"))
        .or_else(|| message_tool_use_id(entry))
        .or_else(|| replacement_tool_use_id(value))
}

fn message_tool_use_id(entry: &TranscriptEntry) -> Option<String> {
    match entry {
        TranscriptEntry::Message(message) => first_message_tool_use_id(message),
        TranscriptEntry::Progress(message) => message.tool_use_id.clone(),
        _ => None,
    }
}

fn first_message_tool_use_id(message: &TranscriptMessage) -> Option<String> {
    match &message.message.content {
        claude_insight_types::TranscriptContent::Text(_) => None,
        claude_insight_types::TranscriptContent::Blocks(blocks) => {
            blocks.iter().find_map(|block| match block {
                ContentBlock::ToolUse(block) => Some(block.id.clone()),
                ContentBlock::ToolResult(block) => Some(block.tool_use_id.clone()),
                _ => None,
            })
        }
    }
}

fn replacement_tool_use_id(value: &Value) -> Option<String> {
    value
        .get("replacements")
        .and_then(Value::as_array)
        .and_then(|replacements| {
            replacements.iter().find_map(|replacement| {
                replacement
                    .get("toolUseId")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
        })
}

fn transcript_event_type(entry: &TranscriptEntry) -> &'static str {
    match entry {
        TranscriptEntry::Message(message) => match message.kind {
            TranscriptMessageKind::User => "TranscriptUserMessage",
            TranscriptMessageKind::Assistant => "TranscriptAssistantMessage",
            TranscriptMessageKind::Attachment => "TranscriptAttachmentMessage",
            TranscriptMessageKind::System => "TranscriptSystemMessage",
        },
        TranscriptEntry::Progress(_) => "TranscriptProgress",
        TranscriptEntry::Summary(_) => "TranscriptSummary",
        TranscriptEntry::CustomTitle(_) => "TranscriptCustomTitle",
        TranscriptEntry::AiTitle(_) => "TranscriptAiTitle",
        TranscriptEntry::LastPrompt(_) => "TranscriptLastPrompt",
        TranscriptEntry::TaskSummary(_) => "TranscriptTaskSummary",
        TranscriptEntry::Tag(_) => "TranscriptTag",
        TranscriptEntry::AgentName(_) => "TranscriptAgentName",
        TranscriptEntry::AgentColor(_) => "TranscriptAgentColor",
        TranscriptEntry::AgentSetting(_) => "TranscriptAgentSetting",
        TranscriptEntry::PRLink(_) => "TranscriptPRLink",
        TranscriptEntry::FileHistorySnapshot(_) => "TranscriptFileHistorySnapshot",
        TranscriptEntry::AttributionSnapshot(_) => "TranscriptAttributionSnapshot",
        TranscriptEntry::QueueOperation(_) => "TranscriptQueueOperation",
        TranscriptEntry::SpeculationAccept(_) => "TranscriptSpeculationAccept",
        TranscriptEntry::Mode(_) => "TranscriptMode",
        TranscriptEntry::WorktreeState(_) => "TranscriptWorktreeState",
        TranscriptEntry::ContentReplacement(_) => "TranscriptContentReplacement",
        TranscriptEntry::ContextCollapseCommit(_) => "TranscriptContextCollapseCommit",
        TranscriptEntry::ContextCollapseSnapshot(_) => "TranscriptContextCollapseSnapshot",
        TranscriptEntry::Unknown(_) => "TranscriptUnknown",
    }
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn is_transcript_jsonl(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("jsonl"))
}

fn file_len(path: &Path) -> Result<u64, TranscriptTailerError> {
    Ok(std::fs::metadata(path)?.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_insight_storage::RawEventQuery;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn existing_transcript_files_are_baselined_without_backfill(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = TestWorkspace::new()?;
        let transcript_path = workspace.transcript_path("session-existing.jsonl");
        write_transcript_line(
            &transcript_path,
            &message_line("session-existing", "2026-04-03T15:00:00Z", None, None),
        )?;

        let tailer = TranscriptTailer::new(workspace.config())?;
        let database = Database::new(&workspace.database_path)?;

        let events = database.query_raw_events(RawEventQuery::default())?;
        assert!(events.is_empty());
        assert_eq!(
            tailer.tracked_offset(&transcript_path),
            Some(std::fs::metadata(&transcript_path)?.len())
        );

        Ok(())
    }

    #[test]
    fn ingest_path_skips_malformed_lines_and_persists_offsets(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = TestWorkspace::new()?;
        let mut tailer = TranscriptTailer::new(workspace.config())?;
        let transcript_path = workspace.transcript_path("session-live.jsonl");

        append_lines(
            &transcript_path,
            &[
                "{\"type\":\"user\",\"sessionId\":\"broken\"\n".to_string(),
                format!(
                    "{}\n",
                    message_line(
                        "session-live",
                        "2026-04-03T15:00:00Z",
                        Some("prompt-1"),
                        None
                    )
                ),
            ],
        )?;

        let ingested = tailer.ingest_path(&transcript_path)?;
        let database = Database::new(&workspace.database_path)?;
        let events = database.query_raw_events_by_session("session-live")?;

        assert_eq!(ingested, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "TranscriptUserMessage");
        assert_eq!(
            tailer.tracked_offset(&transcript_path),
            Some(std::fs::metadata(&transcript_path)?.len())
        );
        assert!(workspace.positions_path.exists());

        Ok(())
    }

    #[test]
    fn resume_after_restart_only_ingests_new_lines() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = TestWorkspace::new()?;
        let transcript_path = workspace.transcript_path("session-resume.jsonl");

        {
            let mut tailer = TranscriptTailer::new(workspace.config())?;
            append_lines(
                &transcript_path,
                &[format!(
                    "{}\n",
                    message_line(
                        "session-resume",
                        "2026-04-03T15:00:00Z",
                        Some("prompt-1"),
                        None
                    )
                )],
            )?;

            assert_eq!(tailer.ingest_path(&transcript_path)?, 1);
        }

        {
            let mut tailer = TranscriptTailer::new(workspace.config())?;
            append_lines(
                &transcript_path,
                &[format!(
                    "{}\n",
                    message_line(
                        "session-resume",
                        "2026-04-03T15:00:01Z",
                        Some("prompt-2"),
                        None
                    )
                )],
            )?;

            assert_eq!(tailer.ingest_path(&transcript_path)?, 1);
        }

        let database = Database::new(&workspace.database_path)?;
        let events = database.query_raw_events_by_session("session-resume")?;

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].prompt_id.as_deref(), Some("prompt-1"));
        assert_eq!(events[1].prompt_id.as_deref(), Some("prompt-2"));

        Ok(())
    }

    #[test]
    fn subagent_transcript_files_are_tailed_with_path_fallbacks(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = TestWorkspace::new()?;
        let mut tailer = TranscriptTailer::new(workspace.config())?;
        let transcript_path =
            workspace.subagent_transcript_path("project-dir", "session-parent", "agent-42");

        append_lines(
            &transcript_path,
            &[format!(
                "{}\n",
                message_line("session-parent", "2026-04-03T15:00:00Z", None, None)
            )],
        )?;

        let ingested = tailer.ingest_path(&transcript_path)?;
        let database = Database::new(&workspace.database_path)?;
        let events = database.query_raw_events_by_session("session-parent")?;

        assert_eq!(ingested, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].agent_id.as_deref(), Some("42"));

        Ok(())
    }

    #[test]
    fn watcher_ingests_new_transcript_lines_within_timeout(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = TestWorkspace::new()?;
        let mut tailer = TranscriptTailer::new(workspace.config())?;
        let transcript_path = workspace.transcript_path("session-watch.jsonl");

        append_lines(
            &transcript_path,
            &[format!(
                "{}\n",
                message_line(
                    "session-watch",
                    "2026-04-03T15:00:00Z",
                    Some("prompt-watch"),
                    None
                )
            )],
        )?;

        let mut ingested = 0;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline && ingested == 0 {
            ingested += tailer.wait_for_events(Duration::from_millis(250))?;
        }

        let database = Database::new(&workspace.database_path)?;
        let events = database.query_raw_events_by_session("session-watch")?;

        assert_eq!(ingested, 1);
        assert_eq!(events.len(), 1);

        Ok(())
    }

    struct TestWorkspace {
        root: PathBuf,
        transcript_root: PathBuf,
        database_path: PathBuf,
        positions_path: PathBuf,
    }

    impl TestWorkspace {
        fn new() -> Result<Self, Box<dyn std::error::Error>> {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            let root = std::env::temp_dir().join(format!(
                "claude-insight-transcript-tailer-{}-{unique}",
                std::process::id()
            ));
            let transcript_root = root.join(".claude/projects");
            let database_path = root.join(".claude-insight/insight.db");
            let positions_path = root.join(".claude-insight/transcript_offsets.json");

            std::fs::create_dir_all(&transcript_root)?;

            Ok(Self {
                root,
                transcript_root,
                database_path,
                positions_path,
            })
        }

        fn config(&self) -> TranscriptTailerConfig {
            TranscriptTailerConfig {
                transcript_root: self.transcript_root.clone(),
                positions_path: self.positions_path.clone(),
                database_path: self.database_path.clone(),
            }
        }

        fn transcript_path(&self, file_name: &str) -> PathBuf {
            self.transcript_root
                .join("sanitized-project")
                .join(file_name)
        }

        fn subagent_transcript_path(
            &self,
            sanitized_project: &str,
            session_id: &str,
            agent_stem: &str,
        ) -> PathBuf {
            self.transcript_root
                .join(sanitized_project)
                .join(session_id)
                .join("subagents")
                .join(format!("{agent_stem}.jsonl"))
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn append_lines(path: &Path, lines: &[String]) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        use std::io::Write;

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;

        for line in lines {
            file.write_all(line.as_bytes())?;
        }

        file.flush()?;

        Ok(())
    }

    fn write_transcript_line(path: &Path, line: &str) -> Result<(), Box<dyn std::error::Error>> {
        append_lines(path, &[format!("{line}\n")])
    }

    fn message_line(
        session_id: &str,
        timestamp: &str,
        prompt_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> String {
        serde_json::json!({
            "type": "user",
            "uuid": format!("{session_id}-uuid"),
            "parentUuid": null,
            "logicalParentUuid": null,
            "isSidechain": false,
            "gitBranch": "eddingsuree/mot-121-feat-transcript-jsonl-file-tailer",
            "agentId": agent_id,
            "promptId": prompt_id,
            "cwd": "/workspace/claude-insight",
            "userType": "external",
            "entrypoint": "sdk-cli",
            "sessionId": session_id,
            "timestamp": timestamp,
            "version": "2.1.81",
            "message": {
                "role": "user",
                "content": "hello from transcript tailer"
            }
        })
        .to_string()
    }
}
