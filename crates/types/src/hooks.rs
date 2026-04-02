use std::fmt;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HookEventName {
    #[default]
    Unknown,
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    Notification,
    UserPromptSubmit,
    SessionStart,
    SessionEnd,
    Stop,
    StopFailure,
    SubagentStart,
    SubagentStop,
    PreCompact,
    PostCompact,
    PermissionRequest,
    PermissionDenied,
    Setup,
    TeammateIdle,
    TaskCreated,
    TaskCompleted,
    Elicitation,
    ElicitationResult,
    ConfigChange,
    WorktreeCreate,
    WorktreeRemove,
    InstructionsLoaded,
    CwdChanged,
    FileChanged,
}

impl HookEventName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::PostToolUseFailure => "PostToolUseFailure",
            Self::Notification => "Notification",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::Stop => "Stop",
            Self::StopFailure => "StopFailure",
            Self::SubagentStart => "SubagentStart",
            Self::SubagentStop => "SubagentStop",
            Self::PreCompact => "PreCompact",
            Self::PostCompact => "PostCompact",
            Self::PermissionRequest => "PermissionRequest",
            Self::PermissionDenied => "PermissionDenied",
            Self::Setup => "Setup",
            Self::TeammateIdle => "TeammateIdle",
            Self::TaskCreated => "TaskCreated",
            Self::TaskCompleted => "TaskCompleted",
            Self::Elicitation => "Elicitation",
            Self::ElicitationResult => "ElicitationResult",
            Self::ConfigChange => "ConfigChange",
            Self::WorktreeCreate => "WorktreeCreate",
            Self::WorktreeRemove => "WorktreeRemove",
            Self::InstructionsLoaded => "InstructionsLoaded",
            Self::CwdChanged => "CwdChanged",
            Self::FileChanged => "FileChanged",
        }
    }
}

impl fmt::Display for HookEventName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BaseHookInput {
    pub session_id: String,
    pub transcript_path: String,
    pub cwd: String,
    pub permission_mode: Option<String>,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    #[serde(skip_deserializing, default)]
    pub hook_event_name: HookEventName,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PermissionSuggestion {
    pub label: String,
    pub rule: String,
}

macro_rules! hook_input {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Deserialize)]
        pub struct $name {
            #[serde(flatten)]
            pub base: BaseHookInput,
            $(pub $field: $ty,)*
        }
    };
}

hook_input!(PreToolUseInput {
    tool_name: String,
    tool_input: Value,
    tool_use_id: String,
});
hook_input!(PostToolUseInput {
    tool_name: String,
    tool_input: Value,
    tool_response: Value,
    tool_use_id: String,
});
hook_input!(PostToolUseFailureInput {
    tool_name: String,
    tool_input: Value,
    tool_use_id: String,
    error: String,
    is_interrupt: Option<bool>,
});
hook_input!(NotificationInput {
    message: String,
    title: Option<String>,
    notification_type: String,
});
hook_input!(UserPromptSubmitInput { prompt: String });
hook_input!(SessionStartInput {
    source: String,
    model: Option<String>,
});
hook_input!(SessionEndInput { reason: String });
hook_input!(StopInput {
    stop_hook_active: bool,
    last_assistant_message: Option<String>,
});
hook_input!(StopFailureInput {
    error: String,
    error_details: Option<String>,
    last_assistant_message: Option<String>,
});
hook_input!(SubagentStartInput {});
hook_input!(SubagentStopInput {
    stop_hook_active: bool,
    agent_transcript_path: String,
    last_assistant_message: Option<String>,
});
hook_input!(PreCompactInput {
    trigger: String,
    custom_instructions: String,
});
hook_input!(PostCompactInput {
    trigger: String,
    compact_summary: String,
});
hook_input!(PermissionRequestInput {
    tool_name: String,
    tool_input: Value,
    permission_suggestions: Option<Vec<PermissionSuggestion>>,
});
hook_input!(PermissionDeniedInput {
    tool_name: String,
    tool_input: Value,
    tool_use_id: String,
    reason: String,
});
hook_input!(SetupInput { trigger: String });
hook_input!(TeammateIdleInput {
    teammate_name: String,
    team_name: String,
});
hook_input!(TaskCreatedInput {
    task_id: String,
    task_subject: String,
    task_description: Option<String>,
    teammate_name: Option<String>,
    team_name: Option<String>,
});
hook_input!(TaskCompletedInput {
    task_id: String,
    task_subject: String,
    task_description: Option<String>,
    teammate_name: Option<String>,
    team_name: Option<String>,
});
hook_input!(ElicitationInput {
    mcp_server_name: String,
    message: String,
    mode: Option<String>,
    url: Option<String>,
    elicitation_id: Option<String>,
    requested_schema: Option<Value>,
});
hook_input!(ElicitationResultInput {
    mcp_server_name: String,
    elicitation_id: Option<String>,
    mode: Option<String>,
    action: String,
    content: Option<Value>,
});
hook_input!(ConfigChangeInput {
    source: String,
    file_path: Option<String>,
});
hook_input!(WorktreeCreateInput { name: String });
hook_input!(WorktreeRemoveInput {
    worktree_path: String
});
hook_input!(InstructionsLoadedInput {
    file_path: String,
    memory_type: String,
    load_reason: String,
    globs: Option<Vec<String>>,
    trigger_file_path: Option<String>,
    parent_file_path: Option<String>,
});
hook_input!(CwdChangedInput {
    old_cwd: String,
    new_cwd: String,
});
hook_input!(FileChangedInput {
    file_path: String,
    event: String,
});

