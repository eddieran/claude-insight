#!/usr/bin/env python3
from __future__ import annotations

import json
from copy import deepcopy
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
OUT_DIR = REPO_ROOT / ".research" / "claude-hook-probe" / "out"
FIXTURES_DIR = REPO_ROOT / "tests" / "fixtures"
HOOKS_DIR = FIXTURES_DIR / "hooks"
TRANSCRIPTS_DIR = FIXTURES_DIR / "transcripts"
SETTINGS_DIR = FIXTURES_DIR / "settings"

WORKSPACE_ROOT = "/workspace/claude-insight"
CLAUDE_HOME = "/workspace/.claude"
TRANSCRIPT_PATH = f"{CLAUDE_HOME}/projects/claude-insight/11111111-1111-4111-8111-111111111111.jsonl"
SUBAGENT_TRANSCRIPT_PATH = (
    f"{CLAUDE_HOME}/projects/claude-insight/11111111-1111-4111-8111-111111111111/"
    "subagents/agent-22222222-2222-4222-8222-222222222222.jsonl"
)
SESSION_ID = "11111111-1111-4111-8111-111111111111"
AGENT_ID = "22222222-2222-4222-8222-222222222222"
PROMPT_ID = "33333333-3333-4333-8333-333333333333"
TOOL_USE_ID = "toolu_01MOT115fixture"
REQUEST_ID = "req_mot115_fixture"
BRANCH_NAME = "ticket/mot-115-hook-corpus"
VERSION = "2.1.81"

HOOK_EVENTS = [
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Notification",
    "UserPromptSubmit",
    "SessionStart",
    "SessionEnd",
    "Stop",
    "StopFailure",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
    "PermissionRequest",
    "PermissionDenied",
    "Setup",
    "TeammateIdle",
    "TaskCreated",
    "TaskCompleted",
    "Elicitation",
    "ElicitationResult",
    "ConfigChange",
    "WorktreeCreate",
    "WorktreeRemove",
    "InstructionsLoaded",
    "CwdChanged",
    "FileChanged",
]

PATH_KEYS = {
    "cwd",
    "old_cwd",
    "new_cwd",
    "transcript_path",
    "agent_transcript_path",
    "file_path",
    "trigger_file_path",
    "parent_file_path",
    "worktree_path",
    "original_cwd",
    "worktreePath",
    "originalCwd",
}
BRANCH_KEYS = {"gitBranch", "worktreeBranch", "originalBranch"}


def sanitize_path(value: str) -> str:
    if value.startswith(str(REPO_ROOT)):
        rel = Path(value).relative_to(REPO_ROOT).as_posix()
        return f"{WORKSPACE_ROOT}/{rel}"
    if "/projects/" in value and value.endswith(".jsonl"):
        if "/subagents/" in value:
            return SUBAGENT_TRANSCRIPT_PATH
        return TRANSCRIPT_PATH
    if value.endswith("CLAUDE.md"):
        return f"{WORKSPACE_ROOT}/CLAUDE.md"
    if ".claude/rules/" in value:
        return f"{WORKSPACE_ROOT}/.claude/rules/{Path(value).name}"
    if value.endswith("settings.json"):
        return f"{WORKSPACE_ROOT}/.claude/settings.json"
    if value.endswith(".mcp.json"):
        return f"{WORKSPACE_ROOT}/.mcp.json"
    if value.startswith("/Users/"):
        return f"/workspace/redacted/{Path(value).name}"
    return value


def sanitize(value, key: str | None = None):
    if isinstance(value, dict):
        return {k: sanitize(v, k) for k, v in value.items()}
    if isinstance(value, list):
        return [sanitize(item, key) for item in value]
    if isinstance(value, str):
        if key in PATH_KEYS or (key and (key.endswith("_path") or key.endswith("Path"))):
            return sanitize_path(value)
        if key in BRANCH_KEYS:
            return BRANCH_NAME
        if value.startswith("/Users/"):
            return sanitize_path(value)
        if value == str(REPO_ROOT):
            return WORKSPACE_ROOT
        if str(REPO_ROOT) in value:
            return value.replace(str(REPO_ROOT), WORKSPACE_ROOT)
        if "eddingsuree/mot-115-spike-corpus-collection-capture-all-27-hook-events" in value:
            return value.replace(
                "eddingsuree/mot-115-spike-corpus-collection-capture-all-27-hook-events",
                BRANCH_NAME,
            )
    return value


