use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::Duration as StdDuration,
};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    backend::TestBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::{Buffer, Frame, Terminal},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::{
    session_list::{
        ACCENT_CYAN, ACCENT_GREEN, BACKGROUND, BORDER_ACTIVE, SURFACE, TEXT_DIM, TEXT_PRIMARY,
    },
    widgets::{banner::banner_lines, progress_bar::ProgressBar, spinner::BrailleSpinner},
};

const APP_STATE_DIR: &str = ".claude-insight";
const DATABASE_FILE_NAME: &str = "insight.db";
const TRANSCRIPT_ROOT_DIR: &str = ".claude/projects";
const GLOBAL_SETTINGS_PATH: &str = "~/.claude/settings.json";
const PROJECT_SETTINGS_PATH: &str = ".claude/settings.json";
const WAITING_DOT_INTERVAL_MS: u64 = 300;
const EMPTY_SKIP_MESSAGE: &str = "No hooks installed. You can run `claude-insight init` later.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookInstallTarget {
    Global,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WizardCommand {
    None,
    Initialize { scope: HookInstallTarget },
    ImportExistingSessions { total: usize },
    EnterSessionList { notice: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WizardStep {
    InstallHooks,
    StartingDaemon,
    BackfillPrompt { total: usize },
    BackfillProgress { current: usize, total: usize },
    WaitingForFirstSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardViewState {
    step: WizardStep,
    elapsed: StdDuration,
    daemon_port: Option<u16>,
}

impl Default for WizardViewState {
    fn default() -> Self {
        Self::new()
    }
}

impl WizardViewState {
    pub fn new() -> Self {
        Self {
            step: WizardStep::InstallHooks,
            elapsed: StdDuration::ZERO,
            daemon_port: None,
        }
    }

    pub fn should_launch() -> bool {
        Self::should_launch_at(default_database_path())
    }

    pub fn should_launch_at(database_path: impl AsRef<Path>) -> bool {
        !database_path.as_ref().exists()
    }

    pub fn step(&self) -> &WizardStep {
        &self.step
    }

    pub fn tick(&mut self, delta: StdDuration) {
        match self.step {
            WizardStep::StartingDaemon | WizardStep::WaitingForFirstSession => {
                self.elapsed = self.elapsed.saturating_add(delta);
            }
            WizardStep::InstallHooks
            | WizardStep::BackfillPrompt { .. }
            | WizardStep::BackfillProgress { .. } => {}
        }
    }

    pub fn discover_existing_sessions(&self) -> io::Result<Vec<PathBuf>> {
        Self::discover_existing_sessions_in(default_transcript_root())
    }

    pub fn discover_existing_sessions_in(root: impl AsRef<Path>) -> io::Result<Vec<PathBuf>> {
        let mut transcripts = Vec::new();
        collect_transcript_files(root.as_ref(), &mut transcripts)?;
        transcripts.sort();
        Ok(transcripts)
    }

    pub fn confirm_daemon_started(&mut self, port: u16, existing_sessions: usize) {
        self.daemon_port = Some(port);
        self.elapsed = StdDuration::ZERO;
        self.step = if existing_sessions > 0 {
            WizardStep::BackfillPrompt {
                total: existing_sessions,
            }
        } else {
            WizardStep::WaitingForFirstSession
        };
    }

    pub fn update_backfill_progress(&mut self, current: usize, total: usize) {
        self.step = WizardStep::BackfillProgress { current, total };
    }

    pub fn finish_backfill(&mut self) {
        self.elapsed = StdDuration::ZERO;
        self.step = WizardStep::WaitingForFirstSession;
    }

    pub fn handle_session_start(&mut self) -> bool {
        matches!(self.step, WizardStep::WaitingForFirstSession)
    }

    pub fn waiting_message(&self) -> String {
        let dot_count =
            ((self.elapsed.as_millis() / u128::from(WAITING_DOT_INTERVAL_MS)) % 4) as usize;
        format!("Waiting for first Claude session{}", ".".repeat(dot_count))
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> WizardCommand {
        match self.step {
            WizardStep::InstallHooks => match key.code {
                KeyCode::Char('g') | KeyCode::Char('G') => {
                    self.elapsed = StdDuration::ZERO;
                    self.step = WizardStep::StartingDaemon;
                    WizardCommand::Initialize {
                        scope: HookInstallTarget::Global,
                    }
                }
                KeyCode::Char('p') | KeyCode::Char('P') => {
                    self.elapsed = StdDuration::ZERO;
                    self.step = WizardStep::StartingDaemon;
                    WizardCommand::Initialize {
                        scope: HookInstallTarget::Project,
                    }
                }
                KeyCode::Char('s') | KeyCode::Char('S') => WizardCommand::EnterSessionList {
                    notice: EMPTY_SKIP_MESSAGE.to_string(),
                },
                _ => WizardCommand::None,
            },
            WizardStep::BackfillPrompt { total } => match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.step = WizardStep::BackfillProgress { current: 0, total };
                    WizardCommand::ImportExistingSessions { total }
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.elapsed = StdDuration::ZERO;
                    self.step = WizardStep::WaitingForFirstSession;
                    WizardCommand::None
                }
                _ => WizardCommand::None,
            },
            WizardStep::StartingDaemon
            | WizardStep::BackfillProgress { .. }
            | WizardStep::WaitingForFirstSession => WizardCommand::None,
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default()
            .title(Line::from(" CLAUDE INSIGHT ").bold())
            .borders(Borders::ALL)
            .border_style(Style::new().fg(BORDER_ACTIVE))
            .style(Style::new().bg(BACKGROUND).fg(TEXT_PRIMARY));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let [banner_area, content_area, footer_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length((banner_lines().len() as u16).saturating_add(2)),
                Constraint::Min(1),
                Constraint::Length(2),
            ])
            .areas(inner);

        frame.render_widget(
            Paragraph::new(banner_lines()).alignment(Alignment::Center),
            banner_area,
        );
        frame.render_widget(render_step_content(self), content_area);
        frame.render_widget(render_help(self), footer_area);
    }
}

pub fn render_wizard_step1(width: u16, height: u16) -> String {
    let mut backend = TestBackend::new(width, height);
    let terminal = Terminal::new(backend);
    let mut terminal = match terminal {
        Ok(terminal) => terminal,
        Err(error) => return format!("terminal error: {error}"),
    };
    let state = WizardViewState::new();

    let draw_result = terminal.draw(|frame| state.render(frame, frame.area()));
    if let Err(error) = draw_result {
        return format!("draw error: {error}");
    }

    backend = terminal.backend().clone();
    buffer_to_string(backend.buffer())
}

fn render_step_content(state: &WizardViewState) -> Paragraph<'static> {
    let mut lines = vec![Line::from(Span::styled(
        "First-run guided setup",
        Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
    ))];

    if let Some(port) = state.daemon_port {
        lines.push(Line::from(Span::styled(
            format!("Daemon running on port {port}"),
            Style::new().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::default());
    } else {
        lines.push(Line::default());
    }

    match state.step {
        WizardStep::InstallHooks => {
            lines.push(Line::from("Install hooks?"));
            lines.push(Line::from(vec![
                Span::styled(
                    "[G]",
                    Style::new().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("lobal ({GLOBAL_SETTINGS_PATH})")),
            ]));
            lines.push(Line::from(vec![
                Span::styled(
                    "[P]",
                    Style::new().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("roject ({PROJECT_SETTINGS_PATH})")),
            ]));
            lines.push(Line::from(vec![
                Span::styled(
                    "[S]",
                    Style::new().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
                ),
                Span::raw("kip"),
            ]));
        }
        WizardStep::StartingDaemon => {
            lines.push(BrailleSpinner::render("Starting daemon...", state.elapsed));
            lines.push(Line::from(Span::styled(
                "Waiting for port 4180 health check",
                Style::new().fg(TEXT_DIM),
            )));
        }
        WizardStep::BackfillPrompt { total } => {
            lines.push(Line::from(format!(
                "Found {total} existing sessions. Import? [Y/n]"
            )));
            lines.push(Line::from(Span::styled(
                "Import existing transcript JSONL files before waiting for the next live session.",
                Style::new().fg(TEXT_DIM),
            )));
        }
        WizardStep::BackfillProgress { current, total } => {
            lines.push(Line::from(format!(
                "Importing {total} existing sessions..."
            )));
            lines.push(
                ProgressBar::new(current as u64, total as u64)
                    .with_width(8)
                    .render(),
            );
        }
        WizardStep::WaitingForFirstSession => {
            lines.push(Line::from(state.waiting_message()));
            lines.push(Line::from(Span::styled(
                "Watching for the first SessionStart event to enter the session list.",
                Style::new().fg(TEXT_DIM),
            )));
        }
    }

    Paragraph::new(lines).alignment(Alignment::Center).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(ACCENT_CYAN))
            .style(Style::new().bg(SURFACE)),
    )
}

fn render_help(state: &WizardViewState) -> Paragraph<'static> {
    let message = match state.step {
        WizardStep::InstallHooks => "g global  p project  s skip",
        WizardStep::BackfillPrompt { .. } => "Enter/y import  n skip import",
        WizardStep::StartingDaemon
        | WizardStep::BackfillProgress { .. }
        | WizardStep::WaitingForFirstSession => "q quit",
    };

    Paragraph::new(message)
        .style(Style::new().fg(TEXT_DIM))
        .alignment(Alignment::Center)
}

