#![deny(clippy::expect_used, clippy::unwrap_used)]

use claude_insight_storage::Database;
use rusqlite::{params, Connection};
use std::{
    error::Error,
    fs::{self, File},
    io::{self, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{Mutex, MutexGuard, OnceLock},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(windows)]
use std::os::windows::fs::symlink_dir;

const CLAUDE_BIN: &str = "claude";
const CLAUDE_TIMEOUT: Duration = Duration::from_secs(180);
const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(20);
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const CLI_BINARY_RELATIVE_PATH: &str = "target/debug/claude-insight";
const PROJECT_CLAUDE_MD: &str = r#"# E2E Project Memory

- Use file tools when the prompt explicitly asks for them.
- Keep answers short.
"#;

static TEST_MUTEX: Mutex<()> = Mutex::new(());
static CLI_BINARY_ONCE: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug)]
struct RawEventRecord {
    event_type: String,
    tool_use_id: Option<String>,
}

struct E2eHarness {
    _guard: MutexGuard<'static, ()>,
    session_id: String,
    root_dir: PathBuf,
    project_dir: PathBuf,
    plugin_dir: PathBuf,
    app_home: PathBuf,
    db_path: PathBuf,
    backlog_path: PathBuf,
    capture_addr: SocketAddr,
    daemon: Option<Child>,
}

impl E2eHarness {
    fn new(test_name: &str) -> Result<Self, Box<dyn Error>> {
        let guard = match TEST_MUTEX.lock() {
            Ok(guard) => guard,
            // Keep tests serialized even after an earlier assertion panic so later
            // failures remain actionable when the default test runner keeps going.
            Err(poisoned) => poisoned.into_inner(),
        };
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root_dir = repo_root
            .join("target")
            .join("e2e")
            .join(unique_name(test_name));
        let project_dir = root_dir.join("project");
        let plugin_dir = root_dir.join("hook-plugin");
        let app_home = root_dir.join("app-home");
        let db_path = app_home.join(".claude-insight").join("insight.db");
        let backlog_path = app_home.join(".claude-insight").join("backlog.jsonl");
        let capture_port = reserve_capture_port()?;
        let capture_addr = SocketAddr::from(([127, 0, 0, 1], capture_port));

        fs::create_dir_all(&project_dir)?;
        fs::create_dir_all(&plugin_dir)?;
        fs::create_dir_all(app_home.join(".claude"))?;
        fs::create_dir_all(app_home.join(".claude-insight"))?;

        let transcript_root = actual_transcript_root()?;
        let transcript_symlink = app_home.join(".claude").join("projects");
        create_projects_symlink(&transcript_root, &transcript_symlink)?;

        fs::write(project_dir.join("CLAUDE.md"), PROJECT_CLAUDE_MD)?;
        fs::write(project_dir.join("sample.txt"), "smoke target\n")?;
        initialize_git_repo(&project_dir)?;
        write_plugin(&plugin_dir)?;

        Ok(Self {
            _guard: guard,
            session_id: unique_session_id(),
            root_dir,
            project_dir,
            plugin_dir,
            app_home,
            db_path,
            backlog_path,
            capture_addr,
            daemon: None,
        })
    }

    fn start_daemon(&mut self) -> Result<(), Box<dyn Error>> {
        if self.daemon.is_some() {
            return Ok(());
        }

        let cli_binary = ensure_cli_binary()?;
        let stdout_log = File::create(self.root_dir.join("daemon.stdout.log"))?;
        let stderr_log = File::create(self.root_dir.join("daemon.stderr.log"))?;

        let child = Command::new(&cli_binary)
            .arg("serve")
            .current_dir(&self.project_dir)
            .env("CLAUDE_INSIGHT_HOME", &self.app_home)
            .env(
                "CLAUDE_INSIGHT_CAPTURE_PORT",
                self.capture_addr.port().to_string(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_log))
            .stderr(Stdio::from(stderr_log))
            .spawn()?;

        self.daemon = Some(child);
        self.wait_for_daemon_ready()?;

        Ok(())
    }

    fn stop_daemon(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(mut daemon) = self.daemon.take() {
            let _ = daemon.kill();
            let _ = daemon.wait();
        }

        Ok(())
    }

