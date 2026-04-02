use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{layout::Rect, prelude::Frame};
use std::time::Duration as StdDuration;

use crate::replay::{ReplayView, ReplayViewState};
use crate::session_list::{SessionListOverlay, SessionListView};
use crate::wizard::{HookInstallTarget, WizardCommand, WizardViewState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    None,
    Quit,
    OpenReplay { session_id: String },
    ReturnToSessionList,
    InitializeFirstRun { scope: HookInstallTarget },
    ImportExistingSessions { total: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppView {
    Wizard(WizardViewState),
    SessionList,
    Replay(ReplayViewState),
}

#[derive(Debug, Clone, PartialEq)]
pub struct App {
    session_list: SessionListView,
    view: AppView,
    should_quit: bool,
}

impl App {
    pub fn new(session_list: SessionListView) -> Self {
        Self::new_with_first_run(session_list, WizardViewState::should_launch())
    }

    pub fn new_with_first_run(session_list: SessionListView, first_run: bool) -> Self {
        let view = if first_run {
            AppView::Wizard(WizardViewState::new())
        } else {
            AppView::SessionList
        };

        Self {
            session_list,
            view,
            should_quit: false,
        }
    }

    pub fn session_list(&self) -> &SessionListView {
        &self.session_list
    }

    pub fn view(&self) -> &AppView {
        &self.view
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        match &self.view {
            AppView::Wizard(state) => state.render(frame, area),
            AppView::SessionList => self.session_list.render(frame, area),
            AppView::Replay(state) => ReplayView::render(frame, area, state),
        }
    }

    pub fn tick(&mut self, delta: StdDuration) {
        if let AppView::Wizard(state) = &mut self.view {
            state.tick(delta);
        }
    }

    pub fn wizard(&self) -> Option<&WizardViewState> {
        match &self.view {
            AppView::Wizard(state) => Some(state),
            AppView::SessionList | AppView::Replay(_) => None,
        }
    }

    pub fn on_daemon_started(&mut self, port: u16) {
        if let AppView::Wizard(state) = &mut self.view {
            let existing_sessions = state.discover_existing_sessions().unwrap_or_default();
            state.confirm_daemon_started(port, existing_sessions.len());
        }
    }

    pub fn on_daemon_started_with_existing_sessions(
        &mut self,
        port: u16,
        existing_sessions: usize,
    ) {
        if let AppView::Wizard(state) = &mut self.view {
            state.confirm_daemon_started(port, existing_sessions);
        }
    }

    pub fn on_backfill_progress(&mut self, current: usize, total: usize) {
        if let AppView::Wizard(state) = &mut self.view {
            state.update_backfill_progress(current, total);
        }
    }

    pub fn on_backfill_complete(&mut self) {
        if let AppView::Wizard(state) = &mut self.view {
            state.finish_backfill();
        }
    }

    pub fn on_first_session_start(&mut self) -> AppAction {
        if let AppView::Wizard(state) = &mut self.view {
            if state.handle_session_start() {
                self.session_list.clear_empty_state_message();
                self.view = AppView::SessionList;
                return AppAction::ReturnToSessionList;
            }
        }

        AppAction::None
    }

    pub fn handle_event(&mut self, event: Event) -> AppAction {
        match event {
            Event::Key(key) => self.handle_key_event(key),
            _ => AppAction::None,
        }
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> AppAction {
        if is_quit_key(key) {
            self.should_quit = true;
            return AppAction::Quit;
        }

        match &mut self.view {
            AppView::Wizard(state) => match state.handle_key_event(key) {
                WizardCommand::None => AppAction::None,
                WizardCommand::Initialize { scope } => AppAction::InitializeFirstRun { scope },
                WizardCommand::ImportExistingSessions { total } => {
                    AppAction::ImportExistingSessions { total }
                }
                WizardCommand::EnterSessionList { notice } => {
                    self.session_list.set_empty_state_message(notice);
                    self.view = AppView::SessionList;
                    AppAction::ReturnToSessionList
                }
            },
            AppView::SessionList => {
                if matches!(self.session_list.overlay(), SessionListOverlay::Search)
                    && matches!(key.code, KeyCode::Esc)
                {
                    self.session_list.close_overlay();
                    return AppAction::None;
                }

                if matches!(key.code, KeyCode::Enter)
                    && matches!(self.session_list.overlay(), SessionListOverlay::None)
                {
                    if let Some(session) = self.session_list.selected_session() {
                        let session_id = session.session_id.clone();
                        self.view = AppView::Replay(ReplayViewState::from_session(session.clone()));
                        return AppAction::OpenReplay { session_id };
                    }
                }

                self.session_list.handle_key_event(key);
                AppAction::None
            }
            AppView::Replay(state) => {
                if matches!(key.code, KeyCode::Esc) {
                    self.view = AppView::SessionList;
                    AppAction::ReturnToSessionList
                } else {
                    state.handle_key_event(key);
                    AppAction::None
                }
            }
        }
    }
}

fn is_quit_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q'))
        || (matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL))
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::time::Duration as StdDuration;

    use super::*;
    use crate::{
        session_list::{SessionEvent, SessionEventKind, SessionListItem},
        HookInstallTarget, WizardStep,
    };
    use time::OffsetDateTime;

    #[test]
    fn app_starts_on_session_list_view_when_database_exists() {
        let app = App::new_with_first_run(
            SessionListView::new(sample_sessions(), parse_timestamp("2026-04-03T01:10:00Z")),
            false,
        );

        assert_eq!(app.view(), &AppView::SessionList);
    }

    #[test]
    fn wizard_launches_when_database_is_missing() {
        let app = App::new_with_first_run(
            SessionListView::new(Vec::new(), parse_timestamp("2026-04-03T01:10:00Z")),
            true,
        );

        assert!(matches!(app.view(), AppView::Wizard(_)));
    }

    #[test]
    fn enter_opens_replay_for_selected_session() {
        let mut app = App::new_with_first_run(
            SessionListView::new(sample_sessions(), parse_timestamp("2026-04-03T01:10:00Z")),
            false,
        );

        let action = app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            action,
            AppAction::OpenReplay {
                session_id: "session-1".to_string(),
            }
        );
        assert_eq!(
            app.view(),
            &AppView::Replay(ReplayViewState::from_session(sample_sessions()[0].clone()))
        );
    }

    #[test]
    fn quit_key_sets_quit_flag() {
        let mut app = App::new_with_first_run(
            SessionListView::new(sample_sessions(), parse_timestamp("2026-04-03T01:10:00Z")),
            false,
        );

        let action = app.handle_key_event(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));

        assert_eq!(action, AppAction::Quit);
        assert!(app.should_quit());
    }

    #[test]
    fn wizard_skip_transitions_to_session_list_with_notice() {
        let mut app = App::new_with_first_run(
            SessionListView::new(Vec::new(), parse_timestamp("2026-04-03T01:10:00Z")),
            true,
        );

        let action = app.handle_key_event(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));

        assert_eq!(action, AppAction::ReturnToSessionList);
        assert_eq!(app.view(), &AppView::SessionList);
        assert_eq!(
            app.session_list().empty_state_message(),
            Some("No hooks installed. You can run `claude-insight init` later.")
        );
    }

    #[test]
    fn wizard_waiting_step_transitions_on_first_session_start() {
        let mut app = App::new_with_first_run(
            SessionListView::new(Vec::new(), parse_timestamp("2026-04-03T01:10:00Z")),
            true,
        );

        let action = app.handle_key_event(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(
            action,
            AppAction::InitializeFirstRun {
                scope: HookInstallTarget::Global,
            }
        );

        app.on_daemon_started_with_existing_sessions(4180, 0);
        app.tick(StdDuration::from_millis(900));

        let wizard = match app.wizard() {
            Some(state) => state,
            None => panic!("wizard should still be active"),
        };
        assert!(matches!(wizard.step(), WizardStep::WaitingForFirstSession));
        assert!(wizard.waiting_message().contains("..."));

        assert_eq!(app.on_first_session_start(), AppAction::ReturnToSessionList);
        assert_eq!(app.view(), &AppView::SessionList);
    }

    fn sample_sessions() -> Vec<SessionListItem> {
        vec![SessionListItem::new(
            "session-1",
            "main",
            parse_timestamp("2026-04-03T01:08:00Z"),
            0.42,
            vec![
                SessionEvent::tool("tool-1", parse_timestamp("2026-04-03T01:07:00Z")),
                SessionEvent::tool("tool-2", parse_timestamp("2026-04-03T01:07:05Z")),
                SessionEvent::new(
                    SessionEventKind::Other,
                    parse_timestamp("2026-04-03T01:08:00Z"),
                ),
            ],
        )]
    }

    fn parse_timestamp(input: &str) -> OffsetDateTime {
        match OffsetDateTime::parse(input, &time::format_description::well_known::Rfc3339) {
            Ok(value) => value,
            Err(error) => panic!("failed to parse timestamp {input}: {error}"),
        }
    }
}