def hook_base(event_name: str) -> dict:
    payload = {
        "session_id": SESSION_ID,
        "transcript_path": TRANSCRIPT_PATH,
        "cwd": WORKSPACE_ROOT,
        "hook_event_name": event_name,
    }
    if event_name not in {"SessionStart", "SessionEnd"}:
        payload["permission_mode"] = "acceptEdits"
    return payload


def documented_hook_payloads() -> dict[str, dict]:
    payloads: dict[str, dict] = {}

    for event_name in HOOK_EVENTS:
        payloads[event_name] = hook_base(event_name)

    payloads["SessionStart"].update(
        {"source": "startup", "model": "claude-sonnet-4-20250514"}
    )
    payloads["SessionEnd"].update({"reason": "prompt_input_exit"})
    payloads["PreToolUse"].update(
        {
            "tool_name": "Read",
            "tool_input": {"file_path": f"{WORKSPACE_ROOT}/docs/DESIGN.md", "limit": 200},
            "tool_use_id": TOOL_USE_ID,
        }
    )
    payloads["PostToolUse"].update(
        {
            "tool_name": "Read",
            "tool_input": {"file_path": f"{WORKSPACE_ROOT}/docs/DESIGN.md", "limit": 200},
            "tool_use_id": TOOL_USE_ID,
            "tool_response": {"content": "#### R1.1 Hook Events (27 total)"},
        }
    )
    payloads["PostToolUseFailure"].update(
        {
            "tool_name": "Bash",
            "tool_input": {"command": "cargo test --workspace"},
            "tool_use_id": TOOL_USE_ID,
            "error": "command exited with status 101",
            "is_interrupt": False,
        }
    )
    payloads["Notification"].update(
        {
            "message": "Permission prompt waiting for user action",
            "title": "Claude Code notification",
            "notification_type": "permission_prompt",
        }
    )
    payloads["UserPromptSubmit"].update(
        {"prompt": "Collect fixture corpus coverage for all hook and transcript types."}
    )
    payloads["Stop"].update(
        {
            "stop_hook_active": False,
            "last_assistant_message": "Fixture generation plan is complete.",
        }
    )
    payloads["StopFailure"].update(
        {
            "error": "authentication_failed",
            "error_details": "Not logged in",
            "last_assistant_message": "Not logged in · Please run /login",
        }
    )
    payloads["SubagentStart"].update({"agent_id": AGENT_ID, "agent_type": "reviewer"})
    payloads["SubagentStop"].update(
        {
            "stop_hook_active": False,
            "agent_id": AGENT_ID,
            "agent_type": "reviewer",
            "agent_transcript_path": SUBAGENT_TRANSCRIPT_PATH,
            "last_assistant_message": "Subagent review complete.",
        }
    )
    payloads["PreCompact"].update(
        {
            "trigger": "auto",
            "custom_instructions": "Preserve tool lineage and transcript UUID links.",
        }
    )
    payloads["PostCompact"].update(
        {
            "trigger": "auto",
            "compact_summary": "Compacted prior turns into a short summary block.",
        }
    )
    payloads["PermissionRequest"].update(
        {
            "tool_name": "Bash",
            "tool_input": {"command": "git status --short"},
            "permission_suggestions": [
                {
                    "label": "Allow git status in this project",
                    "rule": "Bash(git status:*)",
                }
            ],
        }
    )
    payloads["PermissionDenied"].update(
        {
            "tool_name": "Bash",
            "tool_input": {"command": "rm -rf /tmp/build"},
            "tool_use_id": TOOL_USE_ID,
            "reason": "auto mode classifier denied destructive shell command",
        }
    )
    payloads["Setup"].update({"trigger": "project_init"})
    payloads["TeammateIdle"].update(
        {"teammate_name": "implementer", "team_name": "fixture-team"}
    )
    payloads["TaskCreated"].update(
        {
            "task_id": "task-mot-115-a",
            "task_subject": "Capture hook event fixtures",
            "task_description": "Assemble one JSON payload per hook event type.",
            "teammate_name": "implementer",
            "team_name": "fixture-team",
        }
    )
    payloads["TaskCompleted"].update(
        {
            "task_id": "task-mot-115-a",
            "task_subject": "Capture hook event fixtures",
            "task_description": "Assemble one JSON payload per hook event type.",
            "teammate_name": "implementer",
            "team_name": "fixture-team",
        }
    )
    payloads["Elicitation"].update(
        {
            "mcp_server_name": "fixture-memory",
            "message": "Please confirm the API token label to store.",
            "mode": "form",
            "elicitation_id": "elicit-mot-115",
            "requested_schema": {
                "type": "object",
                "properties": {"label": {"type": "string"}},
                "required": ["label"],
            },
        }
    )
    payloads["ElicitationResult"].update(
        {
            "mcp_server_name": "fixture-memory",
            "elicitation_id": "elicit-mot-115",
            "mode": "form",
            "action": "accept",
            "content": {"label": "fixture-token"},
        }
    )
    payloads["ConfigChange"].update(
        {
            "source": "project_settings",
            "file_path": f"{WORKSPACE_ROOT}/.claude/settings.json",
        }
    )
    payloads["WorktreeCreate"].update({"name": "mot-115-fixtures"})
    payloads["WorktreeRemove"].update({"worktree_path": "/workspace/worktrees/mot-115"})
    payloads["InstructionsLoaded"].update(
        {
            "file_path": f"{WORKSPACE_ROOT}/CLAUDE.md",
            "memory_type": "Project",
            "load_reason": "session_start",
            "trigger_file_path": f"{WORKSPACE_ROOT}/docs/DESIGN.md",
            "parent_file_path": f"{WORKSPACE_ROOT}/CLAUDE.md",
        }
    )
    payloads["CwdChanged"].update(
        {
            "old_cwd": WORKSPACE_ROOT,
            "new_cwd": f"{WORKSPACE_ROOT}/crates/types",
        }
    )
    payloads["FileChanged"].update(
        {
            "file_path": f"{WORKSPACE_ROOT}/docs/DESIGN.md",
            "event": "change",
        }
    )
    return payloads


