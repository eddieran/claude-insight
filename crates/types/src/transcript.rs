use std::collections::BTreeMap;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptEntry {
    Message(Box<TranscriptMessage>),
    Progress(Box<ProgressMessage>),
    Summary(Box<SummaryMessage>),
    CustomTitle(Box<CustomTitleMessage>),
    AiTitle(Box<AiTitleMessage>),
    LastPrompt(Box<LastPromptMessage>),
    TaskSummary(Box<TaskSummaryMessage>),
    Tag(Box<TagMessage>),
    AgentName(Box<AgentNameMessage>),
    AgentColor(Box<AgentColorMessage>),
    AgentSetting(Box<AgentSettingMessage>),
    PRLink(Box<PRLinkMessage>),
    FileHistorySnapshot(Box<FileHistorySnapshotMessage>),
    AttributionSnapshot(Box<AttributionSnapshotMessage>),
    QueueOperation(Box<QueueOperationMessage>),
    SpeculationAccept(Box<SpeculationAcceptMessage>),
    Mode(Box<ModeEntry>),
    WorktreeState(Box<WorktreeStateEntry>),
    ContentReplacement(Box<ContentReplacementEntry>),
    ContextCollapseCommit(Box<ContextCollapseCommitEntry>),
    ContextCollapseSnapshot(Box<ContextCollapseSnapshotEntry>),
    Unknown(serde_json::Value),
}

