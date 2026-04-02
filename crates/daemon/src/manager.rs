use std::{
    env, fmt, fs, io,
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::Router;
use claude_insight_capture::{
    hooks_router_with_config, BacklogError, BacklogProcessor, CaptureConfig, TranscriptTailer,
    TranscriptTailerConfig, DEFAULT_CAPTURE_PORT,
};
use claude_insight_storage::Database;
use tokio::{net::TcpListener, sync::Notify, task::JoinHandle};

const APP_STATE_DIR: &str = ".claude-insight";
const BACKLOG_FILE_NAME: &str = "backlog.jsonl";
const DATABASE_FILE_NAME: &str = "insight.db";
const PID_FILE_NAME: &str = "daemon.pid";
const TRANSCRIPT_ROOT_DIR: &str = ".claude/projects";
const TRANSCRIPT_POSITIONS_FILE: &str = "transcript_offsets.json";
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_millis(200);
const TRANSCRIPT_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    pub capture_addr: SocketAddr,
    pub database_path: PathBuf,
    pub backlog_path: PathBuf,
    pub pid_file_path: PathBuf,
    pub transcript_root: PathBuf,
    pub transcript_positions_path: PathBuf,
    pub transcript_poll_interval: Duration,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        let home_root = app_home_root().unwrap_or_else(|_| PathBuf::from("."));
        let state_dir = home_root.join(APP_STATE_DIR);
        let capture_port = env::var("CLAUDE_INSIGHT_CAPTURE_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(DEFAULT_CAPTURE_PORT);

        Self {
            capture_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, capture_port)),
            database_path: state_dir.join(DATABASE_FILE_NAME),
            backlog_path: state_dir.join(BACKLOG_FILE_NAME),
            pid_file_path: state_dir.join(PID_FILE_NAME),
            transcript_root: home_root.join(TRANSCRIPT_ROOT_DIR),
            transcript_positions_path: state_dir.join(TRANSCRIPT_POSITIONS_FILE),
            transcript_poll_interval: TRANSCRIPT_POLL_INTERVAL,
        }
    }
}

impl DaemonConfig {
    pub fn with_capture_addr(mut self, capture_addr: SocketAddr) -> Self {
        self.capture_addr = capture_addr;
        self
    }

    pub fn with_database_path(mut self, database_path: impl Into<PathBuf>) -> Self {
        self.database_path = database_path.into();
        self
    }

    pub fn with_backlog_path(mut self, backlog_path: impl Into<PathBuf>) -> Self {
        self.backlog_path = backlog_path.into();
        self
    }

    pub fn with_pid_file_path(mut self, pid_file_path: impl Into<PathBuf>) -> Self {
        self.pid_file_path = pid_file_path.into();
        self
    }

    pub fn with_transcript_root(mut self, transcript_root: impl Into<PathBuf>) -> Self {
        self.transcript_root = transcript_root.into();
        self
    }

    pub fn with_transcript_positions_path(
        mut self,
        transcript_positions_path: impl Into<PathBuf>,
    ) -> Self {
        self.transcript_positions_path = transcript_positions_path.into();
        self
    }
}

#[derive(Debug)]
pub enum DaemonError {
    MissingHomeDirectory,
    AlreadyRunning {
        pid: Option<u32>,
        capture_addr: SocketAddr,
    },
    CapturePortBusy(SocketAddr),
    Io(io::Error),
    Storage(rusqlite::Error),
    Backlog(BacklogError),
    Transcript(claude_insight_capture::TranscriptTailerError),
    Join(String),
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHomeDirectory => {
                f.write_str("HOME or CLAUDE_INSIGHT_HOME must be set for daemon paths")
            }
            Self::AlreadyRunning { pid, capture_addr } => match pid {
                Some(pid) => write!(f, "daemon already running on {capture_addr} with pid {pid}"),
                None => write!(f, "daemon already responding on {capture_addr}"),
            },
            Self::CapturePortBusy(capture_addr) => {
                write!(f, "capture port already in use at {capture_addr}")
            }
            Self::Io(error) => write!(f, "{error}"),
            Self::Storage(error) => write!(f, "{error}"),
            Self::Backlog(error) => write!(f, "{error}"),
            Self::Transcript(error) => write!(f, "{error}"),
            Self::Join(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for DaemonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Storage(error) => Some(error),
            Self::Backlog(error) => Some(error),
            Self::Transcript(error) => Some(error),
            Self::Join(_) | Self::MissingHomeDirectory | Self::AlreadyRunning { .. } => None,
            Self::CapturePortBusy(_) => None,
        }
    }
}

