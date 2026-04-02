#![deny(clippy::expect_used, clippy::unwrap_used)]

use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode, Stdio},
    thread,
    time::Duration,
};

use clap::{error::ErrorKind, CommandFactory, Parser, Subcommand};
use crossterm::style::{Color, Stylize};

type CliResult<T = ()> = Result<T, Box<dyn Error>>;

#[derive(Debug, Parser)]
#[command(
    name = "claude-insight",
    about = "Observe Claude Code sessions from the terminal"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Install hooks and prepare local storage.
    Init,
    /// Run the daemon in the foreground.
    Serve,
    /// Show recent sessions or a colored event timeline for one session.
    Trace {
        /// Session identifier to inspect. Omit it to list recent sessions.
        session_id: Option<String>,
        /// Maximum number of recent sessions to display when no session id is supplied.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Run a full-text query across captured events.
    Search {
        /// FTS query to run against captured events.
        query: String,
        /// Maximum number of matching events to display.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Prune events older than the retention window.
    Gc {
        /// Retain this many days of raw event history.
        #[arg(long, default_value_t = 90)]
        days: u32,
    },
    /// Materialize normalized tables from the raw event store.
    Normalize {
        /// Rebuild normalized tables from scratch before replaying raw events.
        #[arg(long)]
        rebuild: bool,
    },
    /// Start or stop the local daemon manually.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum DaemonCommand {
    /// Spawn the daemon in the background.
    Start,
    /// Stop the background daemon if it is running.
    Stop,
}

#[tokio::main]
async fn main() -> ExitCode {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    match Cli::try_parse() {
        Ok(cli) => match run(cli).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{}", format!("error: {error}").red());
                ExitCode::FAILURE
            }
        },
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                print!("{}", claude_insight_tui::ansi_banner());
                let _ = error.print();
                ExitCode::SUCCESS
            }
            _ => {
                let _ = error.print();
                ExitCode::FAILURE
            }
        },
    }
}

async fn run(cli: Cli) -> CliResult {
    match cli.command {
        Some(Command::Init) => handle_init(),
        Some(Command::Serve) => handle_serve().await,
        Some(Command::Trace { session_id, limit }) => handle_trace(session_id.as_deref(), limit),
        Some(Command::Search { query, limit }) => handle_search(&query, limit),
        Some(Command::Gc { days }) => handle_gc(days),
        Some(Command::Normalize { rebuild }) => handle_normalize(rebuild),
        Some(Command::Daemon { command }) => handle_daemon(command).await,
        None => {
            print!("{}", claude_insight_tui::ansi_banner());
            let mut command = Cli::command();
            command.print_help()?;
            println!();
            Ok(())
        }
    }
}

fn handle_init() -> CliResult {
    let db_path = claude_insight_storage::Database::default_path()?;
    let tui = claude_insight_tui::TuiStub::new("Claude Insight");
    let title = tui.title_line().to_string();

    print!("{}", claude_insight_tui::ansi_banner());
    println!("{} {}", "Initialized".green().bold(), title.white().bold());
    println!("database: {}", db_path.display().to_string().cyan());
    println!("storage: {}", "sqlite".dark_grey());

    Ok(())
}

async fn handle_serve() -> CliResult {
    let mut daemon =
        claude_insight_daemon::DaemonManager::new(claude_insight_daemon::DaemonConfig::default());
    let report = daemon.start().await?;

    println!(
        "{} {} {}",
        "Daemon running.".green().bold(),
        format!("capture={}", report.capture_addr).cyan(),
        database_path_label()?.dark_grey()
    );
    daemon.wait_for_shutdown().await?;

    Ok(())
}