    fn run_claude(
        &self,
        prompt: &str,
        extra_args: &[&str],
        workdir: &Path,
    ) -> Result<String, Box<dyn Error>> {
        let mut command = Command::new(CLAUDE_BIN);
        command
            .arg("-p")
            .arg("--model")
            .arg("sonnet")
            .arg("--session-id")
            .arg(&self.session_id)
            .arg("--plugin-dir")
            .arg(&self.plugin_dir)
            .current_dir(workdir)
            .env("CLAUDE_INSIGHT_HOME", &self.app_home)
            .env(
                "CLAUDE_INSIGHT_HOOK_URL",
                format!("http://{}/hooks", self.capture_addr),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for arg in extra_args {
            command.arg(arg);
        }

        let mut child = command.spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes())?;
        }

        let output = wait_for_output(child, CLAUDE_TIMEOUT)?;
        ensure_success("claude -p", &output)?;

        Ok(String::from_utf8(output.stdout)?)
    }

    fn wait_for_event(&self, source: &str, event_type: &str) -> Result<(), Box<dyn Error>> {
        let session_id = self.session_id.clone();
        let source = source.to_owned();
        let event_type = event_type.to_owned();

        self.wait_for_value(|| {
            let connection = Connection::open(&self.db_path).ok()?;
            let count = connection
                .query_row(
                    "SELECT COUNT(*)
                     FROM raw_events
                     WHERE session_id = ?1 AND source = ?2 AND event_type = ?3",
                    params![session_id, source, event_type],
                    |row| row.get::<_, i64>(0),
                )
                .ok()?;

            (count > 0).then_some(())
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!("timed out waiting for {source}/{event_type}"),
            )
            .into()
        })
    }

    fn normalize_session(&self) -> Result<(), Box<dyn Error>> {
        let database = Database::new(&self.db_path)?;
        let _ = database.rebuild()?;
        let _ = database.correlate_session(&self.session_id)?;
        Ok(())
    }

    fn hook_events(&self) -> Result<Vec<RawEventRecord>, Box<dyn Error>> {
        self.events_for_session("hook")
    }

    fn events_for_session(&self, source: &str) -> Result<Vec<RawEventRecord>, Box<dyn Error>> {
        let connection = Connection::open(&self.db_path)?;
        let mut statement = connection.prepare(
            "SELECT id, source, event_type, tool_use_id
             FROM raw_events
             WHERE session_id = ?1 AND source = ?2
             ORDER BY id ASC",
        )?;
        let rows = statement.query_map(params![self.session_id.as_str(), source], |row| {
            Ok(RawEventRecord {
                event_type: row.get(2)?,
                tool_use_id: row.get(3)?,
            })
        })?;

        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn query_single_optional_string(
        &self,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<Option<String>, Box<dyn Error>> {
        let connection = Connection::open(&self.db_path)?;
        let value = connection.query_row(sql, params, |row| row.get(0)).ok();
        Ok(value)
    }

    fn query_strings(
        &self,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<String>, Box<dyn Error>> {
        let connection = Connection::open(&self.db_path)?;
        let mut statement = connection.prepare(sql)?;
        let rows = statement.query_map(params, |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn backlog_line_count(&self) -> Result<usize, Box<dyn Error>> {
        if !self.backlog_path.exists() {
            return Ok(0);
        }

        Ok(fs::read_to_string(&self.backlog_path)?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count())
    }

    fn wait_for_daemon_ready(&mut self) -> Result<(), Box<dyn Error>> {
        let started_at = SystemTime::now();

        loop {
            if daemon_health_ready(self.capture_addr)? {
                return Ok(());
            }

            if let Some(daemon) = self.daemon.as_mut() {
                if let Some(status) = daemon.try_wait()? {
                    return Err(io::Error::other(format!(
                        "daemon exited early with status {status}"
                    ))
                    .into());
                }
            }

            if started_at
                .elapsed()
                .unwrap_or_else(|_| Duration::from_secs(0))
                >= DAEMON_READY_TIMEOUT
            {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("timed out waiting for daemon at {}", self.capture_addr),
                )
                .into());
            }

            thread::sleep(POLL_INTERVAL);
        }
    }

    fn wait_for_value<T, F>(&self, mut query: F) -> Option<T>
    where
        F: FnMut() -> Option<T>,
    {
        let started_at = SystemTime::now();
        loop {
            if let Some(value) = query() {
                return Some(value);
            }

            if started_at
                .elapsed()
                .unwrap_or_else(|_| Duration::from_secs(0))
                >= CLAUDE_TIMEOUT
            {
                return None;
            }

            thread::sleep(POLL_INTERVAL);
        }
    }
}

impl Drop for E2eHarness {
    fn drop(&mut self) {
        let _ = self.stop_daemon();
        let _ = fs::remove_dir_all(&self.root_dir);
    }
}

#[test]
fn e2e_simple_prompt() -> Result<(), Box<dyn Error>> {
    if should_skip_in_ci() {
        return Ok(());
    }

    ensure_claude_available()?;
    let mut harness = E2eHarness::new("simple-prompt")?;
    harness.start_daemon()?;

    let output = harness.run_claude(
        "What is 2+2? Reply with just 4.\n",
        &[],
        &harness.project_dir,
    )?;
    assert!(output.contains('4'));

    harness.wait_for_event("hook", "Stop")?;
    harness.wait_for_event("hook", "SessionEnd")?;
    harness.normalize_session()?;

    let hook_events = harness.hook_events()?;
    assert_has_event(&hook_events, "SessionStart");
    assert_has_event(&hook_events, "UserPromptSubmit");
    assert_has_event(&hook_events, "Stop");
    assert_has_event(&hook_events, "SessionEnd");
    assert_order(
        &hook_events,
        &["SessionStart", "UserPromptSubmit", "Stop", "SessionEnd"],
    );

    let session_cwd = harness
        .query_single_optional_string(
            "SELECT cwd FROM sessions WHERE id = ?1",
            &[&harness.session_id],
        )?
        .ok_or_else(|| io::Error::other("missing normalized session row"))?;
    assert_eq!(session_cwd, harness.project_dir.display().to_string());

    Ok(())
}

#[test]
fn e2e_tool_usage() -> Result<(), Box<dyn Error>> {
    if should_skip_in_ci() {
        return Ok(());
    }

    ensure_claude_available()?;
    let mut harness = E2eHarness::new("tool-usage")?;
    harness.start_daemon()?;

    let output = harness.run_claude(
        "Read sample.txt and reply with exactly its contents.\n",
        &[],
        &harness.project_dir,
    )?;
    assert!(output.contains("smoke target"));

    harness.wait_for_event("hook", "PostToolUse")?;
    harness.wait_for_event("hook", "SessionEnd")?;
    harness.normalize_session()?;

    let hook_events = harness.hook_events()?;
    assert_has_event(&hook_events, "PreToolUse");
    assert_has_event(&hook_events, "PostToolUse");
    assert_order(
        &hook_events,
        &[
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "Stop",
            "SessionEnd",
        ],
    );

    let connection = Connection::open(&harness.db_path)?;
    let (tool_name, success): (String, Option<bool>) = connection.query_row(
        "SELECT tool_name, success
         FROM tool_invocations
         WHERE session_id = ?1
         ORDER BY pre_hook_ts ASC
         LIMIT 1",
        [harness.session_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(tool_name, "Read");
    assert_eq!(success, Some(true));

    Ok(())
}

#[test]
fn e2e_file_edit() -> Result<(), Box<dyn Error>> {
    if should_skip_in_ci() {
        return Ok(());
    }

    ensure_claude_available()?;
    let mut harness = E2eHarness::new("file-edit")?;
    harness.start_daemon()?;

    let output = harness.run_claude(
        "Use the Write tool to create output.txt containing exactly hello e3 and then reply DONE.\n",
        &["--permission-mode", "acceptEdits"],
        &harness.project_dir,
    )?;
    assert!(output.to_ascii_uppercase().contains("DONE"));

    harness.wait_for_event("hook", "PostToolUse")?;
    harness.wait_for_event("hook", "SessionEnd")?;
    harness.normalize_session()?;

    let output_path = harness.project_dir.join("output.txt");
    let contents = fs::read_to_string(&output_path)?;
    assert!(contents.contains("hello e3"));

    let tool_names = harness.query_strings(
        "SELECT tool_name
         FROM tool_invocations
         WHERE session_id = ?1
         ORDER BY pre_hook_ts ASC",
        &[&harness.session_id],
    )?;
    assert!(tool_names.iter().any(|tool_name| tool_name == "Write"));

    Ok(())
}

#[test]
fn e2e_permission_denial() -> Result<(), Box<dyn Error>> {
    if should_skip_in_ci() {
        return Ok(());
    }

    ensure_claude_available()?;
    let mut harness = E2eHarness::new("permission-denial")?;
    write_plugin_with_permission_denied(&harness.plugin_dir)?;

    if !supports_permission_denied_plugin_hooks(&harness.plugin_dir)? {
        eprintln!(
            "skipping e2e_permission_denial: installed Claude Code does not accept PermissionDenied session hooks"
        );
        return Ok(());
    }

    harness.start_daemon()?;

    let output = harness.run_claude(
        "Use the Write tool to create denied.txt containing exactly denied and then explain the result in one short sentence.\n",
        &["--permission-mode", "dontAsk"],
        &harness.project_dir,
    )?;
    let output_lower = output.to_ascii_lowercase();
    assert!(output_lower.contains("denied") || output_lower.contains("permission"));

    harness.wait_for_event("hook", "PermissionDenied")?;
    harness.wait_for_event("hook", "SessionEnd")?;
    harness.normalize_session()?;

    let hook_events = harness.hook_events()?;
    assert_has_event(&hook_events, "PermissionDenied");

    let connection = Connection::open(&harness.db_path)?;
    let (tool_name, success, error_text): (String, Option<bool>, Option<String>) = connection
        .query_row(
            "SELECT tool_name, success, error_text
             FROM tool_invocations
             WHERE session_id = ?1
             ORDER BY pre_hook_ts ASC
             LIMIT 1",
            [harness.session_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
    assert_eq!(tool_name, "Write");
    assert_eq!(success, Some(false));
    assert!(error_text.is_some_and(|value| !value.trim().is_empty()));

    let (decision_count, rule_text): (i64, String) = connection.query_row(
        "SELECT COUNT(*), COALESCE(MAX(rule_text), '')
         FROM permission_decisions
         WHERE session_id = ?1
           AND decision = 'denied'",
        [harness.session_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(decision_count, 1);
    assert!(!rule_text.trim().is_empty());

    Ok(())
}

#[test]
fn e2e_instruction_loading() -> Result<(), Box<dyn Error>> {
    if should_skip_in_ci() {
        return Ok(());
    }

    ensure_claude_available()?;
    let mut harness = E2eHarness::new("instruction-loading")?;
    harness.start_daemon()?;

    let _ = harness.run_claude("Reply with OK only.\n", &[], &harness.project_dir)?;

    harness.wait_for_event("hook", "InstructionsLoaded")?;
    harness.wait_for_event("hook", "SessionEnd")?;
    harness.normalize_session()?;

    let paths = harness.query_strings(
        "SELECT file_path
         FROM instruction_loads
         WHERE session_id = ?1
         ORDER BY id ASC",
        &[&harness.session_id],
    )?;
    assert!(paths
        .iter()
        .any(|path| path == &harness.project_dir.join("CLAUDE.md").display().to_string()));

    Ok(())
}

#[test]
fn e2e_session_metadata() -> Result<(), Box<dyn Error>> {
    if should_skip_in_ci() {
        return Ok(());
    }

    ensure_claude_available()?;
    let mut harness = E2eHarness::new("session-metadata")?;
    harness.start_daemon()?;

    let _ = harness.run_claude("Reply with OK only.\n", &[], &harness.project_dir)?;

    harness.wait_for_event("hook", "SessionEnd")?;
    harness.wait_for_event("transcript", "TranscriptAssistantMessage")?;
    harness.normalize_session()?;

    let connection = Connection::open(&harness.db_path)?;
    let (cwd, model, claude_version): (String, Option<String>, Option<String>) = connection
        .query_row(
            "SELECT cwd, model, claude_version
             FROM sessions
             WHERE id = ?1",
            [harness.session_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

    assert_eq!(cwd, harness.project_dir.display().to_string());
    assert!(model.is_some_and(|value| !value.trim().is_empty()));
    assert!(claude_version.is_some_and(|value| !value.trim().is_empty()));

    Ok(())
}

#[test]
fn e2e_multi_tool() -> Result<(), Box<dyn Error>> {
    if should_skip_in_ci() {
        return Ok(());
    }

    ensure_claude_available()?;
    let mut harness = E2eHarness::new("multi-tool")?;
    harness.start_daemon()?;

    let output = harness.run_claude(
        "Read sample.txt, then use the Write tool to create summary.txt containing exactly summary: smoke target, then read summary.txt and reply with exactly its contents.\n",
        &["--permission-mode", "acceptEdits"],
        &harness.project_dir,
    )?;
    assert!(output.contains("summary: smoke target"));

    harness.wait_for_event("hook", "SessionEnd")?;
    harness.normalize_session()?;

    let hook_events = harness.hook_events()?;
    let pre_tool_count = hook_events
        .iter()
        .filter(|event| event.event_type == "PreToolUse")
        .count();
    let post_tool_count = hook_events
        .iter()
        .filter(|event| event.event_type == "PostToolUse")
        .count();
    assert!(pre_tool_count >= 3);
    assert_eq!(pre_tool_count, post_tool_count);

    let tool_names = harness.query_strings(
        "SELECT tool_name
         FROM tool_invocations
         WHERE session_id = ?1
         ORDER BY pre_hook_ts ASC",
        &[&harness.session_id],
    )?;
    assert!(tool_names.len() >= 3);
    assert_eq!(tool_names[0], "Read");
    assert!(tool_names.iter().any(|tool_name| tool_name == "Write"));

    Ok(())
}

#[test]
fn e2e_transcript_correlation() -> Result<(), Box<dyn Error>> {
    if should_skip_in_ci() {
        return Ok(());
    }

    ensure_claude_available()?;
    let mut harness = E2eHarness::new("transcript-correlation")?;
    harness.start_daemon()?;

    let _ = harness.run_claude(
        "Read sample.txt and reply with exactly its contents.\n",
        &[],
        &harness.project_dir,
    )?;

    harness.wait_for_event("hook", "SessionEnd")?;
    harness.wait_for_event("transcript", "TranscriptAssistantMessage")?;
    harness.normalize_session()?;

    let hook_events = harness.hook_events()?;
    let tool_use_id = hook_events
        .iter()
        .find(|event| event.event_type == "PreToolUse")
        .and_then(|event| event.tool_use_id.clone())
        .ok_or_else(|| io::Error::other("missing PreToolUse tool_use_id"))?;

    let connection = Connection::open(&harness.db_path)?;
    let pre_tool_event_id = raw_event_id(
        &connection,
        &harness.session_id,
        "hook",
        "PreToolUse",
        Some(tool_use_id.as_str()),
    )?;
    let post_tool_event_id = raw_event_id(
        &connection,
        &harness.session_id,
        "hook",
        "PostToolUse",
        Some(tool_use_id.as_str()),
    )?;
    let transcript_event_id = raw_event_id(
        &connection,
        &harness.session_id,
        "transcript",
        "TranscriptAssistantMessage",
        Some(tool_use_id.as_str()),
    )?;

    assert_link(
        &connection,
        pre_tool_event_id,
        post_tool_event_id,
        "tool_use_id",
    )?;
    assert_link(
        &connection,
        pre_tool_event_id,
        transcript_event_id,
        "tool_use_id",
    )?;
    assert_link(
        &connection,
        post_tool_event_id,
        transcript_event_id,
        "tool_use_id",
    )?;

    Ok(())
}

#[test]
fn e2e_daemon_crash_recovery() -> Result<(), Box<dyn Error>> {
    if should_skip_in_ci() {
        return Ok(());
    }

    ensure_claude_available()?;
    let mut harness = E2eHarness::new("daemon-crash-recovery")?;

    let output = harness.run_claude(
        "What is 2+2? Reply with just 4.\n",
        &[],
        &harness.project_dir,
    )?;
    assert!(output.contains('4'));

    assert!(harness.backlog_line_count()? >= 4);
    assert!(!harness.db_path.exists() || count_raw_events(&harness.db_path)? == 0);

    harness.start_daemon()?;
    harness.wait_for_event("hook", "SessionEnd")?;

    assert_eq!(harness.backlog_line_count()?, 0);
    let hook_events = harness.hook_events()?;
    assert_has_event(&hook_events, "SessionStart");
    assert_has_event(&hook_events, "UserPromptSubmit");
    assert_has_event(&hook_events, "Stop");
    assert_has_event(&hook_events, "SessionEnd");

    Ok(())
}

fn unique_name(test_name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{test_name}-{}-{nanos}", std::process::id())
}

fn unique_session_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mixed = nanos ^ ((u128::from(std::process::id())) << 64);
    let part1 = (mixed >> 96) as u32;
    let part2 = ((mixed >> 80) & 0xffff) as u16;
    let part3 = (((mixed >> 64) & 0x0fff) as u16) | 0x4000;
    let part4 = (((mixed >> 48) & 0x3fff) as u16) | 0x8000;
    let part5 = mixed & 0x0000_ffff_ffff_ffff;

    format!("{part1:08x}-{part2:04x}-{part3:04x}-{part4:04x}-{part5:012x}")
}

fn reserve_capture_port() -> Result<u16, io::Error> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn initialize_git_repo(project_dir: &Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new("git")
        .arg("init")
        .current_dir(project_dir)
        .output()?;
    ensure_success("git init", &output)?;
    Ok(())
}

fn actual_transcript_root() -> Result<PathBuf, Box<dyn Error>> {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    let transcript_root = home.join(".claude").join("projects");

    if !transcript_root.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "Claude transcript root does not exist: {}",
                transcript_root.display()
            ),
        )
        .into());
    }

    Ok(transcript_root)
}

fn create_projects_symlink(target: &Path, link: &Path) -> Result<(), Box<dyn Error>> {
    if link.exists() {
        return Ok(());
    }

    #[cfg(unix)]
    symlink(target, link)?;
    #[cfg(windows)]
    symlink_dir(target, link)?;

    Ok(())
}

fn write_plugin(plugin_dir: &Path) -> Result<(), Box<dyn Error>> {
    write_plugin_impl(plugin_dir, false)
}

fn write_plugin_with_permission_denied(plugin_dir: &Path) -> Result<(), Box<dyn Error>> {
    write_plugin_impl(plugin_dir, true)
}

fn write_plugin_impl(
    plugin_dir: &Path,
    include_permission_denied: bool,
) -> Result<(), Box<dyn Error>> {
    let manifest_dir = plugin_dir.join(".claude-plugin");
    let hooks_dir = plugin_dir.join("hooks");
    let hook_script = hooks_dir.join("post-hook.sh");

    fs::create_dir_all(&manifest_dir)?;
    fs::create_dir_all(&hooks_dir)?;

    fs::write(
        manifest_dir.join("plugin.json"),
        r#"{
  "name": "claude-insight-e2e",
  "version": "0.0.1",
  "description": "Session-local hook bridge for claude-insight e2e",
  "author": {
    "name": "Codex"
  }
}"#,
    )?;
    fs::write(
        &hook_script,
        r#"#!/bin/bash
set -euo pipefail
payload=$(cat)
url="${CLAUDE_INSIGHT_HOOK_URL}"
if ! curl --connect-timeout 1 -fsS -m 1 -H 'content-type: application/json' -d "$payload" "$url" >/dev/null; then
  state_root="${CLAUDE_INSIGHT_HOME:-$HOME}"
  backlog_dir="$state_root/.claude-insight"
  mkdir -p "$backlog_dir"
  printf '%s\n' "$payload" >> "$backlog_dir/backlog.jsonl"
fi
"#,
    )?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&hook_script)?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&hook_script, permissions)?;
    }

    let hook_script_path = hook_script.display().to_string();
    let mut hook_entries = vec![
        format!(
            r#""SessionStart": [{{ "hooks": [{{ "type": "command", "command": "{hook_script_path}" }}] }}]"#
        ),
        format!(
            r#""UserPromptSubmit": [{{ "hooks": [{{ "type": "command", "command": "{hook_script_path}" }}] }}]"#
        ),
        format!(
            r#""Stop": [{{ "hooks": [{{ "type": "command", "command": "{hook_script_path}" }}] }}]"#
        ),
        format!(
            r#""SessionEnd": [{{ "hooks": [{{ "type": "command", "command": "{hook_script_path}" }}] }}]"#
        ),
        format!(
            r#""InstructionsLoaded": [{{ "matcher": ".*", "hooks": [{{ "type": "command", "command": "{hook_script_path}" }}] }}]"#
        ),
        format!(
            r#""PreToolUse": [{{ "matcher": ".*", "hooks": [{{ "type": "command", "command": "{hook_script_path}" }}] }}]"#
        ),
        format!(
            r#""PostToolUse": [{{ "matcher": ".*", "hooks": [{{ "type": "command", "command": "{hook_script_path}" }}] }}]"#
        ),
        format!(
            r#""Notification": [{{ "hooks": [{{ "type": "command", "command": "{hook_script_path}" }}] }}]"#
        ),
    ];
    if include_permission_denied {
        hook_entries.push(format!(
            r#""PermissionDenied": [{{ "hooks": [{{ "type": "command", "command": "{hook_script_path}" }}] }}]"#
        ));
    }
    let hooks_json = format!(
        "{{\n  \"hooks\": {{\n    {}\n  }}\n}}",
        hook_entries.join(",\n    ")
    );
    fs::write(hooks_dir.join("hooks.json"), hooks_json)?;

    Ok(())
}

fn ensure_cli_binary() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = CLI_BINARY_ONCE.get() {
        return Ok(path.clone());
    }

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cli_binary = repo_root.join(CLI_BINARY_RELATIVE_PATH);
    if !cli_binary.exists() {
        let output = Command::new("cargo")
            .args(["build", "-p", "cli", "--bin", "claude-insight"])
            .current_dir(&repo_root)
            .output()?;
        ensure_success("cargo build -p cli --bin claude-insight", &output)?;
    }

    let _ = CLI_BINARY_ONCE.set(cli_binary.clone());
    Ok(cli_binary)
}