impl From<io::Error> for DaemonError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for DaemonError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Storage(value)
    }
}

impl From<BacklogError> for DaemonError {
    fn from(value: BacklogError) -> Self {
        Self::Backlog(value)
    }
}

impl From<claude_insight_capture::TranscriptTailerError> for DaemonError {
    fn from(value: claude_insight_capture::TranscriptTailerError) -> Self {
        Self::Transcript(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonStartReport {
    pub capture_addr: SocketAddr,
    pub pid: u32,
    pub backlog_processed: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DaemonShutdownReport {
    pub transcript_events_processed: usize,
}

#[derive(Debug)]
pub struct DaemonManager {
    config: DaemonConfig,
    running: Option<RunningDaemon>,
}

impl DaemonManager {
    pub fn new(config: DaemonConfig) -> Self {
        Self {
            config,
            running: None,
        }
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub fn capture_addr(&self) -> SocketAddr {
        self.running
            .as_ref()
            .map(|running| running.capture_addr)
            .unwrap_or(self.config.capture_addr)
    }

    pub async fn start(&mut self) -> Result<DaemonStartReport, DaemonError> {
        if let Some(running) = &self.running {
            return Err(DaemonError::AlreadyRunning {
                pid: Some(running.pid),
                capture_addr: running.capture_addr,
            });
        }

        ensure_parent_dir(&self.config.database_path)?;
        ensure_parent_dir(&self.config.backlog_path)?;
        ensure_parent_dir(&self.config.pid_file_path)?;
        ensure_parent_dir(&self.config.transcript_positions_path)?;
        fs::create_dir_all(&self.config.transcript_root)?;

        self.clear_stale_pid_file()?;

        if let Some(pid) = self.active_pid_from_file()? {
            return Err(DaemonError::AlreadyRunning {
                pid: Some(pid),
                capture_addr: self.config.capture_addr,
            });
        }

        if daemon_responds(self.config.capture_addr)? {
            return Err(DaemonError::AlreadyRunning {
                pid: read_pid_file(&self.config.pid_file_path).ok(),
                capture_addr: self.config.capture_addr,
            });
        }

        let listener = TcpListener::bind(self.config.capture_addr)
            .await
            .map_err(|error| match error.kind() {
                io::ErrorKind::AddrInUse => DaemonError::CapturePortBusy(self.config.capture_addr),
                _ => DaemonError::Io(error),
            })?;
        let capture_addr = listener.local_addr()?;
        let pid = std::process::id();
        let backlog_processed = process_startup_backlog(&self.config)?;
        let router = daemon_router(capture_addr, &self.config);
        let shutdown = ShutdownController::new();
        write_pid_file(&self.config.pid_file_path, pid)?;

        let config = self.config.clone();
        let shutdown_clone = shutdown.clone();
        let join_handle =
            tokio::spawn(async move { run_daemon(listener, router, config, shutdown_clone).await });

        self.running = Some(RunningDaemon {
            capture_addr,
            pid,
            shutdown,
            join_handle,
        });

        Ok(DaemonStartReport {
            capture_addr,
            pid,
            backlog_processed,
        })
    }

    pub async fn stop(&mut self) -> Result<DaemonShutdownReport, DaemonError> {
        let Some(running) = self.running.take() else {
            return Ok(DaemonShutdownReport::default());
        };

        running.shutdown.trigger();
        join_running_daemon(running).await
    }

    pub async fn wait_for_shutdown(&mut self) -> Result<DaemonShutdownReport, DaemonError> {
        let Some(running) = self.running.take() else {
            return Ok(DaemonShutdownReport::default());
        };

        join_running_daemon(running).await
    }

    pub fn health_check(&self) -> Result<bool, DaemonError> {
        daemon_responds(self.capture_addr())
    }

    fn active_pid_from_file(&self) -> Result<Option<u32>, DaemonError> {
        match read_pid_file(&self.config.pid_file_path) {
            Ok(pid) if is_process_running(pid)? => Ok(Some(pid)),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(DaemonError::Io(error)),
        }
    }

    fn clear_stale_pid_file(&self) -> Result<(), DaemonError> {
        match read_pid_file(&self.config.pid_file_path) {
            Ok(pid) if !is_process_running(pid)? => {
                remove_file_if_exists(&self.config.pid_file_path)?;
                Ok(())
            }
            Ok(_) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(DaemonError::Io(error)),
        }
    }
}

#[derive(Debug)]
struct RunningDaemon {
    capture_addr: SocketAddr,
    pid: u32,
    shutdown: ShutdownController,
    join_handle: JoinHandle<Result<DaemonShutdownReport, DaemonError>>,
}

#[derive(Debug, Clone)]
struct ShutdownController {
    is_shutdown: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl ShutdownController {
    fn new() -> Self {
        Self {
            is_shutdown: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    fn trigger(&self) {
        if !self.is_shutdown.swap(true, Ordering::SeqCst) {
            self.notify.notify_waiters();
        }
    }

    fn is_triggered(&self) -> bool {
        self.is_shutdown.load(Ordering::SeqCst)
    }

    async fn wait(&self) {
        if self.is_triggered() {
            return;
        }

        self.notify.notified().await;
    }
}

async fn join_running_daemon(running: RunningDaemon) -> Result<DaemonShutdownReport, DaemonError> {
    running
        .join_handle
        .await
        .map_err(|error| DaemonError::Join(format!("daemon task join failed: {error}")))?
}

async fn run_daemon(
    listener: TcpListener,
    router: Router,
    config: DaemonConfig,
    shutdown: ShutdownController,
) -> Result<DaemonShutdownReport, DaemonError> {
    let server_shutdown = shutdown.clone();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                server_shutdown.wait().await;
            })
            .await
            .map_err(DaemonError::Io)
    });

    let transcript_shutdown = shutdown.clone();
    let transcript_config = config.clone();
    let transcript_task = tokio::task::spawn_blocking(move || {
        run_transcript_tailer_loop(transcript_config, transcript_shutdown)
    });

    let signal_task = spawn_signal_task(shutdown.clone());
    let server_result = server_task
        .await
        .map_err(|error| DaemonError::Join(format!("capture server join failed: {error}")))?;
    shutdown.trigger();

    let transcript_events_processed = transcript_task
        .await
        .map_err(|error| DaemonError::Join(format!("transcript tailer join failed: {error}")))??;
    signal_task
        .await
        .map_err(|error| DaemonError::Join(format!("signal task join failed: {error}")))?;
    remove_file_if_exists(&config.pid_file_path)?;
    server_result?;

    Ok(DaemonShutdownReport {
        transcript_events_processed,
    })
}

fn process_startup_backlog(config: &DaemonConfig) -> Result<usize, DaemonError> {
    let database = Database::new(&config.database_path)?;
    let processor = BacklogProcessor::new(&config.backlog_path);

    Ok(processor.process(&database)?)
}

fn daemon_router(capture_addr: SocketAddr, config: &DaemonConfig) -> Router {
    let capture_config = CaptureConfig::default()
        .with_database_path(&config.database_path)
        .with_port(capture_addr.port());

    hooks_router_with_config(capture_config)
}

fn run_transcript_tailer_loop(
    config: DaemonConfig,
    shutdown: ShutdownController,
) -> Result<usize, DaemonError> {
    let tailer_config = TranscriptTailerConfig {
        transcript_root: config.transcript_root,
        positions_path: config.transcript_positions_path,
        database_path: config.database_path,
    };
    let mut tailer = TranscriptTailer::new(tailer_config)?;
    let mut processed_events = 0;

    while !shutdown.is_triggered() {
        processed_events += tailer.wait_for_events(config.transcript_poll_interval)?;
    }

    Ok(processed_events)
}

#[cfg(unix)]
fn spawn_signal_task(shutdown: ShutdownController) -> JoinHandle<()> {
    tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(error) => {
                tracing::warn!(error = %error, "failed to register SIGTERM handler");
                return;
            }
        };

        tokio::select! {
            _ = terminate.recv() => shutdown.trigger(),
            _ = tokio::signal::ctrl_c() => shutdown.trigger(),
            _ = shutdown.wait() => {}
        }
    })
}

#[cfg(not(unix))]
fn spawn_signal_task(shutdown: ShutdownController) -> JoinHandle<()> {
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => shutdown.trigger(),
            _ = shutdown.wait() => {}
        }
    })
}