impl<'de> Deserialize<'de> for TranscriptEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let entry_type = value.get("type").and_then(serde_json::Value::as_str);

        match entry_type {
            Some("user" | "assistant" | "system" | "attachment") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::Message)
                .map_err(D::Error::custom),
            Some("progress") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::Progress)
                .map_err(D::Error::custom),
            Some("summary") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::Summary)
                .map_err(D::Error::custom),
            Some("custom-title") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::CustomTitle)
                .map_err(D::Error::custom),
            Some("ai-title") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::AiTitle)
                .map_err(D::Error::custom),
            Some("last-prompt") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::LastPrompt)
                .map_err(D::Error::custom),
            Some("task-summary") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::TaskSummary)
                .map_err(D::Error::custom),
            Some("tag") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::Tag)
                .map_err(D::Error::custom),
            Some("agent-name") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::AgentName)
                .map_err(D::Error::custom),
            Some("agent-color") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::AgentColor)
                .map_err(D::Error::custom),
            Some("agent-setting") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::AgentSetting)
                .map_err(D::Error::custom),
            Some("pr-link") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::PRLink)
                .map_err(D::Error::custom),
            Some("file-history-snapshot") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::FileHistorySnapshot)
                .map_err(D::Error::custom),
            Some("attribution-snapshot") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::AttributionSnapshot)
                .map_err(D::Error::custom),
            Some("queue-operation") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::QueueOperation)
                .map_err(D::Error::custom),
            Some("speculation-accept") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::SpeculationAccept)
                .map_err(D::Error::custom),
            Some("mode") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::Mode)
                .map_err(D::Error::custom),
            Some("worktree-state") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::WorktreeState)
                .map_err(D::Error::custom),
            Some("content-replacement") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::ContentReplacement)
                .map_err(D::Error::custom),
            Some("marble-origami-commit") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::ContextCollapseCommit)
                .map_err(D::Error::custom),
            Some("marble-origami-snapshot") => serde_json::from_value(value)
                .map(Box::new)
                .map(TranscriptEntry::ContextCollapseSnapshot)
                .map_err(D::Error::custom),
            Some(_) | None => Ok(TranscriptEntry::Unknown(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMessage {
    #[serde(rename = "type")]
    pub kind: TranscriptMessageKind,
    pub uuid: String,
    pub parent_uuid: Option<String>,
    pub logical_parent_uuid: Option<String>,
    pub is_sidechain: bool,
    pub git_branch: Option<String>,
    pub agent_id: Option<String>,
    pub team_name: Option<String>,
    pub agent_name: Option<String>,
    pub agent_color: Option<String>,
    pub prompt_id: Option<String>,
    pub cwd: String,
    pub user_type: String,
    pub entrypoint: String,
    pub session_id: String,
    pub timestamp: String,
    pub version: String,
    pub slug: Option<String>,
    pub request_id: Option<String>,
    pub permission_mode: Option<String>,
    pub subtype: Option<String>,
    pub duration_ms: Option<u64>,
    pub is_api_error_message: Option<bool>,
    pub message: TranscriptMessageBody,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptMessageKind {
    User,
    Assistant,
    Attachment,
    System,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TranscriptMessageBody {
    pub role: String,
    pub content: TranscriptContent,
    pub id: Option<String>,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub usage: Option<serde_json::Value>,
    pub context_management: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum TranscriptContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    Text(TextContentBlock),
    Thinking(ThinkingContentBlock),
    ToolUse(ToolUseContentBlock),
    ToolResult(ToolResultContentBlock),
    Image(ImageContentBlock),
    Unknown(serde_json::Value),
}

impl<'de> Deserialize<'de> for ContentBlock {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let block_type = value.get("type").and_then(serde_json::Value::as_str);

        match block_type {
            Some("text") => serde_json::from_value(value)
                .map(ContentBlock::Text)
                .map_err(D::Error::custom),
            Some("thinking") => serde_json::from_value(value)
                .map(ContentBlock::Thinking)
                .map_err(D::Error::custom),
            Some("tool_use") => serde_json::from_value(value)
                .map(ContentBlock::ToolUse)
                .map_err(D::Error::custom),
            Some("tool_result") => serde_json::from_value(value)
                .map(ContentBlock::ToolResult)
                .map_err(D::Error::custom),
            Some("image") => serde_json::from_value(value)
                .map(ContentBlock::Image)
                .map_err(D::Error::custom),
            Some(_) | None => Ok(ContentBlock::Unknown(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TextContentBlock {
    #[serde(rename = "type")]
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ThinkingContentBlock {
    #[serde(rename = "type")]
    pub kind: String,
    pub thinking: String,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ToolUseContentBlock {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ToolResultContentBlock {
    #[serde(rename = "type")]
    pub kind: String,
    pub content: ToolResultContent,
    pub is_error: bool,
    #[serde(rename = "tool_use_id")]
    pub tool_use_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Json(serde_json::Value),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ImageContentBlock {
    #[serde(rename = "type")]
    pub kind: String,
    pub source: ImageSource,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "media_type")]
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressMessage {
    #[serde(rename = "type")]
    pub kind: String,
    pub uuid: String,
    pub parent_uuid: Option<String>,
    pub logical_parent_uuid: Option<String>,
    pub is_sidechain: bool,
    pub git_branch: Option<String>,
    pub agent_id: Option<String>,
    pub prompt_id: Option<String>,
    pub cwd: String,
    pub user_type: String,
    pub entrypoint: String,
    pub session_id: String,
    pub timestamp: String,
    pub version: String,
    pub data: ProgressData,
    #[serde(rename = "toolUseID")]
    pub tool_use_id: Option<String>,
    #[serde(rename = "parentToolUseID")]
    pub parent_tool_use_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressData {
    #[serde(rename = "type")]
    pub kind: String,
    pub command: String,
    pub hook_event: String,
    pub hook_name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryMessage {
    pub leaf_uuid: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomTitleMessage {
    pub session_id: String,
    pub custom_title: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTitleMessage {
    pub session_id: String,
    pub ai_title: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastPromptMessage {
    pub session_id: String,
    pub last_prompt: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummaryMessage {
    pub session_id: String,
    pub summary: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagMessage {
    pub session_id: String,
    pub tag: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNameMessage {
    pub session_id: String,
    pub agent_name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentColorMessage {
    pub session_id: String,
    pub agent_color: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettingMessage {
    pub session_id: String,
    pub agent_setting: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PRLinkMessage {
    pub session_id: String,
    pub pr_number: u64,
    pub pr_repository: String,
    pub pr_url: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHistorySnapshotMessage {
    pub is_snapshot_update: bool,
    pub message_id: String,
    pub snapshot: FileHistorySnapshot,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHistorySnapshot {
    pub tracked_file_backups: BTreeMap<String, TrackedFileBackup>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedFileBackup {
    pub backup_path: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributionSnapshotMessage {
    pub escape_count: u64,
    pub escape_count_at_last_commit: u64,
    pub file_states: BTreeMap<String, AttributionFileState>,
    pub message_id: String,
    pub permission_prompt_count: u64,
    pub permission_prompt_count_at_last_commit: u64,
    pub prompt_count: u64,
    pub prompt_count_at_last_commit: u64,
    pub surface: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributionFileState {
    pub claude_contribution: u64,
    pub content_hash: String,
    pub mtime: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueOperationMessage {
    pub operation: QueueOperation,
    pub session_id: String,
    pub timestamp: String,
    pub content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueOperation {
    Enqueue,
    Dequeue,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeculationAcceptMessage {
    pub time_saved_ms: f64,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeEntry {
    pub mode: SessionMode,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionMode {
    Normal,
    Coordinator,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeStateEntry {
    pub session_id: String,
    pub worktree_session: WorktreeSession,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSession {
    pub hook_based: bool,
    pub original_branch: String,
    pub original_cwd: String,
    pub original_head_commit: String,
    pub session_id: String,
    pub tmux_session_name: String,
    pub worktree_branch: String,
    pub worktree_name: String,
    pub worktree_path: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentReplacementEntry {
    pub agent_id: String,
    pub replacements: Vec<ContentReplacement>,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentReplacement {
    pub kind: String,
    pub replacement: String,
    #[serde(rename = "toolUseId")]
    pub tool_use_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCollapseCommitEntry {
    pub collapse_id: String,
    pub first_archived_uuid: String,
    pub last_archived_uuid: String,
    pub session_id: String,
    pub summary: String,
    pub summary_content: String,
    pub summary_uuid: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCollapseSnapshotEntry {
    pub armed: bool,
    pub last_spawn_tokens: u64,
    pub session_id: String,
    pub staged: Vec<StagedCollapse>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedCollapse {
    pub end_uuid: String,
    pub risk: f64,
    pub staged_at: u64,
    pub start_uuid: String,
    pub summary: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const COMPREHENSIVE_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/transcripts/comprehensive.jsonl"
    ));
    const AUTH_FAILURE_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/transcripts/auth-failure.observed.jsonl"
    ));

    fn fixture_lines() -> impl Iterator<Item = &'static str> {
        COMPREHENSIVE_FIXTURE
            .lines()
            .chain(AUTH_FAILURE_FIXTURE.lines())
            .filter(|line| !line.trim().is_empty())
    }

    fn fixture_value_by_type(entry_type: &str) -> Value {
        fixture_lines()
            .find_map(|line| {
                let value = serde_json::from_str::<Value>(line)
                    .unwrap_or_else(|error| panic!("failed to parse fixture line: {error}"));
                (value.get("type").and_then(Value::as_str) == Some(entry_type)).then_some(value)
            })
            .unwrap_or_else(|| panic!("no transcript fixture line found for type {entry_type}"))
    }

    fn fixture_entry_by_type(entry_type: &str) -> TranscriptEntry {
        serde_json::from_value(fixture_value_by_type(entry_type)).unwrap_or_else(|error| {
            panic!("failed to deserialize transcript fixture type {entry_type}: {error}")
        })
    }

    macro_rules! transcript_fixture_test {
        ($test_name:ident, $entry_type:literal, $pattern:pat) => {
            #[test]
            fn $test_name() {
                let entry = fixture_entry_by_type($entry_type);
                assert!(
                    matches!(entry, $pattern),
                    "fixture type {} deserialized to unexpected variant: {:?}",
                    $entry_type,
                    entry
                );
            }
        };
    }

    #[test]
    fn transcript_deserializes_all_fixture_lines() {
        let entries = fixture_lines()
            .map(serde_json::from_str::<TranscriptEntry>)
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|error| panic!("fixture lines should deserialize: {error}"));

        assert_eq!(entries.len(), 30);
        assert!(entries
            .iter()
            .all(|entry| !matches!(entry, TranscriptEntry::Unknown(_))));
    }

    transcript_fixture_test!(
        transcript_agent_color_fixture_deserializes,
        "agent-color",
        TranscriptEntry::AgentColor(_)
    );
    transcript_fixture_test!(
        transcript_agent_name_fixture_deserializes,
        "agent-name",
        TranscriptEntry::AgentName(_)
    );
    transcript_fixture_test!(
        transcript_agent_setting_fixture_deserializes,
        "agent-setting",
        TranscriptEntry::AgentSetting(_)
    );
    transcript_fixture_test!(
        transcript_ai_title_fixture_deserializes,
        "ai-title",
        TranscriptEntry::AiTitle(_)
    );
    transcript_fixture_test!(
        transcript_assistant_fixture_deserializes,
        "assistant",
        TranscriptEntry::Message(_)
    );
    transcript_fixture_test!(
        transcript_attachment_fixture_deserializes,
        "attachment",
        TranscriptEntry::Message(_)
    );
    transcript_fixture_test!(
        transcript_attribution_snapshot_fixture_deserializes,
        "attribution-snapshot",
        TranscriptEntry::AttributionSnapshot(_)
    );
    transcript_fixture_test!(
        transcript_content_replacement_fixture_deserializes,
        "content-replacement",
        TranscriptEntry::ContentReplacement(_)
    );
    transcript_fixture_test!(
        transcript_custom_title_fixture_deserializes,
        "custom-title",
        TranscriptEntry::CustomTitle(_)
    );
    transcript_fixture_test!(
        transcript_file_history_snapshot_fixture_deserializes,
        "file-history-snapshot",
        TranscriptEntry::FileHistorySnapshot(_)
    );
    transcript_fixture_test!(
        transcript_last_prompt_fixture_deserializes,
        "last-prompt",
        TranscriptEntry::LastPrompt(_)
    );
    transcript_fixture_test!(
        transcript_mode_fixture_deserializes,
        "mode",
        TranscriptEntry::Mode(_)
    );
    transcript_fixture_test!(
        transcript_progress_fixture_deserializes,
        "progress",
        TranscriptEntry::Progress(_)
    );
    transcript_fixture_test!(
        transcript_pr_link_fixture_deserializes,
        "pr-link",
        TranscriptEntry::PRLink(_)
    );
    transcript_fixture_test!(
        transcript_queue_operation_fixture_deserializes,
        "queue-operation",
        TranscriptEntry::QueueOperation(_)
    );
    transcript_fixture_test!(
        transcript_speculation_accept_fixture_deserializes,
        "speculation-accept",
        TranscriptEntry::SpeculationAccept(_)
    );
    transcript_fixture_test!(
        transcript_summary_fixture_deserializes,
        "summary",
        TranscriptEntry::Summary(_)
    );
    transcript_fixture_test!(
        transcript_system_fixture_deserializes,
        "system",
        TranscriptEntry::Message(_)
    );
    transcript_fixture_test!(
        transcript_tag_fixture_deserializes,
        "tag",
        TranscriptEntry::Tag(_)
    );
    transcript_fixture_test!(
        transcript_task_summary_fixture_deserializes,
        "task-summary",
        TranscriptEntry::TaskSummary(_)
    );
    transcript_fixture_test!(
        transcript_user_fixture_deserializes,
        "user",
        TranscriptEntry::Message(_)
    );
    transcript_fixture_test!(
        transcript_worktree_state_fixture_deserializes,
        "worktree-state",
        TranscriptEntry::WorktreeState(_)
    );
    transcript_fixture_test!(
        transcript_context_collapse_commit_fixture_deserializes,
        "marble-origami-commit",
        TranscriptEntry::ContextCollapseCommit(_)
    );
    transcript_fixture_test!(
        transcript_context_collapse_snapshot_fixture_deserializes,
        "marble-origami-snapshot",
        TranscriptEntry::ContextCollapseSnapshot(_)
    );

    #[test]
    fn transcript_unknown_entry_type_deserializes_to_unknown() {
        let entry = serde_json::from_str::<TranscriptEntry>(
            r#"{"type":"future-entry","sessionId":"s-1","value":42}"#,
        )
        .unwrap_or_else(|error| panic!("unknown entry types should not fail: {error}"));

        match entry {
            TranscriptEntry::Unknown(value) => {
                assert_eq!(value["type"], "future-entry");
                assert_eq!(value["value"], 42);
            }
            other => panic!("expected unknown transcript entry, got {other:?}"),
        }
    }

    #[test]
    fn transcript_unknown_fields_are_ignored_during_deserialization() {
        let mut value = fixture_value_by_type("assistant");
        let object = value
            .as_object_mut()
            .unwrap_or_else(|| panic!("assistant fixture should be a JSON object"));

        object.insert(
            "future_field".to_owned(),
            serde_json::json!("forward-compatible"),
        );
        object.insert(
            "future_nested".to_owned(),
            serde_json::json!({ "enabled": true }),
        );

        let entry = serde_json::from_value::<TranscriptEntry>(value).unwrap_or_else(|error| {
            panic!("augmented transcript entry should deserialize: {error}")
        });

        assert!(matches!(entry, TranscriptEntry::Message(_)));
    }

    #[test]
    fn transcript_parses_message_content_blocks() {
        let assistant = serde_json::from_str::<TranscriptEntry>(
            r#"{
                "cwd":"/workspace/claude-insight",
                "entrypoint":"sdk-cli",
                "gitBranch":"ticket/mot-115-hook-corpus",
                "isSidechain":false,
                "message":{
                    "content":[
                        {"text":"hello","type":"text"},
                        {"id":"toolu_123","input":{"path":"docs/DESIGN.md"},"name":"Read","type":"tool_use"},
                        {"content":"ok","is_error":false,"tool_use_id":"toolu_123","type":"tool_result"}
                    ],
                    "role":"assistant"
                },
                "parentUuid":"44444444-4444-4444-8444-444444444444",
                "requestId":"req_123",
                "sessionId":"11111111-1111-4111-8111-111111111111",
                "timestamp":"2026-04-02T15:34:15.784Z",
                "type":"assistant",
                "userType":"external",
                "uuid":"55555555-5555-4555-8555-555555555555",
                "version":"2.1.81"
            }"#,
        )
        .unwrap_or_else(|error| panic!("assistant entry should deserialize: {error}"));

        let TranscriptEntry::Message(message) = assistant else {
            panic!("expected assistant message variant");
        };

        let TranscriptContent::Blocks(blocks) = message.message.content else {
            panic!("expected block content");
        };

        assert!(matches!(blocks[0], ContentBlock::Text(_)));
        assert!(matches!(blocks[1], ContentBlock::ToolUse(_)));
        assert!(matches!(blocks[2], ContentBlock::ToolResult(_)));
    }
}