fn default_home_root() -> PathBuf {
    if let Some(root) = env::var_os("CLAUDE_INSIGHT_HOME") {
        return PathBuf::from(root);
    }

    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home);
    }

    PathBuf::from(".")
}

fn default_database_path() -> PathBuf {
    default_home_root()
        .join(APP_STATE_DIR)
        .join(DATABASE_FILE_NAME)
}

fn default_transcript_root() -> PathBuf {
    default_home_root().join(TRANSCRIPT_ROOT_DIR)
}

fn collect_transcript_files(root: &Path, transcripts: &mut Vec<PathBuf>) -> io::Result<()> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_transcript_files(&path, transcripts)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            transcripts.push(path);
        }
    }

    Ok(())
}

fn buffer_to_string(buffer: &Buffer) -> String {
    let mut view = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            view.push_str(buffer[(x, y)].symbol());
        }
        view.push('\n');
    }
    view
}

#[cfg(test)]
mod tests {
    use std::time::Duration as StdDuration;

    use super::*;

    #[test]
    fn render_wizard_step1_snapshot() {
        insta::assert_snapshot!(render_wizard_step1(120, 40));
    }

    #[test]
    fn wizard_enters_starting_daemon_step_after_install_choice() {
        let mut wizard = WizardViewState::new();

        let command = wizard.handle_key_event(KeyEvent::from(KeyCode::Char('g')));

        assert_eq!(
            command,
            WizardCommand::Initialize {
                scope: HookInstallTarget::Global,
            }
        );
        assert_eq!(wizard.step(), &WizardStep::StartingDaemon);
    }

