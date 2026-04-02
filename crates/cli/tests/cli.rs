use std::{
    fs, io,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use claude_insight_storage::Database;
use serde_json::Value;

const BIN_PATH: &str = env!("CARGO_BIN_EXE_claude-insight");

struct TestEnv {
    root: PathBuf,
}

impl TestEnv {
    fn new() -> io::Result<Self> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "claude-insight-cli-tests-{}-{}",
            std::process::id(),
            unique
        ));
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn app_home(&self) -> &Path {
        &self.root
    }

    fn database_path(&self) -> PathBuf {
        self.app_home().join(".claude-insight").join("insight.db")
    }

    fn database(&self) -> Result<Database, Box<dyn std::error::Error>> {
        Ok(Database::new(self.database_path())?)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(BIN_PATH);
        command.env("CLAUDE_INSIGHT_HOME", self.app_home());
        command.env("HOME", self.app_home());
        command
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn reserve_capture_port() -> io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn daemon_responds(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(200),
    )
    .is_ok()
}

fn read_settings(path: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn event_has_insight_hook(settings: &Value, event_name: &str, port: u16) -> bool {
    let expected_url = format!("http://127.0.0.1:{port}/hooks");
    settings["hooks"][event_name]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter_map(|entry| entry.get("hooks"))
        .filter_map(Value::as_array)
        .flat_map(|hooks| hooks.iter())
        .filter_map(Value::as_object)
        .any(|hook| {
            hook.get("type").and_then(Value::as_str) == Some("http")
                && hook.get("url").and_then(Value::as_str) == Some(expected_url.as_str())
        })
}

#[test]
fn trace_without_session_lists_recent_sessions() -> Result<(), Box<dyn std::error::Error>> {
    let env = TestEnv::new()?;
    let database = env.database()?;

    database.insert_raw_event(
        "session-1",
        "hook",
        "SessionStart",
        "2026-04-03T15:00:00Z",
        &serde_json::json!({ "source": "startup" }).to_string(),
    )?;

    let output = env.command().arg("trace").output()?;

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("Recent sessions"));
    assert!(stdout.contains("session-1"));

    Ok(())
}

#[test]
fn search_returns_matching_events() -> Result<(), Box<dyn std::error::Error>> {
    let env = TestEnv::new()?;
    let database = env.database()?;

    database.insert_raw_event(
        "session-1",
        "hook",
        "PreToolUse",
        "2026-04-03T15:00:00Z",
        &serde_json::json!({
            "tool_name": "Bash",
            "tool_input": { "command": "pwd" },
        })
        .to_string(),
    )?;

    let output = env.command().args(["search", "Bash"]).output()?;

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("Search"));
    assert!(stdout.contains("Bash"));
    assert!(stdout.contains("session-1"));

    Ok(())
}

#[test]
fn gc_prunes_old_events() -> Result<(), Box<dyn std::error::Error>> {
    let env = TestEnv::new()?;
    let database = env.database()?;

    database.insert_raw_event(
        "session-old",
        "hook",
        "Notification",
        "2020-01-01T00:00:00Z",
        &serde_json::json!({ "message": "old" }).to_string(),
    )?;
    database.insert_raw_event(
        "session-new",
        "hook",
        "Notification",
        "2099-01-01T00:00:00Z",
        &serde_json::json!({ "message": "new" }).to_string(),
    )?;

    let output = env.command().args(["gc", "--days", "90"]).output()?;
    let database = env.database()?;
    let remaining = database.query_raw_events_by_event_type("Notification")?;

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].session_id.as_deref(), Some("session-new"));

    Ok(())
}

#[test]
fn normalize_materializes_sessions_table() -> Result<(), Box<dyn std::error::Error>> {
    let env = TestEnv::new()?;
    let database = env.database()?;

    database.insert_raw_event(
        "session-1",
        "hook",
        "SessionStart",
        "2026-04-03T15:00:00Z",
        &serde_json::json!({
            "hook_event_name": "SessionStart",
            "session_id": "session-1",
            "source": "startup",
            "transcript_path": "/tmp/session-1/transcript.jsonl",
            "cwd": "/workspace/claude-insight",
            "transcript_path": "/workspace/.claude/projects/claude-insight/session-1.jsonl",
        })
        .to_string(),
    )?;

    let output = env.command().arg("normalize").output()?;
    let database = env.database()?;
    let session_exists = database.normalized_session_exists("session-1")?;

    assert!(output.status.success());
    assert!(session_exists);

    Ok(())
}

#[test]
fn help_lists_new_commands() -> Result<(), Box<dyn std::error::Error>> {
    let env = TestEnv::new()?;

    let root_help = env.command().arg("--help").output()?;
    let daemon_help = env.command().args(["daemon", "--help"]).output()?;
    let init_help = env.command().args(["init", "--help"]).output()?;
    let normalize_help = env.command().args(["normalize", "--help"]).output()?;

    assert!(root_help.status.success());
    assert!(daemon_help.status.success());
    assert!(init_help.status.success());
    assert!(normalize_help.status.success());

    let root_stdout = String::from_utf8(root_help.stdout)?;
    assert!(root_stdout.contains("Local observability for Claude Code"));
    assert!(root_stdout.contains("trace"));
    assert!(root_stdout.contains("search"));
    assert!(root_stdout.contains("gc"));
    assert!(root_stdout.contains("normalize"));
    assert!(root_stdout.contains("daemon"));

    let daemon_stdout = String::from_utf8(daemon_help.stdout)?;
    assert!(daemon_stdout.contains("start"));
    assert!(daemon_stdout.contains("stop"));

    let init_stdout = String::from_utf8(init_help.stdout)?;
    assert!(init_stdout.contains("--global"));
    assert!(init_stdout.contains("--capture-content"));

    Ok(())
}

