use std::{collections::BTreeMap, fmt};

use serde::de::Error as _;
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnknownHookInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    #[serde(rename = "hook_event_name")]
    pub raw_hook_event_name: Option<String>,
    #[serde(flatten)]
    pub extra_fields: BTreeMap<String, Value>,
}

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
    Unknown(UnknownHookInput),
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
            Self::Unknown(input) => &input.base,
        }
    }

    pub fn hook_event_name(&self) -> HookEventName {
        self.base().hook_event_name
    }
}

impl<'de> Deserialize<'de> for HookEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let hook_event_name = value
            .get("hook_event_name")
            .and_then(Value::as_str)
            .map(str::to_owned);

        macro_rules! deserialize_known {
            ($value:expr, $ty:ty, $variant:ident, $name:ident) => {{
                let mut input: $ty = serde_json::from_value($value).map_err(D::Error::custom)?;
                input.base.hook_event_name = HookEventName::$name;
                Ok(Self::$variant(input))
            }};
        }

        match hook_event_name.as_deref() {
            Some("PreToolUse") => {
                deserialize_known!(value, PreToolUseInput, PreToolUse, PreToolUse)
            }
            Some("PostToolUse") => {
                deserialize_known!(value, PostToolUseInput, PostToolUse, PostToolUse)
            }
            Some("PostToolUseFailure") => deserialize_known!(
                value,
                PostToolUseFailureInput,
                PostToolUseFailure,
                PostToolUseFailure
            ),
            Some("Notification") => {
                deserialize_known!(value, NotificationInput, Notification, Notification)
            }
            Some("UserPromptSubmit") => deserialize_known!(
                value,
                UserPromptSubmitInput,
                UserPromptSubmit,
                UserPromptSubmit
            ),
            Some("SessionStart") => {
                deserialize_known!(value, SessionStartInput, SessionStart, SessionStart)
            }
            Some("SessionEnd") => {
                deserialize_known!(value, SessionEndInput, SessionEnd, SessionEnd)
            }
            Some("Stop") => deserialize_known!(value, StopInput, Stop, Stop),
            Some("StopFailure") => {
                deserialize_known!(value, StopFailureInput, StopFailure, StopFailure)
            }
            Some("SubagentStart") => {
                deserialize_known!(value, SubagentStartInput, SubagentStart, SubagentStart)
            }
            Some("SubagentStop") => {
                deserialize_known!(value, SubagentStopInput, SubagentStop, SubagentStop)
            }
            Some("PreCompact") => {
                deserialize_known!(value, PreCompactInput, PreCompact, PreCompact)
            }
            Some("PostCompact") => {
                deserialize_known!(value, PostCompactInput, PostCompact, PostCompact)
            }
            Some("PermissionRequest") => deserialize_known!(
                value,
                PermissionRequestInput,
                PermissionRequest,
                PermissionRequest
            ),
            Some("PermissionDenied") => deserialize_known!(
                value,
                PermissionDeniedInput,
                PermissionDenied,
                PermissionDenied
            ),
            Some("Setup") => deserialize_known!(value, SetupInput, Setup, Setup),
            Some("TeammateIdle") => {
                deserialize_known!(value, TeammateIdleInput, TeammateIdle, TeammateIdle)
            }
            Some("TaskCreated") => {
                deserialize_known!(value, TaskCreatedInput, TaskCreated, TaskCreated)
            }
            Some("TaskCompleted") => {
                deserialize_known!(value, TaskCompletedInput, TaskCompleted, TaskCompleted)
            }
            Some("Elicitation") => {
                deserialize_known!(value, ElicitationInput, Elicitation, Elicitation)
            }
            Some("ElicitationResult") => deserialize_known!(
                value,
                ElicitationResultInput,
                ElicitationResult,
                ElicitationResult
            ),
            Some("ConfigChange") => {
                deserialize_known!(value, ConfigChangeInput, ConfigChange, ConfigChange)
            }
            Some("WorktreeCreate") => {
                deserialize_known!(value, WorktreeCreateInput, WorktreeCreate, WorktreeCreate)
            }
            Some("WorktreeRemove") => {
                deserialize_known!(value, WorktreeRemoveInput, WorktreeRemove, WorktreeRemove)
            }
            Some("InstructionsLoaded") => deserialize_known!(
                value,
                InstructionsLoadedInput,
                InstructionsLoaded,
                InstructionsLoaded
            ),
            Some("CwdChanged") => {
                deserialize_known!(value, CwdChangedInput, CwdChanged, CwdChanged)
            }
            Some("FileChanged") => {
                deserialize_known!(value, FileChangedInput, FileChanged, FileChanged)
            }
            Some(_) | None => serde_json::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
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

    fn fixture_event(fixture: &str) -> HookEvent {
        let path = fixture_dir().join(format!("{fixture}.json"));
        let json = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));

        serde_json::from_str::<HookEvent>(&json)
            .unwrap_or_else(|error| panic!("failed to deserialize {}: {error}", path.display()))
    }

    macro_rules! hook_fixture_test {
        ($test_name:ident, $fixture:literal, $variant:path, $event_name:ident) => {
            #[test]
            fn $test_name() {
                let event = fixture_event($fixture);

                assert_eq!(event.hook_event_name(), HookEventName::$event_name);
                assert_eq!(event.hook_event_name().as_str(), $fixture);
                assert!(matches!(event, $variant(_)));
            }
        };
    }

    #[test]
    fn hook_fixture_set_contains_all_27_events() {
        let paths = fixture_paths();

        assert_eq!(paths.len(), 27, "expected one fixture for each hook event");
    }

    hook_fixture_test!(
        config_change_fixture_deserializes,
        "ConfigChange",
        HookEvent::ConfigChange,
        ConfigChange
    );
    hook_fixture_test!(
        cwd_changed_fixture_deserializes,
        "CwdChanged",
        HookEvent::CwdChanged,
        CwdChanged
    );
    hook_fixture_test!(
        elicitation_fixture_deserializes,
        "Elicitation",
        HookEvent::Elicitation,
        Elicitation
    );
    hook_fixture_test!(
        elicitation_result_fixture_deserializes,
        "ElicitationResult",
        HookEvent::ElicitationResult,
        ElicitationResult
    );
    hook_fixture_test!(
        file_changed_fixture_deserializes,
        "FileChanged",
        HookEvent::FileChanged,
        FileChanged
    );
    hook_fixture_test!(
        instructions_loaded_fixture_deserializes,
        "InstructionsLoaded",
        HookEvent::InstructionsLoaded,
        InstructionsLoaded
    );
    hook_fixture_test!(
        notification_fixture_deserializes,
        "Notification",
        HookEvent::Notification,
        Notification
    );
    hook_fixture_test!(
        permission_denied_fixture_deserializes,
        "PermissionDenied",
        HookEvent::PermissionDenied,
        PermissionDenied
    );
    hook_fixture_test!(
        permission_request_fixture_deserializes,
        "PermissionRequest",
        HookEvent::PermissionRequest,
        PermissionRequest
    );
    hook_fixture_test!(
        post_compact_fixture_deserializes,
        "PostCompact",
        HookEvent::PostCompact,
        PostCompact
    );
    hook_fixture_test!(
        post_tool_use_fixture_deserializes,
        "PostToolUse",
        HookEvent::PostToolUse,
        PostToolUse
    );
    hook_fixture_test!(
        post_tool_use_failure_fixture_deserializes,
        "PostToolUseFailure",
        HookEvent::PostToolUseFailure,
        PostToolUseFailure
    );
    hook_fixture_test!(
        pre_compact_fixture_deserializes,
        "PreCompact",
        HookEvent::PreCompact,
        PreCompact
    );
    hook_fixture_test!(
        pre_tool_use_fixture_deserializes,
        "PreToolUse",
        HookEvent::PreToolUse,
        PreToolUse
    );
    hook_fixture_test!(
        session_end_fixture_deserializes,
        "SessionEnd",
        HookEvent::SessionEnd,
        SessionEnd
    );
    hook_fixture_test!(
        session_start_fixture_deserializes,
        "SessionStart",
        HookEvent::SessionStart,
        SessionStart
    );
    hook_fixture_test!(setup_fixture_deserializes, "Setup", HookEvent::Setup, Setup);
    hook_fixture_test!(stop_fixture_deserializes, "Stop", HookEvent::Stop, Stop);
    hook_fixture_test!(
        stop_failure_fixture_deserializes,
        "StopFailure",
        HookEvent::StopFailure,
        StopFailure
    );
    hook_fixture_test!(
        subagent_start_fixture_deserializes,
        "SubagentStart",
        HookEvent::SubagentStart,
        SubagentStart
    );
    hook_fixture_test!(
        subagent_stop_fixture_deserializes,
        "SubagentStop",
        HookEvent::SubagentStop,
        SubagentStop
    );
    hook_fixture_test!(
        task_completed_fixture_deserializes,
        "TaskCompleted",
        HookEvent::TaskCompleted,
        TaskCompleted
    );
    hook_fixture_test!(
        task_created_fixture_deserializes,
        "TaskCreated",
        HookEvent::TaskCreated,
        TaskCreated
    );
    hook_fixture_test!(
        teammate_idle_fixture_deserializes,
        "TeammateIdle",
        HookEvent::TeammateIdle,
        TeammateIdle
    );
    hook_fixture_test!(
        user_prompt_submit_fixture_deserializes,
        "UserPromptSubmit",
        HookEvent::UserPromptSubmit,
        UserPromptSubmit
    );
    hook_fixture_test!(
        worktree_create_fixture_deserializes,
        "WorktreeCreate",
        HookEvent::WorktreeCreate,
        WorktreeCreate
    );
    hook_fixture_test!(
        worktree_remove_fixture_deserializes,
        "WorktreeRemove",
        HookEvent::WorktreeRemove,
        WorktreeRemove
    );

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

    #[test]
    fn unknown_hook_event_types_deserialize_to_unknown_variant() {
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
            "hook_event_name".to_owned(),
            serde_json::json!("FutureHookEvent"),
        );
        object.insert("future_field".to_owned(), serde_json::json!(42));

        let event = serde_json::from_value::<HookEvent>(payload)
            .unwrap_or_else(|error| panic!("failed to deserialize unknown hook event: {error}"));

        match event {
            HookEvent::Unknown(input) => {
                assert_eq!(input.base.hook_event_name, HookEventName::Unknown);
                assert_eq!(
                    input.raw_hook_event_name.as_deref(),
                    Some("FutureHookEvent")
                );
                assert_eq!(
                    input.extra_fields.get("future_field"),
                    Some(&serde_json::json!(42))
                );
            }
            other => panic!("expected unknown hook event, got {other:?}"),
        }
    }
}
