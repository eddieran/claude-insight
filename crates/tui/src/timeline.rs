use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    prelude::Frame,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::causal_chain::CausalChainState;
use crate::session_list::{
    SessionEvent, SessionEventKind, SessionListItem, ACCENT_AMBER, ACCENT_CYAN, ACCENT_GREEN,
    ACCENT_RED, BORDER, BORDER_ACTIVE, SURFACE, TEXT_DIM, TEXT_PRIMARY,
};
use crate::widgets::sparkline::activity_sparkline;

const ACCENT_PURPLE: Color = Color::Rgb(0xbc, 0x8c, 0xff);
const SELECTED_TEXT: Color = Color::White;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TimelinePane;

impl TimelinePane {
    pub fn visible_event_rows(area: Rect) -> usize {
        let inner_height = area.height.saturating_sub(2);
        let list_height = if inner_height > 1 {
            inner_height.saturating_sub(1)
        } else {
            inner_height
        };

        list_height.max(1) as usize
    }

    pub fn render(
        frame: &mut Frame<'_>,
        area: Rect,
        session: &SessionListItem,
        selected_event: usize,
        scroll: usize,
        active: bool,
        causal_chain: Option<&CausalChainState>,
    ) {
        let block = pane_block(" Timeline [1] ", active);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let show_sparkline = inner.height > 1;
        let (list_area, sparkline_area) = if show_sparkline {
            let [list, sparkline] = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .areas(inner);
            (list, Some(sparkline))
        } else {
            (inner, None)
        };

        let list_inner = list_area.inner(Margin::new(1, 0));
        let lines = if session.events.is_empty() {
            vec![Line::from(Span::styled(
                "No events in this session yet.",
                Style::new().fg(TEXT_DIM),
            ))]
        } else {
            let total_events = session.events.len();
            let selected = selected_event.min(total_events - 1);
            let max_rows = list_inner.height.max(1) as usize;
            let start = scroll.min(total_events.saturating_sub(max_rows));
            let end = (start + max_rows).min(total_events);

            session.events[start..end]
                .iter()
                .enumerate()
                .map(|(offset, event)| {
                    let event_index = start + offset;
                    render_event_line(
                        event,
                        event_index == selected,
                        causal_chain.and_then(|chain| chain.highlight_for(event_index)),
                    )
                })
                .collect()
        };

        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::new().bg(SURFACE))
                .block(Block::default()),
            list_inner,
        );

        if let Some(sparkline_area) = sparkline_area {
            let activity = session.activity_buckets();
            let sparkline = activity_sparkline(&activity);
            frame.render_widget(sparkline, sparkline_area.inner(Margin::new(1, 0)));
        }
    }
}

pub fn format_timestamp(timestamp: OffsetDateTime) -> String {
    match timestamp.format(&Rfc3339) {
        Ok(value) => value,
        Err(_) => timestamp.unix_timestamp().to_string(),
    }
}

pub fn next_tool_index(events: &[SessionEvent], selected: usize) -> Option<usize> {
    events
        .iter()
        .enumerate()
        .skip(selected.saturating_add(1))
        .find(|(_, event)| event.kind.is_tool_call())
        .map(|(index, _)| index)
}

pub fn previous_tool_index(events: &[SessionEvent], selected: usize) -> Option<usize> {
    events
        .iter()
        .enumerate()
        .take(selected)
        .rev()
        .find(|(_, event)| event.kind.is_tool_call())
        .map(|(index, _)| index)
}