def load_observed_hooks() -> dict[str, dict]:
    observed: dict[str, dict] = {}
    for path in sorted(OUT_DIR.glob("*.json")):
        event_name = path.stem.split("-")[-1]
        if event_name not in observed:
            observed[event_name] = json.loads(path.read_text())
    return observed


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def transcript_entry_base(entry_type: str, uuid: str, parent_uuid: str | None, timestamp: str) -> dict:
    return {
        "type": entry_type,
        "uuid": uuid,
        "parentUuid": parent_uuid,
        "isSidechain": False,
        "timestamp": timestamp,
        "sessionId": SESSION_ID,
        "cwd": WORKSPACE_ROOT,
        "version": VERSION,
        "gitBranch": BRANCH_NAME,
        "userType": "external",
        "entrypoint": "sdk-cli",
    }


def comprehensive_transcript() -> list[dict]:
    user = transcript_entry_base("user", "44444444-4444-4444-8444-444444444444", None, "2026-04-02T15:34:15.711Z")
    user.update(
        {
            "promptId": PROMPT_ID,
            "message": {
                "role": "user",
                "content": "Collect corpus fixtures for every hook event and transcript type.",
            },
        }
    )

    assistant = transcript_entry_base(
        "assistant",
        "55555555-5555-4555-8555-555555555555",
        user["uuid"],
        "2026-04-02T15:34:15.784Z",
    )
    assistant.update(
        {
            "requestId": REQUEST_ID,
            "message": {
                "id": "msg_mot115_fixture",
                "model": "claude-sonnet-4-20250514",
                "role": "assistant",
                "stop_reason": "tool_use",
                "usage": {
                    "input_tokens": 1200,
                    "output_tokens": 180,
                    "cache_creation_input_tokens": 256,
                    "cache_read_input_tokens": 512,
                    "cache_creation": {
                        "ephemeral_1h_input_tokens": 0,
                        "ephemeral_5m_input_tokens": 256,
                    },
                    "server_tool_use": {
                        "web_search_requests": 0,
                        "web_fetch_requests": 0,
                    },
                    "service_tier": "standard",
                    "inference_geo": "us",
                    "iterations": [{"attempt": 1}],
                    "speed": "standard",
                },
                "content": [
                    {"type": "thinking", "thinking": "Need hook and transcript fixtures.", "signature": "sig_mot115"},
                    {"type": "text", "text": "I am collecting the requested fixture corpus."},
                    {
                        "type": "tool_use",
                        "id": TOOL_USE_ID,
                        "name": "Read",
                        "input": {"file_path": f"{WORKSPACE_ROOT}/docs/DESIGN.md", "limit": 220},
                    },
                ],
            },
        }
    )

    system = transcript_entry_base(
        "system",
        "66666666-6666-4666-8666-666666666666",
        assistant["uuid"],
        "2026-04-02T15:34:16.000Z",
    )
    system.update(
        {
            "subtype": "tool_result",
            "durationMs": 118,
            "message": {
                "role": "system",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": TOOL_USE_ID,
                        "content": "#### R1.1 Hook Events (27 total)",
                        "is_error": False,
                    }
                ],
            },
        }
    )

    attachment = transcript_entry_base(
        "attachment",
        "77777777-7777-4777-8777-777777777777",
        user["uuid"],
        "2026-04-02T15:34:16.050Z",
    )
    attachment.update(
        {
            "message": {
                "role": "user",
                "content": [
                    {
                        "type": "image",
                        "source": {"type": "base64", "media_type": "image/png", "data": "fixture-image-data"},
                    }
                ],
            }
        }
    )

    return [
        {
            "type": "queue-operation",
            "operation": "enqueue",
            "timestamp": "2026-04-02T15:34:15.705Z",
            "sessionId": SESSION_ID,
            "content": "Collect corpus fixtures for every hook event and transcript type.",
        },
        {
            "type": "queue-operation",
            "operation": "dequeue",
            "timestamp": "2026-04-02T15:34:15.706Z",
            "sessionId": SESSION_ID,
        },
        user,
        assistant,
        system,
        attachment,
        {
            "type": "summary",
            "leafUuid": assistant["uuid"],
            "summary": "Collected hook and transcript coverage requirements for fixture generation.",
        },
        {"type": "custom-title", "sessionId": SESSION_ID, "customTitle": "Corpus Fixture Capture"},
        {"type": "ai-title", "sessionId": SESSION_ID, "aiTitle": "Hook and transcript corpus collection"},
        {
            "type": "last-prompt",
            "sessionId": SESSION_ID,
            "lastPrompt": "Collect corpus fixtures for every hook event and transcript type.",
        },
        {
            "type": "task-summary",
            "sessionId": SESSION_ID,
            "summary": "Building sanitized hook and transcript fixtures.",
            "timestamp": "2026-04-02T15:34:16.100Z",
        },
        {"type": "tag", "sessionId": SESSION_ID, "tag": "fixture-corpus"},
        {"type": "agent-name", "sessionId": SESSION_ID, "agentName": "Corpus Builder"},
        {"type": "agent-color", "sessionId": SESSION_ID, "agentColor": "#1f6feb"},
        {"type": "agent-setting", "sessionId": SESSION_ID, "agentSetting": "reviewer"},
        {
            "type": "pr-link",
            "sessionId": SESSION_ID,
            "prNumber": 115,
            "prUrl": "https://github.com/example/claude-insight/pull/115",
            "prRepository": "example/claude-insight",
            "timestamp": "2026-04-02T15:34:16.200Z",
        },
        {
            "type": "file-history-snapshot",
            "messageId": assistant["uuid"],
            "snapshot": {
                "trackedFileBackups": {
                    f"{WORKSPACE_ROOT}/tests/fixtures/hooks/PreToolUse.json": {
                        "path": f"{WORKSPACE_ROOT}/tests/fixtures/hooks/PreToolUse.json",
                        "backupPath": f"{WORKSPACE_ROOT}/.claude/backups/pretooluse.json",
                    }
                }
            },
            "isSnapshotUpdate": True,
        },
        {
            "type": "attribution-snapshot",
            "messageId": assistant["uuid"],
            "surface": "cli",
            "fileStates": {
                f"{WORKSPACE_ROOT}/tests/fixtures/transcripts/comprehensive.jsonl": {
                    "contentHash": "sha256-fixture",
                    "claudeContribution": 1842,
                    "mtime": 1775144056,
                }
            },
            "promptCount": 3,
            "promptCountAtLastCommit": 2,
            "permissionPromptCount": 1,
            "permissionPromptCountAtLastCommit": 1,
            "escapeCount": 0,
            "escapeCountAtLastCommit": 0,
        },
        {
            "type": "speculation-accept",
            "timestamp": "2026-04-02T15:34:16.250Z",
            "timeSavedMs": 432.5,
        },
        {"type": "mode", "sessionId": SESSION_ID, "mode": "normal"},
        {
            "type": "worktree-state",
            "sessionId": SESSION_ID,
            "worktreeSession": {
                "originalCwd": WORKSPACE_ROOT,
                "worktreePath": "/workspace/worktrees/mot-115",
                "worktreeName": "mot-115-fixtures",
                "worktreeBranch": BRANCH_NAME,
                "originalBranch": BRANCH_NAME,
                "originalHeadCommit": "fb28d7c",
                "sessionId": SESSION_ID,
                "tmuxSessionName": "mot-115-fixtures",
                "hookBased": False,
            },
        },
        {
            "type": "content-replacement",
            "sessionId": SESSION_ID,
            "agentId": AGENT_ID,
            "replacements": [
                {
                    "kind": "tool-result",
                    "toolUseId": TOOL_USE_ID,
                    "replacement": "[tool result stored externally]",
                }
            ],
        },
        {
            "type": "marble-origami-commit",
            "sessionId": SESSION_ID,
            "collapseId": "0000000000000115",
            "summaryUuid": "88888888-8888-4888-8888-888888888888",
            "summaryContent": "<collapsed id=\"0000000000000115\">Fixture corpus summary</collapsed>",
            "summary": "Fixture corpus summary",
            "firstArchivedUuid": user["uuid"],
            "lastArchivedUuid": system["uuid"],
        },
        {
            "type": "marble-origami-snapshot",
            "sessionId": SESSION_ID,
            "staged": [
                {
                    "startUuid": user["uuid"],
                    "endUuid": system["uuid"],
                    "summary": "Pending fixture corpus context collapse",
                    "risk": 0.1,
                    "stagedAt": 1775144056,
                }
            ],
            "armed": True,
            "lastSpawnTokens": 2048,
        },
    ]