#[derive(Debug, Clone, PartialEq)]
pub enum HookEvent {
    PreToolUse(PreToolUseInput),
    PostToolUse(PostToolUseInput),
    PostToolUseFailure(PostToolUseFailureInput),
    Notification(NotificationInput),
    UserPromptSubmit(UserPromptSubmitInput),
    SessionStart(SessionStartInput),
    SessionEnd(SessionEndInput),
    Stop(StopInput),
    StopFailure(StopFailureInput),
    SubagentStart(SubagentStartInput),
    SubagentStop(SubagentStopInput),
    PreCompact(PreCompactInput),
    PostCompact(PostCompactInput),
    PermissionRequest(PermissionRequestInput),
    PermissionDenied(PermissionDeniedInput),
    Setup(SetupInput),
    TeammateIdle(TeammateIdleInput),
    TaskCreated(TaskCreatedInput),
    TaskCompleted(TaskCompletedInput),
    Elicitation(ElicitationInput),
    ElicitationResult(ElicitationResultInput),
    ConfigChange(ConfigChangeInput),
    WorktreeCreate(WorktreeCreateInput),
    WorktreeRemove(WorktreeRemoveInput),
    InstructionsLoaded(InstructionsLoadedInput),
    CwdChanged(CwdChangedInput),
    FileChanged(FileChangedInput),
}

impl HookEvent {
    pub fn base(&self) -> &BaseHookInput {
        match self {
            Self::PreToolUse(input) => &input.base,
            Self::PostToolUse(input) => &input.base,
            Self::PostToolUseFailure(input) => &input.base,
            Self::Notification(input) => &input.base,
            Self::UserPromptSubmit(input) => &input.base,
            Self::SessionStart(input) => &input.base,
            Self::SessionEnd(input) => &input.base,
            Self::Stop(input) => &input.base,
            Self::StopFailure(input) => &input.base,
            Self::SubagentStart(input) => &input.base,
            Self::SubagentStop(input) => &input.base,
            Self::PreCompact(input) => &input.base,
            Self::PostCompact(input) => &input.base,
            Self::PermissionRequest(input) => &input.base,
            Self::PermissionDenied(input) => &input.base,
            Self::Setup(input) => &input.base,
            Self::TeammateIdle(input) => &input.base,
            Self::TaskCreated(input) => &input.base,
            Self::TaskCompleted(input) => &input.base,
            Self::Elicitation(input) => &input.base,
            Self::ElicitationResult(input) => &input.base,
            Self::ConfigChange(input) => &input.base,
            Self::WorktreeCreate(input) => &input.base,
            Self::WorktreeRemove(input) => &input.base,
            Self::InstructionsLoaded(input) => &input.base,
            Self::CwdChanged(input) => &input.base,
            Self::FileChanged(input) => &input.base,
        }
    }

