use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{layout::Rect, prelude::Frame};

use crate::replay::{ReplayView, ReplayViewState};
use crate::session_list::{SessionListOverlay, SessionListView};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    None,
    Quit,
    OpenReplay { session_id: String },
    ReturnToSessionList,
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
}

impl App {
    pub fn new(session_list: SessionListView) -> Self {
        Self {
            session_list,
            view: AppView::SessionList,
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

    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect) {
        match &mut self.view {
            AppView::SessionList => self.session_list.render(frame, area),
            AppView::Replay(state) => ReplayView::render(frame, area, state),
        }
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
