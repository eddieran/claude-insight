#![deny(clippy::expect_used, clippy::unwrap_used)]

use claude_insight_storage::{Database, RawEvent};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::{
    error::Error,
    fs, io,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tempfile::TempDir;

const CLAUDE_BIN_ENV: &str = "CLAUDE_INSIGHT_CLAUDE_BIN";
const REAL_E2E_ENV: &str = "CLAUDE_INSIGHT_RUN_E2E";
const REPO_ROOT: &str = env!("CARGO_MANIFEST_DIR");
const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(30);
const EVENT_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const FILE_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[test]
fn e2e_simple_prompt_captures_traceable_session() -> Result<(), Box<dyn Error>> {
    if let Some(reason) = skip_reason()? {
        eprintln!("{reason}");
        return Ok(());
    }

    let env = E2eEnv::new("simple-prompt")?;
    env.init()?;

    let session_id = unique_session_id();
    let prompt = "For claude-insight e2e simple prompt, reply with exactly FOUR.";
    let _output = env.run_claude_prompt(&session_id, prompt, &[])?;

    let events = env.wait_for_events(&session_id, |events| {
        has_event_type(events, "SessionStart")
            && has_event_type(events, "UserPromptSubmit")
            && has_terminal_event(events)
            && has_transcript_event(events)
    })?;

    assert!(events.len() >= 3, "expected at least three raw events");

    let trace_output = env.run_insight(&["trace", session_id.as_str()])?;
    let trace_stdout = String::from_utf8(trace_output.stdout)?;
    assert!(trace_stdout.contains("Trace"));
    assert!(trace_stdout.contains(session_id.as_str()));
    assert!(trace_stdout.contains("SessionStart"));

    let _normalize_output = env.run_insight(&["normalize"])?;
    env.wait_for_row_count(
        "SELECT COUNT(*) FROM sessions WHERE id = ?1",
        params![session_id.as_str()],
        1,
    )?;

    let connection = Connection::open(env.database_path())?;
    let cwd: String = connection.query_row(
        "SELECT cwd
             FROM sessions
             WHERE id = ?1",
        params![session_id.as_str()],
        |row| row.get(0),
    )?;
    assert_eq!(
        fs::canonicalize(PathBuf::from(cwd))?,
        fs::canonicalize(env.workspace())?
    );

    Ok(())
}

#[test]
fn e2e_tool_usage_populates_search_and_normalized_tool_rows() -> Result<(), Box<dyn Error>> {
    if let Some(reason) = skip_reason()? {
        eprintln!("{reason}");
        return Ok(());
    }

    let env = E2eEnv::new("tool-usage")?;
    fs::write(
        env.workspace().join("tool-fixture.txt"),
        "alpha beta gamma\n",
    )?;
    env.init()?;

    let session_id = unique_session_id();
    let prompt =
        "Use the Read tool to inspect tool-fixture.txt and answer with its first word only.";
    let _output = env.run_claude_prompt(&session_id, prompt, &["--allowedTools", "Read"])?;

    let events = env.wait_for_events(&session_id, |events| {
        has_event_type(events, "PreToolUse") && has_event_type(events, "PostToolUse")
    })?;

    assert!(
        events
            .iter()
            .filter(|event| event.event_type == "PreToolUse")
            .any(|event| event.tool_use_id.is_some()),
        "expected a PreToolUse event with tool_use_id"
    );

    let search_output = env.run_insight(&["search", "Read"])?;
    let search_stdout = String::from_utf8(search_output.stdout)?;
    assert!(search_stdout.contains("Search"));
    assert!(search_stdout.contains(session_id.as_str()));
    assert!(search_stdout.contains("Read"));

    let _normalize_output = env.run_insight(&["normalize"])?;
    env.wait_for_row_count(
        "SELECT COUNT(*) FROM tool_invocations WHERE session_id = ?1",
        params![session_id.as_str()],
        1,
    )?;

    Ok(())
}

#[test]
fn e2e_file_edit_loads_project_instructions_and_correlates_transcript() -> Result<(), Box<dyn Error>>
{
    if let Some(reason) = skip_reason()? {
        eprintln!("{reason}");
        return Ok(());
    }

    let env = E2eEnv::new("file-edit")?;
    fs::write(
        env.workspace().join("CLAUDE.md"),
        "When asked to create e2e-note.txt, write exactly `note-from-claude-md`.\n",
    )?;
    env.init()?;

    let session_id = unique_session_id();
    let prompt = "Use the Read and Write tools. Read CLAUDE.md, then create e2e-note.txt with the exact content requested there.";
    let _output = env.run_claude_prompt(&session_id, prompt, &["--allowedTools", "Read,Write"])?;

    let note_path = env.workspace().join("e2e-note.txt");
    wait_for_file(&note_path, FILE_WAIT_TIMEOUT)?;
    assert_eq!(
        fs::read_to_string(&note_path)?.trim(),
        "note-from-claude-md"
    );

    let events = env.wait_for_events(&session_id, |events| {
        has_event_type(events, "InstructionsLoaded")
            && has_event_type(events, "PreToolUse")
            && has_event_type(events, "PostToolUse")
            && has_transcript_event(events)
    })?;
    assert!(
        events.iter().any(|event| event.source == "transcript"),
        "expected transcript events to be ingested for correlation"
    );

    env.wait_for_event_links(&session_id)?;

    let _normalize_output = env.run_insight(&["normalize"])?;
    env.wait_for_row_count(
        "SELECT COUNT(*) FROM instruction_loads WHERE session_id = ?1",
        params![session_id.as_str()],
        1,
    )?;

    Ok(())
}

struct E2eEnv {
    root: TempDir,
    workspace: PathBuf,
    capture_port: u16,
    real_home: PathBuf,
}

impl E2eEnv {
    fn new(test_name: &str) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::Builder::new()
            .prefix("claude-insight-e2e-")
            .tempdir()?;
        let workspace = root.path().join(test_name);
        fs::create_dir_all(&workspace)?;
        let real_home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other("HOME must be set for Claude auth"))?;

        link_transcript_root(root.path(), &real_home)?;

        Ok(Self {
            root,
            workspace,
            capture_port: reserve_capture_port()?,
            real_home,
        })
    }

    fn workspace(&self) -> &Path {
        &self.workspace
    }

    fn app_home(&self) -> &Path {
        self.root.path()
    }

    fn database_path(&self) -> PathBuf {
        self.app_home().join(".claude-insight").join("insight.db")
    }

    fn insight_command(&self) -> Command {
        let mut command = Command::new(cli_binary_path());
        command.current_dir(self.workspace());
        command.env("CLAUDE_INSIGHT_CAPTURE_PORT", self.capture_port.to_string());
        command.env("CLAUDE_INSIGHT_HOME", self.app_home());
        command.env("HOME", &self.real_home);
        command
    }

    fn claude_command(&self) -> Command {
        let binary = std::env::var(CLAUDE_BIN_ENV).unwrap_or_else(|_| "claude".to_string());
        let mut command = Command::new(binary);
        command.current_dir(self.workspace());
        command.env("HOME", &self.real_home);
        command.env("NO_COLOR", "1");
        command
    }

    fn init(&self) -> Result<(), Box<dyn Error>> {
        ensure_cli_binary()?;
        let output = self
            .insight_command()
            .args(["init", "--capture-content"])
            .output()?;
        let _checked = ensure_success(output, "claude-insight init")?;
        wait_for_daemon(self.capture_port)?;
        Ok(())
    }

    fn run_insight(&self, args: &[&str]) -> Result<Output, Box<dyn Error>> {
        let output = self.insight_command().args(args).output()?;
        ensure_success(output, "claude-insight command")
    }

    fn run_claude_prompt(
        &self,
        session_id: &str,
        prompt: &str,
        extra_args: &[&str],
    ) -> Result<Output, Box<dyn Error>> {
        let mut command = self.claude_command();
        command.args([
            "-p",
            "--output-format",
            "json",
            "--session-id",
            session_id,
            "--permission-mode",
            "bypassPermissions",
        ]);
        command.args(extra_args);
        command.arg("--");
        command.arg(prompt);

        let output = command.output()?;
        ensure_success(output, "claude -p")
    }

    fn wait_for_events<F>(
        &self,
        session_id: &str,
        predicate: F,
    ) -> Result<Vec<RawEvent>, Box<dyn Error>>
    where
        F: Fn(&[RawEvent]) -> bool,
    {
        let started_at = Instant::now();

        while started_at.elapsed() < EVENT_WAIT_TIMEOUT {
            let database = Database::new(self.database_path())?;
            let events = database.query_raw_events_by_session(session_id)?;

            if predicate(&events) {
                return Ok(events);
            }

            thread::sleep(POLL_INTERVAL);
        }

        Err(io::Error::other(format!(
            "timed out waiting for raw events for session {session_id}"
        ))
        .into())
    }

    fn wait_for_row_count<P>(
        &self,
        query: &str,
        params: P,
        minimum_count: i64,
    ) -> Result<(), Box<dyn Error>>
    where
        P: rusqlite::Params + Clone,
    {
        let started_at = Instant::now();

        while started_at.elapsed() < EVENT_WAIT_TIMEOUT {
            let connection = Connection::open(self.database_path())?;
            let count: i64 = connection.query_row(query, params.clone(), |row| row.get(0))?;

            if count >= minimum_count {
                return Ok(());
            }

            thread::sleep(POLL_INTERVAL);
        }

        Err(io::Error::other(format!(
            "timed out waiting for query to reach count {minimum_count}"
        ))
        .into())
    }

    fn wait_for_event_links(&self, session_id: &str) -> Result<(), Box<dyn Error>> {
        let started_at = Instant::now();

        while started_at.elapsed() < EVENT_WAIT_TIMEOUT {
            let database = Database::new(self.database_path())?;
            let _stats = database.correlate_session(session_id)?;
            let links = database.query_event_links_by_session(session_id)?;

            if !links.is_empty() {
                return Ok(());
            }

            thread::sleep(POLL_INTERVAL);
        }

        Err(io::Error::other(format!(
            "timed out waiting for correlated event links for session {session_id}"
        ))
        .into())
    }

    fn best_effort_stop_daemon(&self) {
        let _ = self.insight_command().args(["daemon", "stop"]).output();
    }
}