    pub fn hook_event_name(&self) -> HookEventName {
        self.base().hook_event_name
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "hook_event_name")]
enum HookEventRepr {
    #[serde(rename = "PreToolUse")]
    PreToolUse(PreToolUseInput),
    #[serde(rename = "PostToolUse")]
    PostToolUse(PostToolUseInput),
    #[serde(rename = "PostToolUseFailure")]
    PostToolUseFailure(PostToolUseFailureInput),
    #[serde(rename = "Notification")]
    Notification(NotificationInput),
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit(UserPromptSubmitInput),
    #[serde(rename = "SessionStart")]
    SessionStart(SessionStartInput),
    #[serde(rename = "SessionEnd")]
    SessionEnd(SessionEndInput),
    #[serde(rename = "Stop")]
    Stop(StopInput),
    #[serde(rename = "StopFailure")]
    StopFailure(StopFailureInput),
    #[serde(rename = "SubagentStart")]
    SubagentStart(SubagentStartInput),
    #[serde(rename = "SubagentStop")]
    SubagentStop(SubagentStopInput),
    #[serde(rename = "PreCompact")]
    PreCompact(PreCompactInput),
    #[serde(rename = "PostCompact")]
    PostCompact(PostCompactInput),
    #[serde(rename = "PermissionRequest")]
    PermissionRequest(PermissionRequestInput),
    #[serde(rename = "PermissionDenied")]
    PermissionDenied(PermissionDeniedInput),
    #[serde(rename = "Setup")]
    Setup(SetupInput),
    #[serde(rename = "TeammateIdle")]
    TeammateIdle(TeammateIdleInput),
    #[serde(rename = "TaskCreated")]
    TaskCreated(TaskCreatedInput),
    #[serde(rename = "TaskCompleted")]
    TaskCompleted(TaskCompletedInput),
    #[serde(rename = "Elicitation")]
    Elicitation(ElicitationInput),
    #[serde(rename = "ElicitationResult")]
    ElicitationResult(ElicitationResultInput),
    #[serde(rename = "ConfigChange")]
    ConfigChange(ConfigChangeInput),
    #[serde(rename = "WorktreeCreate")]
    WorktreeCreate(WorktreeCreateInput),
    #[serde(rename = "WorktreeRemove")]
    WorktreeRemove(WorktreeRemoveInput),
    #[serde(rename = "InstructionsLoaded")]
    InstructionsLoaded(InstructionsLoadedInput),
    #[serde(rename = "CwdChanged")]
    CwdChanged(CwdChangedInput),
    #[serde(rename = "FileChanged")]
    FileChanged(FileChangedInput),
}

