use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{layout::Rect, prelude::Frame};

use crate::{
    keyboard::render_help_overlay_widget,
    replay::{ReplayAction, ReplayView, ReplayViewState},
    session_list::{SessionListOverlay, SessionListView},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    None,
    Quit,
    OpenReplay { session_id: String },
    ReturnToSessionList,
    CopyEvidenceJson(String),
    OpenFileInEditor { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppView {
    SessionList,
    Replay(ReplayViewState),
}

#[derive(Debug, Clone, PartialEq)]
pub struct App {
    session_list: SessionListView,
    view: AppView,
    should_quit: bool,
    help_overlay_open: bool,
}

impl App {
    pub fn new(session_list: SessionListView) -> Self {
        Self {
            session_list,
            view: AppView::SessionList,
            should_quit: false,
            help_overlay_open: false,
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

    pub fn help_overlay_open(&self) -> bool {
        self.help_overlay_open
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        match &self.view {
            AppView::SessionList => self.session_list.render(frame, area),
            AppView::Replay(state) => ReplayView::render(frame, area, state),
        }

        if self.help_overlay_open {
            render_help_overlay_widget(frame, area);
        }
    }

    pub fn handle_event(&mut self, event: Event) -> AppAction {
        self.handle_event_in_area(event, Rect::default())
    }

    pub fn handle_event_in_area(&mut self, event: Event, area: Rect) -> AppAction {
        match event {
            Event::Key(key) => self.handle_key_event(key),
            Event::Mouse(mouse) => match &mut self.view {
                AppView::Replay(state) if !self.help_overlay_open => {
                    map_replay_action(state.handle_mouse_event(mouse, area))
                }
                _ => AppAction::None,
            },
            _ => AppAction::None,
        }
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> AppAction {
        if is_quit_key(key) {
            self.should_quit = true;
            return AppAction::Quit;
        }

        if matches!(key.code, KeyCode::Char('?')) {
            self.help_overlay_open = !self.help_overlay_open;
            return AppAction::None;
        }

        if self.help_overlay_open {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                self.help_overlay_open = false;
            }
            return AppAction::None;
        }

        match &mut self.view {
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
                if state.search_overlay.is_none() && matches!(key.code, KeyCode::Esc) {
                    self.view = AppView::SessionList;
                    return AppAction::ReturnToSessionList;
                }

                map_replay_action(state.handle_key_event(key))
            }
        }
    }
}

fn map_replay_action(action: ReplayAction) -> AppAction {
    match action {
        ReplayAction::None => AppAction::None,
        ReplayAction::CopyEvidenceJson(text) => AppAction::CopyEvidenceJson(text),
        ReplayAction::OpenFileInEditor { path } => AppAction::OpenFileInEditor { path },
    }
}

fn is_quit_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q'))
        || (matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL))
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::session_list::{SessionEvent, SessionEventKind, SessionListItem};
    use time::OffsetDateTime;

    #[test]
    fn app_starts_on_session_list_view() {
        let app = App::new(SessionListView::new(
            sample_sessions(),
            parse_timestamp("2026-04-03T01:10:00Z"),
        ));

        assert_eq!(app.view(), &AppView::SessionList);
    }

    #[test]
    fn enter_opens_replay_for_selected_session() {
        let mut app = App::new(SessionListView::new(
            sample_sessions(),
            parse_timestamp("2026-04-03T01:10:00Z"),
        ));

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
        let mut app = App::new(SessionListView::new(
            sample_sessions(),
            parse_timestamp("2026-04-03T01:10:00Z"),
        ));

        let action = app.handle_key_event(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));

        assert_eq!(action, AppAction::Quit);
        assert!(app.should_quit());
    }

    #[test]
    fn help_key_toggles_global_overlay() {
        let mut app = App::new(SessionListView::new(
            sample_sessions(),
            parse_timestamp("2026-04-03T01:10:00Z"),
        ));

        app.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert!(app.help_overlay_open());

        app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.help_overlay_open());
    }

    #[test]
    fn escape_in_replay_returns_to_session_list_when_no_overlay_is_open() {
        let mut app = App::new(SessionListView::new(
            sample_sessions(),
            parse_timestamp("2026-04-03T01:10:00Z"),
        ));
        app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let action = app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(action, AppAction::ReturnToSessionList);
        assert_eq!(app.view(), &AppView::SessionList);
    }

    fn sample_sessions() -> Vec<SessionListItem> {
        vec![SessionListItem::new(
            "session-1",
            "main",
            parse_timestamp("2026-04-03T01:08:00Z"),
            0.42,
            vec![
                SessionEvent::new(
                    SessionEventKind::Other,
                    parse_timestamp("2026-04-03T01:07:00Z"),
                )
                .with_event_type("UserPromptSubmit")
                .with_prompt_id("prompt-1")
                .with_content("Inspect src/lib.rs"),
                SessionEvent::tool("tool-1", parse_timestamp("2026-04-03T01:07:05Z"))
                    .with_tool_name("Bash")
                    .with_prompt_id("prompt-1")
                    .with_content("cat src/lib.rs")
                    .with_file_path("src/lib.rs"),
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
