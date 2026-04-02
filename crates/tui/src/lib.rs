#![deny(clippy::expect_used, clippy::unwrap_used)]

pub mod app;
pub mod replay;
pub mod session_list;
pub mod transcript;
pub mod widgets;

pub const CRATE_NAME: &str = "claude-insight-tui";

pub use app::{App, AppAction, AppView};
pub use replay::{ReplayPane, ReplayView, ReplayViewState};
pub use session_list::{
    render_session_list, MoodFilter, SessionEvent, SessionEventKind, SessionListItem,
    SessionListOverlay, SessionListView, SortOrder,
};
pub use widgets::{
    banner::{ansi_banner, banner_lines, banner_width, ASCII_BANNER},
    mood_badge::{compute_mood, render_mood_badge, Mood},
    progress_bar::ProgressBar,
    sparkline::compute_sparkline_data,
    spinner::BrailleSpinner,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiStub {
    pub title: String,
}

impl TuiStub {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
        }
    }

    pub fn title_line(&self) -> ratatui::text::Line<'_> {
        tracing::trace!("rendering placeholder title line");
        ratatui::text::Line::from(self.title.as_str())
    }

    pub fn placeholder_event() -> crossterm::event::Event {
        crossterm::event::Event::Resize(0, 0)
    }

    pub fn syntax_theme_count() -> usize {
        syntect::highlighting::ThemeSet::load_defaults()
            .themes
            .len()
    }

    pub fn sample_event() -> claude_insight_types::PlaceholderEvent {
        claude_insight_types::placeholder_event()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_line_contains_stub_title() {
        let stub = TuiStub::new("Claude Insight");

        assert_eq!(stub.title_line().to_string(), "Claude Insight");
    }
}