fn handle_trace(session_id: Option<&str>, limit: usize) -> CliResult {
    let database = claude_insight_storage::Database::open_default()?;

    match session_id {
        Some(session_id) => {
            let events = database.query_raw_events_by_session(session_id)?;

            if events.is_empty() {
                println!(
                    "{} {}",
                    "No events found for session".yellow(),
                    session_id.cyan()
                );
                return Ok(());
            }

            println!("{} {}", "Trace".bold(), session_id.cyan().bold());
            for event in events {
                let (emoji, color) = event_marker(&event.event_type);
                println!(
                    "{} {} {} {}",
                    event.ts.dark_grey(),
                    emoji.with(color),
                    event.event_type.as_str().with(color).bold(),
                    payload_summary(&event.payload_json).with(color)
                );
            }
        }
        None => {
            let sessions = database.list_recent_sessions(limit)?;

            if sessions.is_empty() {
                println!(
                    "{}",
                    "No sessions found. Run `claude-insight init` to start capturing.".yellow()
                );
                return Ok(());
            }

            println!("{}", "Recent sessions".bold());
            for session in sessions {
                let last_event = session
                    .last_event_type
                    .as_deref()
                    .unwrap_or("Unknown")
                    .to_string();
                println!(
                    "{} {} {} {} {}",
                    "📂".magenta(),
                    session.session_id.cyan().bold(),
                    format!("{} -> {}", session.start_ts, session.end_ts).dark_grey(),
                    format!("{} events", session.event_count).green(),
                    last_event.white()
                );
            }
        }
    }

    Ok(())
}

fn handle_search(query: &str, limit: usize) -> CliResult {
    let database = claude_insight_storage::Database::open_default()?;
    let mut hits = database.search_fts(query)?;

    println!("{} {}", "Search".bold(), query.cyan().bold());

    if hits.is_empty() {
        println!("{}", "No matching events found.".yellow());
        return Ok(());
    }

    if hits.len() > limit {
        hits.truncate(limit);
    }

    for event in hits {
        let session_id = event.session_id.as_deref().unwrap_or("<unknown>");
        let (emoji, color) = event_marker(&event.event_type);
        println!(
            "{} {} {} {} {}",
            event.ts.dark_grey(),
            emoji.with(color),
            session_id.cyan(),
            event.event_type.as_str().with(color).bold(),
            payload_summary(&event.payload_json).white()
        );
    }

    Ok(())
}

fn handle_gc(days: u32) -> CliResult {
    let database = claude_insight_storage::Database::open_default()?;
    let report = database.gc_raw_events(days)?;

    println!(
        "{} {} raw events and {} normalized sessions older than {} days.",
        "Deleted".green().bold(),
        report.deleted_events.to_string().cyan(),
        report.deleted_sessions.to_string().cyan(),
        report.retention_days.to_string().cyan()
    );

    Ok(())
}

fn handle_normalize(rebuild: bool) -> CliResult {
    let database = claude_insight_storage::Database::open_default()?;
    let report = database.normalize()?;

    println!(
        "{} {} raw events (last raw event id: {}).",
        if rebuild {
            "Rebuilt".green().bold()
        } else {
            "Normalized".green().bold()
        },
        report.processed_events.to_string().cyan(),
        report.last_raw_event_id.to_string().cyan()
    );

    Ok(())
}

async fn handle_daemon(command: DaemonCommand) -> CliResult {
    match command {
        DaemonCommand::Start => daemon_start().await,
        DaemonCommand::Stop => daemon_stop(),
    }
}

async fn daemon_start() -> CliResult {
    let pid_path = pid_file_path()?;

    if let Ok(pid) = read_pid_file(&pid_path) {
        if is_process_running(pid)? {
            println!("{} {}", "Daemon already running with pid".yellow(), pid);
            return Ok(());
        }

        let _ = fs::remove_file(&pid_path);
    }

    fs::create_dir_all(app_dir()?)?;

    let mut child = ProcessCommand::new(daemon_executable_path()?)
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    for _ in 0..300 {
        if let Some(status) = child.try_wait()? {
            return Err(format!("daemon exited early with status {status}").into());
        }

        if let Ok(pid) = read_pid_file(&pid_path) {
            if is_process_running(pid)? {
                println!("{} {}", "Daemon started with pid".green().bold(), pid);
                return Ok(());
            }
        }

        thread::sleep(Duration::from_millis(100));
    }

    Err("timed out waiting for daemon pid file".into())
}