impl<'de> Deserialize<'de> for HookEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let event = HookEventRepr::deserialize(deserializer)?;

        Ok(match event {
            HookEventRepr::PreToolUse(mut input) => {
                input.base.hook_event_name = HookEventName::PreToolUse;
                Self::PreToolUse(input)
            }
            HookEventRepr::PostToolUse(mut input) => {
                input.base.hook_event_name = HookEventName::PostToolUse;
                Self::PostToolUse(input)
            }
            HookEventRepr::PostToolUseFailure(mut input) => {
                input.base.hook_event_name = HookEventName::PostToolUseFailure;
                Self::PostToolUseFailure(input)
            }
            HookEventRepr::Notification(mut input) => {
                input.base.hook_event_name = HookEventName::Notification;
                Self::Notification(input)
            }
            HookEventRepr::UserPromptSubmit(mut input) => {
                input.base.hook_event_name = HookEventName::UserPromptSubmit;
                Self::UserPromptSubmit(input)
            }
            HookEventRepr::SessionStart(mut input) => {
                input.base.hook_event_name = HookEventName::SessionStart;
                Self::SessionStart(input)
            }
            HookEventRepr::SessionEnd(mut input) => {
                input.base.hook_event_name = HookEventName::SessionEnd;
                Self::SessionEnd(input)
            }
            HookEventRepr::Stop(mut input) => {
                input.base.hook_event_name = HookEventName::Stop;
                Self::Stop(input)
            }
            HookEventRepr::StopFailure(mut input) => {
                input.base.hook_event_name = HookEventName::StopFailure;
                Self::StopFailure(input)
            }
            HookEventRepr::SubagentStart(mut input) => {
                input.base.hook_event_name = HookEventName::SubagentStart;
                Self::SubagentStart(input)
            }
            HookEventRepr::SubagentStop(mut input) => {
                input.base.hook_event_name = HookEventName::SubagentStop;
                Self::SubagentStop(input)
            }
            HookEventRepr::PreCompact(mut input) => {
                input.base.hook_event_name = HookEventName::PreCompact;
                Self::PreCompact(input)
            }
            HookEventRepr::PostCompact(mut input) => {
                input.base.hook_event_name = HookEventName::PostCompact;
                Self::PostCompact(input)
            }
            HookEventRepr::PermissionRequest(mut input) => {
                input.base.hook_event_name = HookEventName::PermissionRequest;
                Self::PermissionRequest(input)
            }
            HookEventRepr::PermissionDenied(mut input) => {
                input.base.hook_event_name = HookEventName::PermissionDenied;
                Self::PermissionDenied(input)
            }
            HookEventRepr::Setup(mut input) => {
                input.base.hook_event_name = HookEventName::Setup;
                Self::Setup(input)
            }
            HookEventRepr::TeammateIdle(mut input) => {
                input.base.hook_event_name = HookEventName::TeammateIdle;
                Self::TeammateIdle(input)
            }
            HookEventRepr::TaskCreated(mut input) => {
                input.base.hook_event_name = HookEventName::TaskCreated;
                Self::TaskCreated(input)
            }
            HookEventRepr::TaskCompleted(mut input) => {
                input.base.hook_event_name = HookEventName::TaskCompleted;
                Self::TaskCompleted(input)
            }
            HookEventRepr::Elicitation(mut input) => {
                input.base.hook_event_name = HookEventName::Elicitation;
                Self::Elicitation(input)
            }
            HookEventRepr::ElicitationResult(mut input) => {
                input.base.hook_event_name = HookEventName::ElicitationResult;
                Self::ElicitationResult(input)
            }
            HookEventRepr::ConfigChange(mut input) => {
                input.base.hook_event_name = HookEventName::ConfigChange;
                Self::ConfigChange(input)
            }
            HookEventRepr::WorktreeCreate(mut input) => {
                input.base.hook_event_name = HookEventName::WorktreeCreate;
                Self::WorktreeCreate(input)
            }
            HookEventRepr::WorktreeRemove(mut input) => {
                input.base.hook_event_name = HookEventName::WorktreeRemove;
                Self::WorktreeRemove(input)
            }
            HookEventRepr::InstructionsLoaded(mut input) => {
                input.base.hook_event_name = HookEventName::InstructionsLoaded;
                Self::InstructionsLoaded(input)
            }
            HookEventRepr::CwdChanged(mut input) => {
                input.base.hook_event_name = HookEventName::CwdChanged;
                Self::CwdChanged(input)
            }
            HookEventRepr::FileChanged(mut input) => {
                input.base.hook_event_name = HookEventName::FileChanged;
                Self::FileChanged(input)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::{HookEvent, HookEventName};

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/hooks")
    }

    fn fixture_paths() -> Vec<PathBuf> {
        let entries = fs::read_dir(fixture_dir())
            .unwrap_or_else(|error| panic!("failed to read hook fixture directory: {error}"));
        let mut paths = entries
            .map(|entry| {
                entry
                    .unwrap_or_else(|error| panic!("failed to read fixture dir entry: {error}"))
                    .path()
            })
            .collect::<Vec<_>>();

        paths.sort_by(|left, right| {
            left.file_name()
                .unwrap_or_else(|| panic!("fixture path missing file name: {}", left.display()))
                .cmp(right.file_name().unwrap_or_else(|| {
                    panic!("fixture path missing file name: {}", right.display())
                }))
        });

        paths
    }

    fn fixture_name(path: &PathBuf) -> String {
        path.file_stem()
            .unwrap_or_else(|| panic!("fixture path missing file stem: {}", path.display()))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn hook_fixture_set_contains_all_27_events() {
        let paths = fixture_paths();

        assert_eq!(paths.len(), 27, "expected one fixture for each hook event");
    }

    #[test]
    fn every_hook_fixture_deserializes_to_the_matching_variant() {
        for path in fixture_paths() {
            let fixture_name = fixture_name(&path);
            let json = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let event = serde_json::from_str::<HookEvent>(&json).unwrap_or_else(|error| {
                panic!("failed to deserialize {}: {error}", path.display())
            });

            assert_eq!(event.hook_event_name().as_str(), fixture_name);
            assert_eq!(event.base().hook_event_name.as_str(), fixture_name);
        }
    }

    #[test]
    fn unknown_fields_are_ignored_during_deserialization() {
        let path = fixture_dir().join("PreToolUse.json");
        let mut payload = serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display())),
        )
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));

        let object = payload.as_object_mut().unwrap_or_else(|| {
            panic!(
                "fixture payload for {} was not a JSON object",
                path.display()
            )
        });
        object.insert(
            "future_field".to_owned(),
            serde_json::json!("forward-compatible"),
        );
        object.insert(
            "nested_future".to_owned(),
            serde_json::json!({ "enabled": true }),
        );

        let event = serde_json::from_value::<HookEvent>(payload)
            .unwrap_or_else(|error| panic!("failed to deserialize augmented fixture: {error}"));

        assert_eq!(event.hook_event_name(), HookEventName::PreToolUse);
        assert!(matches!(event, HookEvent::PreToolUse(_)));
    }
}
