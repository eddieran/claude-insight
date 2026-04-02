use std::{
    fs, io,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use claude_insight_storage::Database;

const BIN_PATH: &str = env!("CARGO_BIN_EXE_claude-insight");

struct TestEnv {
    root: PathBuf,
    capture_port: u16,
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
        Ok(Self {
            root,
            capture_port: next_available_port()?,
        })
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
        command
            .env("CLAUDE_INSIGHT_HOME", self.app_home())
            .env("CLAUDE_INSIGHT_CAPTURE_PORT", self.capture_port.to_string());
        command
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn next_available_port() -> io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
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
fn normalize_rebuild_flag_replays_raw_events() -> Result<(), Box<dyn std::error::Error>> {
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
            "cwd": "/workspace/claude-insight",
            "transcript_path": "/workspace/.claude/projects/claude-insight/session-1.jsonl",
        })
        .to_string(),
    )?;

    let output = env.command().args(["normalize", "--rebuild"]).output()?;
    let database = env.database()?;
    let session_exists = database.normalized_session_exists("session-1")?;

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(session_exists);
    assert!(String::from_utf8(output.stdout)?.contains("Rebuilt"));

    Ok(())
}

#[test]
fn help_lists_new_commands() -> Result<(), Box<dyn std::error::Error>> {
    let env = TestEnv::new()?;

    let root_help = env.command().arg("--help").output()?;
    let daemon_help = env.command().args(["daemon", "--help"]).output()?;
    let normalize_help = env.command().args(["normalize", "--help"]).output()?;

    assert!(root_help.status.success());
    assert!(daemon_help.status.success());
    assert!(normalize_help.status.success());

    let root_stdout = String::from_utf8(root_help.stdout)?;
    assert!(root_stdout.contains("trace"));
    assert!(root_stdout.contains("search"));
    assert!(root_stdout.contains("gc"));
    assert!(root_stdout.contains("normalize"));
    assert!(root_stdout.contains("daemon"));

    let daemon_stdout = String::from_utf8(daemon_help.stdout)?;
    assert!(daemon_stdout.contains("start"));
    assert!(daemon_stdout.contains("stop"));

    Ok(())
}

#[test]
fn daemon_start_and_stop_manage_pid_file() -> Result<(), Box<dyn std::error::Error>> {
    let env = TestEnv::new()?;
    let pid_file = env.app_home().join(".claude-insight").join("daemon.pid");

    let start_output = env.command().args(["daemon", "start"]).output()?;
    assert!(
        start_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&start_output.stdout),
        String::from_utf8_lossy(&start_output.stderr)
    );
    assert!(pid_file.exists());

    let stop_output = env.command().args(["daemon", "stop"]).output()?;
    assert!(
        stop_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&stop_output.stdout),
        String::from_utf8_lossy(&stop_output.stderr)
    );
    assert!(!pid_file.exists());

    Ok(())
}