fn ensure_claude_available() -> Result<(), Box<dyn Error>> {
    let output = Command::new(CLAUDE_BIN).arg("--version").output()?;
    ensure_success("claude --version", &output)?;
    Ok(())
}

fn supports_permission_denied_plugin_hooks(plugin_dir: &Path) -> Result<bool, Box<dyn Error>> {
    let output = Command::new(CLAUDE_BIN)
        .args(["plugins", "validate"])
        .arg(plugin_dir)
        .output()?;

    if output.status.success() {
        return Ok(true);
    }

    let combined_output = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if combined_output.contains("hooks.PermissionDenied")
        || combined_output.contains("Invalid key in record")
    {
        return Ok(false);
    }

    Err(io::Error::other(format!(
        "claude plugins validate failed unexpectedly for {}:\n{}",
        plugin_dir.display(),
        combined_output
    ))
    .into())
}

fn wait_for_output(mut child: Child, timeout: Duration) -> Result<Output, Box<dyn Error>> {
    let started_at = SystemTime::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }

        if started_at
            .elapsed()
            .unwrap_or_else(|_| Duration::from_secs(0))
            >= timeout
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, "command timed out").into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn ensure_success(label: &str, output: &Output) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        return Ok(());
    }

    Err(io::Error::other(format!(
        "{label} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
    .into())
}

fn daemon_health_ready(capture_addr: SocketAddr) -> Result<bool, io::Error> {
    let stream = match TcpStream::connect_timeout(&capture_addr, POLL_INTERVAL) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::TimedOut
                    | io::ErrorKind::AddrNotAvailable
            ) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };

    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(true)
}

fn assert_has_event(events: &[RawEventRecord], event_type: &str) {
    assert!(
        events.iter().any(|event| event.event_type == event_type),
        "missing event {event_type} in {:?}",
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>()
    );
}

fn assert_order(events: &[RawEventRecord], event_types: &[&str]) {
    let mut last_index = None;

    for event_type in event_types {
        let search_start = last_index.map_or(0, |index| index + 1);
        let relative_index = events[search_start..]
            .iter()
            .position(|event| event.event_type == *event_type)
            .unwrap_or_else(|| panic!("event {event_type} was not found"));
        let index = search_start + relative_index;
        last_index = Some(index);
    }
}

fn count_raw_events(db_path: &Path) -> Result<i64, Box<dyn Error>> {
    if !db_path.exists() {
        return Ok(0);
    }

    let connection = Connection::open(db_path)?;
    let count = connection.query_row("SELECT COUNT(*) FROM raw_events", [], |row| row.get(0))?;
    Ok(count)
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

fn should_skip_in_ci() -> bool {
    std::env::var_os("CI").is_some()
        && std::env::var("GITHUB_REF_TYPE").ok().as_deref() != Some("tag")
}
