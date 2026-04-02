#![deny(clippy::expect_used, clippy::unwrap_used)]

use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
    Init,
    Serve,
    Trace { session_id: String },
    Search { query: String },
    Gc,
}

#[tokio::main]
async fn main() -> ExitCode {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let cli = Cli::parse();
    let daemon = claude_insight_daemon::DaemonStub::new("127.0.0.1:4180", "127.0.0.1:4181");
    let tui = claude_insight_tui::TuiStub::new("Claude Insight");
    let _router = claude_insight_capture::hooks_router();

    let exit_code = match cli.command {
        Some(Command::Init) => {
            let _storage = daemon.storage();
            let _title = tui.title_line();
            ExitCode::SUCCESS
        }
        Some(Command::Serve) => {
            let _daemon_router = daemon.router().await;
            ExitCode::SUCCESS
        }
        Some(Command::Trace { session_id }) => {
            tracing::info!(%session_id, "trace stub");
            ExitCode::SUCCESS
        }
        Some(Command::Search { query }) => {
            tracing::info!(%query, "search stub");
            ExitCode::SUCCESS
        }
        Some(Command::Gc) | None => ExitCode::SUCCESS,
    };

    exit_code
}