fn daemon_stop() -> CliResult {
    let pid_path = pid_file_path()?;
    let pid = match read_pid_file(&pid_path) {
        Ok(pid) => pid,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            println!("{}", "Daemon is not running.".yellow());
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };

    if !is_process_running(pid)? {
        let _ = fs::remove_file(&pid_path);
        println!("{}", "Removed stale daemon pid file.".yellow());
        return Ok(());
    }

    terminate_process(pid)?;

    for _ in 0..300 {
        if !is_process_running(pid)? {
            let _ = fs::remove_file(&pid_path);
            println!(
                "{} {}",
                "Daemon stopped (pid".green().bold(),
                format!("{pid})").green()
            );
            return Ok(());
        }

        thread::sleep(Duration::from_millis(100));
    }

    Err(format!("timed out waiting for pid {pid} to stop").into())
}

fn app_dir() -> CliResult<PathBuf> {
    Ok(claude_insight_storage::Database::default_dir()?)
}

fn daemon_executable_path() -> CliResult<PathBuf> {
    let current = std::env::current_exe()?;
    let executable_name = if cfg!(windows) {
        "claude-insight.exe"
    } else {
        "claude-insight"
    };

    if current
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "deps")
    {
        if let Some(root) = current.parent().and_then(Path::parent) {
            let candidate = root.join(executable_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    Ok(current)
}

fn pid_file_path() -> CliResult<PathBuf> {
    Ok(app_dir()?.join("daemon.pid"))
}

fn database_path_label() -> CliResult<String> {
    Ok(format!(
        "db={}",
        claude_insight_storage::Database::default_path()?.display()
    ))
}

fn read_pid_file(path: &Path) -> io::Result<u32> {
    let raw = fs::read_to_string(path)?;
    raw.trim().parse::<u32>().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid pid file {}: {error}", path.display()),
        )
    })
}

#[cfg(unix)]
fn is_process_running(pid: u32) -> io::Result<bool> {
    let status = ProcessCommand::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()?;
    Ok(status.success())
}

#[cfg(not(unix))]
fn is_process_running(pid: u32) -> io::Result<bool> {
    let status = ProcessCommand::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}")])
        .status()?;
    Ok(status.success())
}

#[cfg(unix)]
fn terminate_process(pid: u32) -> io::Result<()> {
    let status = ProcessCommand::new("kill").arg(pid.to_string()).status()?;

    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("failed to stop pid {pid}")))
    }
}

#[cfg(not(unix))]
fn terminate_process(pid: u32) -> io::Result<()> {
    let status = ProcessCommand::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("failed to stop pid {pid}")))
    }
}

fn event_marker(event_type: &str) -> (&'static str, Color) {
    match event_type {
        "SessionStart" => ("📋", Color::Green),
        "SessionEnd" | "Stop" => ("🏁", Color::DarkGreen),
        "PreToolUse" => ("🔧", Color::Yellow),
        "PostToolUse" => ("✅", Color::Green),
        "PostToolUseFailure" => ("❌", Color::Red),
        "Notification" => ("🔔", Color::Blue),
        "UserPromptSubmit" => ("💬", Color::Cyan),
        "FileChanged" => ("📝", Color::Magenta),
        "PermissionRequest" => ("🛂", Color::DarkYellow),
        "PermissionDenied" => ("⛔", Color::Red),
        "InstructionsLoaded" => ("📚", Color::Blue),
        _ => ("•", Color::White),
    }
}

fn payload_summary(payload_json: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(payload_json) {
        Ok(payload) => {
            if let Some(tool_name) = payload.get("tool_name").and_then(serde_json::Value::as_str) {
                let command = payload
                    .get("tool_input")
                    .and_then(|tool_input| tool_input.get("command"))
                    .and_then(serde_json::Value::as_str);
                return summarize_with_optional_detail(tool_name, command);
            }

            for key in ["file_path", "prompt", "message", "reason", "source"] {
                if let Some(value) = payload.get(key).and_then(serde_json::Value::as_str) {
                    return truncate(value, 80);
                }
            }

            truncate(payload.to_string().as_str(), 80)
        }
        Err(_) => truncate(payload_json, 80),
    }
}

fn summarize_with_optional_detail(label: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) => truncate(&format!("{label}: {detail}"), 80),
        None => label.to_string(),
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut result = String::new();
    let mut chars = value.chars();

    for _ in 0..max_chars {
        match chars.next() {
            Some(ch) => result.push(ch),
            None => return result,
        }
    }

    if chars.next().is_some() {
        result.push_str("...");
    }

    result
}
