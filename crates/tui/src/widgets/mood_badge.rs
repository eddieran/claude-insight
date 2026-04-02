use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::session_list::{SessionEvent, SessionEventKind};

pub const MOOD_GREEN_COLOR: Color = Color::Rgb(0x3f, 0xb9, 0x50);
pub const MOOD_AMBER_COLOR: Color = Color::Rgb(0xd2, 0x99, 0x22);
pub const MOOD_RED_COLOR: Color = Color::Rgb(0xf8, 0x51, 0x49);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mood {
    Clean,
    Friction,
    Errors,
}

impl Mood {
    pub fn emoji(self) -> &'static str {
        match self {
            Self::Clean => "🟢",
            Self::Friction => "🟡",
            Self::Errors => "🔴",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Friction => "friction",
            Self::Errors => "errors",
        }
    }

    pub fn style(self) -> Style {
        match self {
            Self::Clean => Style::new().fg(MOOD_GREEN_COLOR),
            Self::Friction => Style::new().fg(MOOD_AMBER_COLOR),
            Self::Errors => Style::new().fg(MOOD_RED_COLOR),
        }
    }
}

pub fn compute_mood(events: &[SessionEvent]) -> Mood {
    if events.iter().any(|event| {
        matches!(
            event.kind,
            SessionEventKind::PermissionDenied
                | SessionEventKind::PostToolUseFailure
                | SessionEventKind::StopFailure
        )
    }) {
        Mood::Errors
    } else if events
        .iter()
        .filter(|event| event.kind == SessionEventKind::PermissionRequest)
        .count()
        > 2
        || events
            .iter()
            .filter(|event| event.kind == SessionEventKind::Retry)
            .count()
            > 1
    {
        Mood::Friction
    } else {
        Mood::Clean
    }
}

pub fn render_mood_badge(mood: Mood) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(mood.emoji(), mood.style()),
        Span::styled(format!(" {}", mood.label()), mood.style()),
    ])
}

#[cfg(test)]
mod animations_widgets_tests {
    use time::OffsetDateTime;

    use super::*;
    use crate::session_list::{SessionEvent, SessionEventKind};

    #[test]
    fn compute_mood_returns_green_for_clean_fixture() {
        let events = vec![
            event(SessionEventKind::Tool, "2026-04-03T01:07:00Z"),
            event(SessionEventKind::Other, "2026-04-03T01:07:05Z"),
        ];

        assert_eq!(compute_mood(&events), Mood::Clean);
    }

    #[test]
    fn compute_mood_returns_amber_for_friction_fixture() {
        let events = vec![
            event(SessionEventKind::PermissionRequest, "2026-04-03T01:07:00Z"),
            event(SessionEventKind::PermissionRequest, "2026-04-03T01:07:05Z"),
            event(SessionEventKind::Retry, "2026-04-03T01:07:10Z"),
            event(SessionEventKind::Retry, "2026-04-03T01:07:15Z"),
        ];

        assert_eq!(compute_mood(&events), Mood::Friction);
    }

    #[test]
    fn compute_mood_returns_red_for_error_fixture() {
        let events = vec![
            event(SessionEventKind::PermissionRequest, "2026-04-03T01:07:00Z"),
            event(SessionEventKind::StopFailure, "2026-04-03T01:07:05Z"),
        ];

        assert_eq!(compute_mood(&events), Mood::Errors);
    }

    #[test]
    fn mood_badge_renders_emoji_and_label() {
        let line = render_mood_badge(Mood::Clean);

        assert_eq!(line.to_string(), " 🟢 clean");
        assert_eq!(line.spans[1].style.fg, Some(MOOD_GREEN_COLOR));
    }

    fn event(kind: SessionEventKind, timestamp: &str) -> SessionEvent {
        SessionEvent::new(kind, parse_timestamp(timestamp))
    }

    fn parse_timestamp(input: &str) -> OffsetDateTime {
        match OffsetDateTime::parse(input, &time::format_description::well_known::Rfc3339) {
            Ok(value) => value,
            Err(error) => panic!("failed to parse timestamp {input}: {error}"),
        }
    }
}