fn app_home_root() -> Result<PathBuf, DaemonError> {
    env::var_os("CLAUDE_INSIGHT_HOME")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or(DaemonError::MissingHomeDirectory)
}

fn ensure_parent_dir(path: &Path) -> Result<(), DaemonError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    Ok(())
}

fn write_pid_file(path: &Path, pid: u32) -> Result<(), DaemonError> {
    ensure_parent_dir(path)?;
    fs::write(path, pid.to_string())?;
    Ok(())
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

fn remove_file_if_exists(path: &Path) -> Result<(), DaemonError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DaemonError::Io(error)),
    }
}

fn daemon_responds(capture_addr: SocketAddr) -> Result<bool, DaemonError> {
    let stream = match TcpStream::connect_timeout(&capture_addr, HEALTH_CHECK_TIMEOUT) {
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
        Err(error) => return Err(DaemonError::Io(error)),
    };
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(true)
}

#[cfg(unix)]
fn is_process_running(pid: u32) -> io::Result<bool> {
    let status = Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()?;

    Ok(status.success())
}

#[cfg(not(unix))]
fn is_process_running(pid: u32) -> io::Result<bool> {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}")])
        .output()?;

    Ok(output.status.success()
        && String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        error::Error,
        net::TcpListener as StdTcpListener,
        sync::{Mutex, MutexGuard},
        time::Duration,
    };
    use tempfile::TempDir;
    use tokio::time::{sleep, timeout};

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    struct TestWorkspace {
        _guard: MutexGuard<'static, ()>,
        _tempdir: TempDir,
        config: DaemonConfig,
    }

    impl TestWorkspace {
        fn new(test_name: &str) -> Result<Self, Box<dyn Error>> {
            let guard = TEST_MUTEX
                .lock()
                .map_err(|_| io::Error::other("test mutex poisoned"))?;
            let tempdir = TempDir::new()?;
            let root = tempdir.path().join(test_name);
            let state_dir = root.join(APP_STATE_DIR);
            let capture_addr = available_capture_addr()?;
            let config = DaemonConfig::default()
                .with_capture_addr(capture_addr)
                .with_database_path(state_dir.join(DATABASE_FILE_NAME))
                .with_backlog_path(state_dir.join(BACKLOG_FILE_NAME))
                .with_pid_file_path(state_dir.join(PID_FILE_NAME))
                .with_transcript_root(root.join(TRANSCRIPT_ROOT_DIR))
                .with_transcript_positions_path(state_dir.join(TRANSCRIPT_POSITIONS_FILE));

            Ok(Self {
                _guard: guard,
                _tempdir: tempdir,
                config,
            })
        }

        fn manager(&self) -> DaemonManager {
            DaemonManager::new(self.config.clone())
        }
    }

    #[tokio::test]
    async fn daemon_start_binds_port_and_writes_pid_file() -> Result<(), Box<dyn Error>> {
        let workspace = TestWorkspace::new("start-binds-port")?;
        let mut manager = workspace.manager();
        let report = manager.start().await?;

        wait_for_health(report.capture_addr).await?;

        assert!(workspace.config.pid_file_path.exists());
        assert!(daemon_responds(report.capture_addr)?);

        manager.stop().await?;
        assert!(!workspace.config.pid_file_path.exists());
        assert!(!daemon_responds(report.capture_addr)?);

        Ok(())
    }

    #[tokio::test]
    async fn second_start_attempt_detects_existing_daemon() -> Result<(), Box<dyn Error>> {
        let workspace = TestWorkspace::new("duplicate-start")?;
        let mut first = workspace.manager();
        let report = first.start().await?;

        wait_for_health(report.capture_addr).await?;

        let mut second = workspace.manager();
        let error = match second.start().await {
            Ok(_) => return Err("second daemon start should fail".into()),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            DaemonError::AlreadyRunning {
                pid: Some(_),
                capture_addr,
            } if capture_addr == report.capture_addr
        ));

        first.stop().await?;

        Ok(())
    }

    #[tokio::test]
    async fn startup_processes_backlog_before_accepting_new_events() -> Result<(), Box<dyn Error>> {
        let workspace = TestWorkspace::new("backlog-startup")?;
        ensure_parent_dir(&workspace.config.backlog_path)?;
        fs::write(
            &workspace.config.backlog_path,
            format!("{}\n", sample_backlog_event()),
        )?;

        let mut manager = workspace.manager();
        let report = manager.start().await?;

        wait_for_health(report.capture_addr).await?;

        let database = Database::new(&workspace.config.database_path)?;
        let events = database.query_raw_events_by_session("session-backlog")?;

        assert_eq!(report.backlog_processed, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "Notification");
        assert_eq!(fs::metadata(&workspace.config.backlog_path)?.len(), 0);

        manager.stop().await?;

        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "process-wide SIGTERM terminates the cargo test harness"]
    async fn sigterm_causes_graceful_shutdown() -> Result<(), Box<dyn Error>> {
        let workspace = TestWorkspace::new("sigterm-shutdown")?;
        let mut manager = workspace.manager();
        let report = manager.start().await?;

        wait_for_health(report.capture_addr).await?;

        let status = Command::new("kill")
            .arg("-TERM")
            .arg(std::process::id().to_string())
            .status()?;
        assert!(status.success());

        timeout(Duration::from_secs(5), manager.wait_for_shutdown()).await??;
        assert!(!workspace.config.pid_file_path.exists());
        assert!(!daemon_responds(report.capture_addr)?);

        Ok(())
    }

    fn available_capture_addr() -> io::Result<SocketAddr> {
        let listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let addr = listener.local_addr()?;
        drop(listener);
        Ok(addr)
    }

    async fn wait_for_health(capture_addr: SocketAddr) -> Result<(), Box<dyn Error>> {
        for _ in 0..40 {
            if daemon_responds(capture_addr)? {
                return Ok(());
            }
            sleep(Duration::from_millis(50)).await;
        }

        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("timed out waiting for daemon health at {capture_addr}"),
        )
        .into())
    }

    fn sample_backlog_event() -> String {
        serde_json::json!({
            "cwd": "/workspace/claude-insight",
            "hook_event_name": "Notification",
            "message": "backlog replay",
            "notification_type": "info",
            "permission_mode": "acceptEdits",
            "session_id": "session-backlog",
            "title": "Claude Code notification",
            "transcript_path": "/workspace/.claude/projects/session-backlog.jsonl",
        })
        .to_string()
    }
}