    #[test]
    fn wizard_prompts_for_import_when_existing_sessions_are_found() {
        let root = temp_root("prompt");
        let transcript_root = root.join(TRANSCRIPT_ROOT_DIR).join("wizard-test");
        let _ = fs::create_dir_all(&transcript_root);
        for index in 0..3 {
            let _ = fs::write(
                transcript_root.join(format!("session-{index}.jsonl")),
                "{\"type\":\"assistant\",\"message\":\"hello\"}\n",
            );
        }
        let mut wizard = WizardViewState::new();
        wizard.handle_key_event(KeyEvent::from(KeyCode::Char('g')));

        let existing =
            WizardViewState::discover_existing_sessions_in(&root.join(TRANSCRIPT_ROOT_DIR))
                .unwrap_or_default();
        wizard.confirm_daemon_started(4180, existing.len());

        assert_eq!(wizard.step(), &WizardStep::BackfillPrompt { total: 3 });
    }

    #[test]
    fn wizard_import_progress_renders_expected_bar() {
        let mut wizard = WizardViewState::new();
        wizard.confirm_daemon_started(4180, 4);

        let command = wizard.handle_key_event(KeyEvent::from(KeyCode::Char('y')));
        assert_eq!(command, WizardCommand::ImportExistingSessions { total: 4 });

        wizard.update_backfill_progress(2, 4);

        let rendered = render_state(&wizard, 80, 20);
        assert!(rendered.contains("[████░░░░] 2/4"));
    }

    #[test]
    fn wizard_waiting_message_animates_and_session_start_can_finish() {
        let mut wizard = WizardViewState::new();
        wizard.confirm_daemon_started(4180, 0);
        wizard.tick(StdDuration::from_millis(900));

        assert!(wizard.waiting_message().ends_with("..."));
        assert!(wizard.handle_session_start());
    }

    #[test]
    fn wizard_should_launch_when_default_database_is_missing() {
        let root = temp_root("missing-db");
        assert!(WizardViewState::should_launch_at(
            root.join(APP_STATE_DIR).join(DATABASE_FILE_NAME)
        ));
    }

    #[test]
    fn wizard_should_not_launch_when_default_database_exists() {
        let root = temp_root("existing-db");
        let database_path = root.join(APP_STATE_DIR).join(DATABASE_FILE_NAME);
        let _ = fs::create_dir_all(root.join(APP_STATE_DIR));
        let _ = fs::write(&database_path, "");
        assert!(!WizardViewState::should_launch_at(database_path));
    }

    fn render_state(state: &WizardViewState, width: u16, height: u16) -> String {
        let mut backend = TestBackend::new(width, height);
        let terminal = Terminal::new(backend);
        let mut terminal = match terminal {
            Ok(terminal) => terminal,
            Err(error) => panic!("terminal error: {error}"),
        };

        if let Err(error) = terminal.draw(|frame| state.render(frame, frame.area())) {
            panic!("draw error: {error}");
        }

        backend = terminal.backend().clone();
        buffer_to_string(backend.buffer())
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = format!(
            "claude-insight-tui-wizard-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default()
        );
        let root = env::temp_dir().join(unique);
        let _ = fs::remove_dir_all(&root);
        root
    }
}