impl Drop for E2eEnv {
    fn drop(&mut self) {
        self.best_effort_stop_daemon();
    }
}

fn skip_reason() -> Result<Option<String>, Box<dyn Error>> {
    if !real_e2e_enabled() {
        return Ok(Some(format!(
            "skipping real Claude E2E; set {REAL_E2E_ENV}=1 to enable authenticated claude -p runs"
        )));
    }

    let binary = std::env::var(CLAUDE_BIN_ENV).unwrap_or_else(|_| "claude".to_string());
    let version_status = Command::new(&binary).arg("--version").status()?;
    if !version_status.success() {
        return Err(io::Error::other(format!(
            "{binary} --version exited with status {version_status}"
        ))
        .into());
    }

    let auth_output = Command::new(&binary).args(["auth", "status"]).output()?;
    let auth_output = ensure_success(auth_output, "claude auth status")?;
    let auth_status: Value = serde_json::from_slice(&auth_output.stdout)?;
    if auth_status["loggedIn"].as_bool() != Some(true) {
        return Ok(Some(
            "skipping real Claude E2E; `claude auth status` reports logged out".to_string(),
        ));
    }

    Ok(None)
}

fn real_e2e_enabled() -> bool {
    matches!(
        std::env::var(REAL_E2E_ENV).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn unique_session_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    format!("00000000-0000-4000-8000-{:012x}", nanos & 0xffffffffffff)
}

fn reserve_capture_port() -> io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn wait_for_daemon(port: u16) -> Result<(), Box<dyn Error>> {
    let started_at = Instant::now();
    let capture_addr = SocketAddr::from(([127, 0, 0, 1], port));

    while started_at.elapsed() < DAEMON_READY_TIMEOUT {
        if TcpStream::connect_timeout(&capture_addr, POLL_INTERVAL).is_ok() {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }

    Err(io::Error::other(format!(
        "timed out waiting for daemon to accept connections on {capture_addr}"
    ))
    .into())
}

fn wait_for_file(path: &Path, timeout: Duration) -> Result<(), Box<dyn Error>> {
    let started_at = Instant::now();

    while started_at.elapsed() < timeout {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }

    Err(io::Error::other(format!("timed out waiting for file {}", path.display())).into())
}

#[cfg(unix)]
fn link_transcript_root(app_home: &Path, real_home: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let claude_dir = app_home.join(".claude");
    fs::create_dir_all(&claude_dir)?;
    symlink(
        real_home.join(".claude").join("projects"),
        claude_dir.join("projects"),
    )?;

    Ok(())
}

#[cfg(not(unix))]
fn link_transcript_root(_app_home: &Path, _real_home: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}

fn ensure_success(output: Output, context: &str) -> Result<Output, Box<dyn Error>> {
    if output.status.success() {
        return Ok(output);
    }

    Err(io::Error::other(format!(
        "{context} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
    .into())
}

fn cli_binary_path() -> PathBuf {
    let executable_name = if cfg!(windows) {
        "claude-insight.exe"
    } else {
        "claude-insight"
    };

    Path::new(REPO_ROOT)
        .join("target")
        .join("debug")
        .join(executable_name)
}

fn ensure_cli_binary() -> Result<(), Box<dyn Error>> {
    let binary_path = cli_binary_path();
    if binary_path.exists() {
        return Ok(());
    }

    let output = Command::new("cargo")
        .current_dir(REPO_ROOT)
        .args(["build", "-q", "--manifest-path"])
        .arg(Path::new(REPO_ROOT).join("Cargo.toml"))
        .args(["-p", "cli"])
        .output()?;
    let _checked = ensure_success(output, "cargo build -p cli")?;

    if !binary_path.exists() {
        return Err(io::Error::other(format!(
            "expected CLI binary at {} after build",
            binary_path.display()
        ))
        .into());
    }

    Ok(())
}

fn has_event_type(events: &[RawEvent], event_type: &str) -> bool {
    events.iter().any(|event| event.event_type == event_type)
}

fn has_terminal_event(events: &[RawEvent]) -> bool {
    ["SessionEnd", "Stop", "StopFailure"]
        .iter()
        .any(|event_type| has_event_type(events, event_type))
}

fn has_transcript_event(events: &[RawEvent]) -> bool {
    events
        .iter()
        .any(|event| event.source == "transcript" || event.event_type.starts_with("Transcript"))
}