#[test]
fn init_installs_project_hooks_and_starts_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let env = TestEnv::new()?;
    let project_dir = env.app_home().join("project");
    fs::create_dir_all(&project_dir)?;
    let capture_port = reserve_capture_port()?;

    let output = env
        .command()
        .current_dir(&project_dir)
        .env("CLAUDE_INSIGHT_CAPTURE_PORT", capture_port.to_string())
        .arg("init")
        .output()?;

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let settings_path = project_dir.join(".claude").join("settings.json");
    let pid_file = env.app_home().join(".claude-insight").join("daemon.pid");
    let settings = read_settings(&settings_path)?;
    let stdout = String::from_utf8(output.stdout)?;

    assert!(settings_path.exists());
    assert_eq!(
        settings["hooks"].as_object().map(|hooks| hooks.len()),
        Some(27)
    );
    assert!(event_has_insight_hook(
        &settings,
        "SessionStart",
        capture_port
    ));
    assert!(event_has_insight_hook(
        &settings,
        "PostToolUse",
        capture_port
    ));
    assert!(pid_file.exists());
    assert!(daemon_responds(capture_port));
    assert!(stdout.contains("settings:"));
    assert!(stdout.contains("hooks:"));
    assert!(stdout.contains("status=started"));
    assert!(stdout.contains(&format!("port={capture_port}")));

    let stop_output = env
        .command()
        .env("CLAUDE_INSIGHT_CAPTURE_PORT", capture_port.to_string())
        .args(["daemon", "stop"])
        .output()?;
    assert!(stop_output.status.success());

    Ok(())
}

#[test]
fn init_global_preserves_existing_hooks_and_is_idempotent() -> Result<(), Box<dyn std::error::Error>>
{
    let env = TestEnv::new()?;
    let capture_port = reserve_capture_port()?;
    let settings_dir = env.app_home().join(".claude");
    let settings_path = settings_dir.join("settings.json");
    fs::create_dir_all(&settings_dir)?;
    fs::write(
        &settings_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "hooks": {
                "Notification": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": "echo keep-me",
                                "timeout": 30
                            }
                        ]
                    }
                ]
            }
        }))?,
    )?;

    let first_output = env
        .command()
        .env("CLAUDE_INSIGHT_CAPTURE_PORT", capture_port.to_string())
        .args(["init", "--global", "--capture-content"])
        .output()?;
    assert!(
        first_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first_output.stdout),
        String::from_utf8_lossy(&first_output.stderr)
    );

    let settings = read_settings(&settings_path)?;
    let notification_entries = settings["hooks"]["Notification"]
        .as_array()
        .ok_or("Notification hooks should be an array")?;

    assert_eq!(settings["capture_content"].as_bool(), Some(true));
    assert_eq!(notification_entries.len(), 2);
    assert_eq!(
        notification_entries[0]["hooks"][0]["command"].as_str(),
        Some("echo keep-me")
    );
    assert!(event_has_insight_hook(
        &settings,
        "Notification",
        capture_port
    ));
    assert!(event_has_insight_hook(
        &settings,
        "TaskCreated",
        capture_port
    ));

    let second_output = env
        .command()
        .env("CLAUDE_INSIGHT_CAPTURE_PORT", capture_port.to_string())
        .args(["init", "--global", "--capture-content"])
        .output()?;
    assert!(second_output.status.success());

    let settings = read_settings(&settings_path)?;
    let notification_entries = settings["hooks"]["Notification"]
        .as_array()
        .ok_or("Notification hooks should stay an array")?;
    assert_eq!(notification_entries.len(), 2);

    let stop_output = env
        .command()
        .env("CLAUDE_INSIGHT_CAPTURE_PORT", capture_port.to_string())
        .args(["daemon", "stop"])
        .output()?;
    assert!(stop_output.status.success());

    Ok(())
}

#[test]
fn init_prints_first_run_banner() -> Result<(), Box<dyn std::error::Error>> {
    let env = TestEnv::new()?;

    let output = env.command().arg("init").output()?;

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("Local observability for Claude Code"));
    assert!(stdout.contains("Initialized"));

    Ok(())
}

#[test]
fn daemon_start_and_stop_manage_pid_file() -> Result<(), Box<dyn std::error::Error>> {
    let env = TestEnv::new()?;
    let pid_file = env.app_home().join(".claude-insight").join("daemon.pid");
    let capture_port = reserve_capture_port()?;

    let start_output = env
        .command()
        .env("CLAUDE_INSIGHT_CAPTURE_PORT", capture_port.to_string())
        .args(["daemon", "start"])
        .output()?;
    assert!(
        start_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&start_output.stdout),
        String::from_utf8_lossy(&start_output.stderr)
    );
    assert!(pid_file.exists());
    assert!(daemon_responds(capture_port));

    let stop_output = env
        .command()
        .env("CLAUDE_INSIGHT_CAPTURE_PORT", capture_port.to_string())
        .args(["daemon", "stop"])
        .output()?;
    assert!(
        stop_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&stop_output.stdout),
        String::from_utf8_lossy(&stop_output.stderr)
    );
    assert!(!pid_file.exists());

    Ok(())
}