def auth_failure_transcript() -> list[dict]:
    return [
        {
            "type": "queue-operation",
            "operation": "enqueue",
            "timestamp": "2026-04-02T15:34:15.705Z",
            "sessionId": SESSION_ID,
            "content": "Reply with exactly: probe ok",
        },
        {
            "type": "queue-operation",
            "operation": "dequeue",
            "timestamp": "2026-04-02T15:34:15.705Z",
            "sessionId": SESSION_ID,
        },
        {
            "parentUuid": None,
            "isSidechain": False,
            "type": "progress",
            "data": {
                "type": "hook_progress",
                "hookEvent": "SessionStart",
                "hookName": "SessionStart:startup",
                "command": (
                    "node \"$CLAUDE_PROJECT_DIR\"/.research/claude-hook-probe/capture-hook.mjs "
                    "SessionStart \"$CLAUDE_PROJECT_DIR\"/.research/claude-hook-probe/out"
                ),
            },
            "parentToolUseID": "99999999-9999-4999-8999-999999999999",
            "toolUseID": "99999999-9999-4999-8999-999999999999",
            "timestamp": "2026-04-02T15:34:15.617Z",
            "uuid": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "userType": "external",
            "entrypoint": "sdk-cli",
            "cwd": WORKSPACE_ROOT,
            "sessionId": SESSION_ID,
            "version": VERSION,
            "gitBranch": BRANCH_NAME,
        },
        {
            "parentUuid": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "isSidechain": False,
            "promptId": PROMPT_ID,
            "type": "user",
            "message": {"role": "user", "content": "Reply with exactly: probe ok"},
            "uuid": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "timestamp": "2026-04-02T15:34:15.711Z",
            "permissionMode": "acceptEdits",
            "userType": "external",
            "entrypoint": "sdk-cli",
            "cwd": WORKSPACE_ROOT,
            "sessionId": SESSION_ID,
            "version": VERSION,
            "gitBranch": BRANCH_NAME,
        },
        {
            "parentUuid": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "isSidechain": False,
            "type": "assistant",
            "uuid": "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
            "timestamp": "2026-04-02T15:34:15.784Z",
            "message": {
                "id": "assistant-auth-failure",
                "model": "<synthetic>",
                "role": "assistant",
                "stop_reason": "stop_sequence",
                "stop_sequence": "",
                "type": "message",
                "usage": {
                    "input_tokens": 0,
                    "output_tokens": 0,
                    "cache_creation_input_tokens": 0,
                    "cache_read_input_tokens": 0,
                    "server_tool_use": {"web_search_requests": 0, "web_fetch_requests": 0},
                    "service_tier": "standard",
                    "cache_creation": {
                        "ephemeral_1h_input_tokens": 0,
                        "ephemeral_5m_input_tokens": 0,
                    },
                    "inference_geo": "",
                    "iterations": [],
                    "speed": "standard",
                },
                "content": [{"type": "text", "text": "Not logged in · Please run /login"}],
                "context_management": None,
            },
            "error": "authentication_failed",
            "isApiErrorMessage": True,
            "userType": "external",
            "entrypoint": "sdk-cli",
            "cwd": WORKSPACE_ROOT,
            "sessionId": SESSION_ID,
            "version": VERSION,
            "gitBranch": BRANCH_NAME,
        },
        {
            "type": "last-prompt",
            "lastPrompt": "Reply with exactly: probe ok",
            "sessionId": SESSION_ID,
        },
    ]