fn render_event_line(
    event: &SessionEvent,
    selected: bool,
    relation: Option<crate::causal_chain::CausalRelation>,
) -> Line<'static> {
    if selected && relation.is_none() {
        return Line::from(vec![
            Span::styled(
                format_timestamp(event.timestamp),
                Style::new()
                    .bg(ACCENT_CYAN)
                    .fg(SELECTED_TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::new().bg(ACCENT_CYAN).fg(SELECTED_TEXT)),
            Span::styled(
                event_emoji(event.kind),
                Style::new()
                    .bg(ACCENT_CYAN)
                    .fg(SELECTED_TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::new().bg(ACCENT_CYAN).fg(SELECTED_TEXT)),
            Span::styled(
                event.label.clone(),
                Style::new()
                    .bg(ACCENT_CYAN)
                    .fg(SELECTED_TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
    }

    let timestamp_style = relation
        .map(|relation| relation.apply_to(Style::new().fg(TEXT_DIM)))
        .unwrap_or_else(|| Style::new().fg(TEXT_DIM));
    let emoji_style = relation
        .map(|relation| relation.apply_to(Style::new().fg(event_color(event.kind))))
        .unwrap_or_else(|| Style::new().fg(event_color(event.kind)));
    let label_style = relation
        .map(|relation| relation.apply_to(Style::new().fg(TEXT_PRIMARY)))
        .unwrap_or_else(|| Style::new().fg(TEXT_PRIMARY));
    let spacer_style = relation
        .map(|relation| relation.apply_to(Style::new()))
        .unwrap_or_default();

    Line::from(vec![
        Span::styled(format_timestamp(event.timestamp), timestamp_style),
        Span::styled(" ", spacer_style),
        Span::styled(event_emoji(event.kind), emoji_style),
        Span::styled(" ", spacer_style),
        Span::styled(event.label.clone(), label_style),
    ])
}

fn event_emoji(kind: SessionEventKind) -> &'static str {
    match kind {
        SessionEventKind::SessionBoundary => "📋",
        SessionEventKind::UserPromptSubmit => "💬",
        SessionEventKind::InstructionsLoaded => "📖",
        SessionEventKind::Subagent => "🤖",
        SessionEventKind::Other => "📋",
        SessionEventKind::Tool => "🔧",
        SessionEventKind::PermissionRequest => "🛡️",
        SessionEventKind::Retry => "🔧",
        SessionEventKind::PermissionDenied => "🚫",
        SessionEventKind::PostToolUseFailure | SessionEventKind::StopFailure => "⚠️",
    }
}

fn event_color(kind: SessionEventKind) -> Color {
    match kind {
        SessionEventKind::PermissionRequest => ACCENT_GREEN,
        SessionEventKind::PermissionDenied
        | SessionEventKind::PostToolUseFailure
        | SessionEventKind::StopFailure => ACCENT_RED,
        SessionEventKind::InstructionsLoaded | SessionEventKind::Subagent => ACCENT_PURPLE,
        SessionEventKind::Retry => ACCENT_AMBER,
        SessionEventKind::Tool | SessionEventKind::UserPromptSubmit => ACCENT_CYAN,
        SessionEventKind::SessionBoundary | SessionEventKind::Other => TEXT_DIM,
    }
}

fn pane_block(title: &'static str, active: bool) -> Block<'static> {
    Block::default()
        .title(Line::from(title).bold())
        .borders(Borders::ALL)
        .border_style(Style::new().fg(if active { BORDER_ACTIVE } else { BORDER }))
        .style(Style::new().bg(SURFACE).fg(TEXT_PRIMARY))
}

#[cfg(test)]
mod tests {
    use ratatui::{
        backend::TestBackend,
        prelude::{Buffer, Terminal},
    };

    use super::*;

    #[test]
    fn timeline_event_emoji_matches_engineering_map() {
        assert_eq!(event_emoji(SessionEventKind::Tool), "🔧");
        assert_eq!(event_emoji(SessionEventKind::PermissionRequest), "🛡️");
        assert_eq!(event_emoji(SessionEventKind::PermissionDenied), "🚫");
        assert_eq!(event_emoji(SessionEventKind::SessionBoundary), "📋");
        assert_eq!(event_emoji(SessionEventKind::UserPromptSubmit), "💬");
        assert_eq!(event_emoji(SessionEventKind::InstructionsLoaded), "📖");
        assert_eq!(event_emoji(SessionEventKind::Subagent), "🤖");
        assert_eq!(event_emoji(SessionEventKind::StopFailure), "⚠️");
    }

    #[test]
    fn timeline_tool_jump_helpers_skip_non_tool_events() {
        let session = sample_session();

        assert_eq!(next_tool_index(&session.events, 0), Some(3));
        assert_eq!(previous_tool_index(&session.events, 4), Some(3));
        assert_eq!(next_tool_index(&session.events, 3), None);
    }

    #[test]
    fn timeline_causal_chain_styles_all_five_events() {
        let timestamps = [
            parse_timestamp("2026-04-03T01:06:55Z"),
            parse_timestamp("2026-04-03T01:07:00Z"),
            parse_timestamp("2026-04-03T01:07:05Z"),
            parse_timestamp("2026-04-03T01:07:10Z"),
            parse_timestamp("2026-04-03T01:07:15Z"),
        ];
        let session = SessionListItem::new(
            "session-1",
            "feature/timeline",
            timestamps[4],
            0.42,
            vec![
                SessionEvent::named(
                    SessionEventKind::UserPromptSubmit,
                    "UserPromptSubmit",
                    timestamps[0],
                )
                .with_raw_event_id(1),
                SessionEvent::named(
                    SessionEventKind::InstructionsLoaded,
                    "InstructionsLoaded",
                    timestamps[1],
                )
                .with_raw_event_id(2),
                SessionEvent::named(SessionEventKind::Tool, "PreToolUse", timestamps[2])
                    .with_raw_event_id(3),
                SessionEvent::named(
                    SessionEventKind::PermissionRequest,
                    "PermissionRequest",
                    timestamps[3],
                )
                .with_raw_event_id(4),
                SessionEvent::named(SessionEventKind::Tool, "PostToolUse", timestamps[4])
                    .with_raw_event_id(5),
            ],
        );
        let links = [
            crate::causal_chain::CausalLink::new(1, 2),
            crate::causal_chain::CausalLink::new(2, 3),
            crate::causal_chain::CausalLink::new(3, 4),
            crate::causal_chain::CausalLink::new(4, 5),
        ];
        let mut chain = crate::causal_chain::CausalChainState::activate(2, &session.events, &links);
        chain.reveal_all_for_test();

        for index in 0..session.events.len() {
            let line = render_event_line(
                &session.events[index],
                index == 2,
                chain.highlight_for(index),
            );
            assert!(
                line.spans.iter().any(|span| span.style.bg.is_some()),
                "event {index} should have a causal highlight background"
            );
        }
    }

    #[test]
    fn timeline_render_includes_label_and_activity_sparkline() {
        let mut backend = TestBackend::new(80, 8);
        let terminal = Terminal::new(backend);
        let mut terminal = match terminal {
            Ok(terminal) => terminal,
            Err(error) => panic!("terminal init failed: {error}"),
        };
        let session = sample_session();

        let draw_result = terminal.draw(|frame| {
            TimelinePane::render(frame, frame.area(), &session, 3, 0, true, None);
        });
        if let Err(error) = draw_result {
            panic!("draw failed: {error}");
        }

        backend = terminal.backend().clone();
        let view = buffer_to_string(backend.buffer());

        assert!(view.contains("Tool call"));
        assert!(view.contains("Permission denied"));
        assert!(
            view.contains("▁") || view.contains("▂") || view.contains("▇") || view.contains("█")
        );
    }

    fn sample_session() -> SessionListItem {
        SessionListItem::new(
            "session-1",
            "feature/timeline",
            parse_timestamp("2026-04-03T01:08:00Z"),
            0.42,
            vec![
                SessionEvent::new(
                    SessionEventKind::SessionBoundary,
                    parse_timestamp("2026-04-03T01:06:55Z"),
                )
                .with_label("Session start"),
                SessionEvent::new(
                    SessionEventKind::UserPromptSubmit,
                    parse_timestamp("2026-04-03T01:07:00Z"),
                ),
                SessionEvent::new(
                    SessionEventKind::InstructionsLoaded,
                    parse_timestamp("2026-04-03T01:07:05Z"),
                ),
                SessionEvent::tool("tool-1", parse_timestamp("2026-04-03T01:07:10Z")),
                SessionEvent::new(
                    SessionEventKind::PermissionDenied,
                    parse_timestamp("2026-04-03T01:07:15Z"),
                ),
            ],
        )
    }

    fn buffer_to_string(buffer: &Buffer) -> String {
        let mut view = String::new();
        for y in 0..buffer.area.height {
            let mut skip = 0usize;
            for x in 0..buffer.area.width {
                let cell = &buffer[(x, y)];
                let symbol = cell.symbol();
                if skip == 0 {
                    view.push_str(symbol);
                }
                skip = std::cmp::max(skip, unicode_width::UnicodeWidthStr::width(symbol))
                    .saturating_sub(1);
            }
            view.push('\n');
        }
        view
    }

    fn parse_timestamp(input: &str) -> OffsetDateTime {
        match OffsetDateTime::parse(input, &Rfc3339) {
            Ok(value) => value,
            Err(error) => panic!("failed to parse timestamp {input}: {error}"),
        }
    }
}