def write_jsonl(path: Path, lines: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w") as handle:
        for entry in lines:
            handle.write(json.dumps(sanitize(entry), sort_keys=True))
            handle.write("\n")


def main() -> None:
    HOOKS_DIR.mkdir(parents=True, exist_ok=True)
    TRANSCRIPTS_DIR.mkdir(parents=True, exist_ok=True)
    SETTINGS_DIR.mkdir(parents=True, exist_ok=True)

    observed_hooks = {name: sanitize(payload) for name, payload in load_observed_hooks().items()}
    documented_hooks = documented_hook_payloads()

    for event_name in HOOK_EVENTS:
        payload = deepcopy(observed_hooks.get(event_name, documented_hooks[event_name]))
        payload["hook_event_name"] = event_name
        write_json(HOOKS_DIR / f"{event_name}.json", sanitize(payload))

    settings_payload = json.loads((REPO_ROOT / ".research" / "claude-hook-probe" / "settings.json").read_text())
    write_json(SETTINGS_DIR / "settings.json", sanitize(settings_payload))
    write_json(
        SETTINGS_DIR / ".mcp.json",
        {
            "mcpServers": {
                "fixture-demo": {
                    "command": "node",
                    "args": ["./tests/fixtures/settings/mock-mcp-server.js"],
                    "env": {"FIXTURE_MODE": "true"},
                }
            }
        },
    )

    write_jsonl(TRANSCRIPTS_DIR / "auth-failure.observed.jsonl", auth_failure_transcript())
    write_jsonl(TRANSCRIPTS_DIR / "comprehensive.jsonl", comprehensive_transcript())


if __name__ == "__main__":
    main()
