use crate::session_list::{
    SessionEvent, SessionListItem, BACKGROUND, BORDER, BORDER_ACTIVE, SURFACE, TEXT_DIM,
    TEXT_PRIMARY,
};
use crate::timeline::{format_timestamp, next_tool_index, previous_tool_index, TimelinePane};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    prelude::Frame,
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

#[cfg(test)]
use ratatui::{
    backend::TestBackend,
    prelude::{Buffer, Terminal},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayPane {
    Timeline,
    Transcript,
    Evidence,
}

impl ReplayPane {
    fn next(self) -> Self {
        match self {
            Self::Timeline => Self::Transcript,
            Self::Transcript => Self::Evidence,
            Self::Evidence => Self::Timeline,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Timeline => " Timeline [1] ",
            Self::Transcript => " Transcript [2] ",
            Self::Evidence => " Evidence [3] ",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplayLayoutMode {
    Wide,
    Medium,
    Narrow,
}

impl ReplayLayoutMode {
    fn from_width(width: u16) -> Self {
        if width > 160 {
            Self::Wide
        } else if width >= 80 {
            Self::Medium
        } else {
            Self::Narrow
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayViewState {
    pub session: SessionListItem,
    pub focus: ReplayPane,
    pub selected_event: usize,
    pub timeline_scroll: usize,
    pub evidence_overlay_open: bool,
}

impl ReplayViewState {
    pub fn from_session(session: SessionListItem) -> Self {
        Self {
            selected_event: session.event_count().saturating_sub(1),
            timeline_scroll: 0,
            session,
            focus: ReplayPane::Timeline,
            evidence_overlay_open: false,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session.session_id
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Tab => self.set_focus(self.focus.next()),
            KeyCode::Char('1') => self.set_focus(ReplayPane::Timeline),
            KeyCode::Char('2') => self.set_focus(ReplayPane::Transcript),
            KeyCode::Char('3') => self.set_focus(ReplayPane::Evidence),
            KeyCode::Enter => {
                if self.focus == ReplayPane::Timeline {
                    self.set_focus(ReplayPane::Evidence);
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.focus == ReplayPane::Timeline {
                    self.move_selection(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.focus == ReplayPane::Timeline {
                    self.move_selection(-1);
                }
            }
            KeyCode::Char('g') => {
                if self.focus == ReplayPane::Timeline {
                    self.jump_first();
                }
            }
            KeyCode::Char('G') => {
                if self.focus == ReplayPane::Timeline {
                    self.jump_last();
                }
            }
            KeyCode::Char('n') => {
                if self.focus == ReplayPane::Timeline {
                    self.jump_to_next_tool();
                }
            }
            KeyCode::Char('N') => {
                if self.focus == ReplayPane::Timeline {
                    self.jump_to_previous_tool();
                }
            }
            _ => {}
        }
    }

    pub fn current_event(&self) -> Option<&SessionEvent> {
        self.session.events.get(self.selected_event)
    }

    fn set_focus(&mut self, focus: ReplayPane) {
        self.focus = focus;
        self.evidence_overlay_open = focus == ReplayPane::Evidence;
    }

    fn move_selection(&mut self, delta: isize) {
        if self.session.events.is_empty() {
            self.selected_event = 0;
            return;
        }

        let next = self.selected_event as isize + delta;
        self.selected_event = next.clamp(0, self.session.events.len() as isize - 1) as usize;
    }

    fn jump_first(&mut self) {
        self.selected_event = 0;
    }

    fn jump_last(&mut self) {
        if !self.session.events.is_empty() {
            self.selected_event = self.session.events.len() - 1;
        }
    }

    fn jump_to_next_tool(&mut self) {
        if let Some(index) = next_tool_index(&self.session.events, self.selected_event) {
            self.selected_event = index;
        }
    }

    fn jump_to_previous_tool(&mut self) {
        if let Some(index) = previous_tool_index(&self.session.events, self.selected_event) {
            self.selected_event = index;
        }
    }

    fn sync_timeline_scroll(&mut self, visible_rows: usize) {
        if self.session.events.is_empty() || visible_rows == 0 {
            self.timeline_scroll = 0;
            return;
        }

        if self.selected_event < self.timeline_scroll {
            self.timeline_scroll = self.selected_event;
        } else if self.selected_event >= self.timeline_scroll + visible_rows {
            self.timeline_scroll = self.selected_event + 1 - visible_rows;
        }

        let max_scroll = self.session.events.len().saturating_sub(visible_rows);
        self.timeline_scroll = self.timeline_scroll.min(max_scroll);
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReplayView;

impl ReplayView {
    pub fn render(frame: &mut Frame<'_>, area: Rect, state: &mut ReplayViewState) {
        let mode = ReplayLayoutMode::from_width(area.width);
        let collapse_footer = area.height < 24;
        let block = Block::default()
            .title(Self::title_line(state, collapse_footer))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(BORDER))
            .style(Style::new().bg(BACKGROUND).fg(TEXT_PRIMARY));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let (body_area, footer_area) = if collapse_footer {
            (inner, None)
        } else {
            let [body, footer] = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .areas(inner);
            (body, Some(footer))
        };

        match mode {
            ReplayLayoutMode::Wide => Self::render_wide(frame, body_area, state),
            ReplayLayoutMode::Medium => Self::render_medium(frame, body_area, state),
            ReplayLayoutMode::Narrow => Self::render_narrow(frame, body_area, state),
        }

        if let Some(footer_area) = footer_area {
            frame.render_widget(Self::status_bar(state), footer_area);
        }
    }

    fn title_line(state: &ReplayViewState, collapse_footer: bool) -> Line<'static> {
        let short_id = short_session_id(&state.session.session_id);
        let title = if collapse_footer {
            format!(
                " Replay {short_id}  ${:.2}  {} tok  {} tools  {}  {} ",
                state.session.cost_usd(),
                estimated_token_count(&state.session),
                state.session.tool_count(),
                state.session.mood().emoji(),
                current_event_timestamp(state)
            )
        } else {
            format!(" Replay {short_id}  [Tab] focus  [1/2/3] jump  [Enter] evidence  [Esc] back ")
        };

        Line::from(title).bold()
    }

    fn render_wide(frame: &mut Frame<'_>, area: Rect, state: &mut ReplayViewState) {
        let [timeline, transcript, evidence] = Layout::horizontal([
            Constraint::Percentage(40),
            Constraint::Percentage(35),
            Constraint::Percentage(25),
        ])
        .areas(area);

        Self::render_timeline(frame, timeline, state);
        Self::render_transcript(frame, transcript, state);
        Self::render_evidence(frame, evidence, state);
    }

    fn render_medium(frame: &mut Frame<'_>, area: Rect, state: &mut ReplayViewState) {
        let [timeline, transcript] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(area);

        Self::render_timeline(frame, timeline, state);
        Self::render_transcript(frame, transcript, state);

        if state.evidence_overlay_open {
            let overlay = centered_rect(
                area.width.saturating_sub(8).clamp(28, 60),
                area.height.saturating_sub(6).clamp(10, 18),
                area,
            );
            frame.render_widget(Clear, overlay);
            Self::render_evidence(frame, overlay, state);
        }
    }

    fn render_narrow(frame: &mut Frame<'_>, area: Rect, state: &mut ReplayViewState) {
        match state.focus {
            ReplayPane::Timeline => Self::render_timeline(frame, area, state),
            ReplayPane::Transcript => Self::render_transcript(frame, area, state),
            ReplayPane::Evidence => Self::render_evidence(frame, area, state),
        }
    }

    fn render_timeline(frame: &mut Frame<'_>, area: Rect, state: &mut ReplayViewState) {
        state.sync_timeline_scroll(TimelinePane::visible_event_rows(area));
        TimelinePane::render(
            frame,
            area,
            &state.session,
            state.selected_event,
            state.timeline_scroll,
            state.focus == ReplayPane::Timeline,
        );
    }

    fn render_transcript(frame: &mut Frame<'_>, area: Rect, state: &ReplayViewState) {
        let block = pane_block(
            ReplayPane::Transcript.title(),
            state.focus == ReplayPane::Transcript,
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let lines = vec![
            Line::from(Span::styled(
                format!("session {}", short_session_id(&state.session.session_id)),
                Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("branch {}", state.session.git_branch),
                Style::new().fg(TEXT_DIM),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Transcript content lands in MOT-130.",
                Style::new().fg(TEXT_PRIMARY),
            )),
            Line::from(Span::styled(
                "This ticket wires layout, focus, and responsive behavior only.",
                Style::new().fg(TEXT_DIM),
            )),
        ];

        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::new().bg(SURFACE))
                .alignment(Alignment::Left),
            inner.inner(Margin::new(1, 0)),
        );
    }

    fn render_evidence(frame: &mut Frame<'_>, area: Rect, state: &ReplayViewState) {
        let block = pane_block(
            ReplayPane::Evidence.title(),
            state.focus == ReplayPane::Evidence,
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let lines = vec![
            Line::from(Span::styled(
                format!("event {}", state.selected_event.saturating_add(1)),
                Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                current_event_timestamp(state),
                Style::new().fg(TEXT_DIM),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Evidence details land in MOT-131.",
                Style::new().fg(TEXT_PRIMARY),
            )),
            Line::from(Span::styled(
                "Press Tab or 1/2/3 to switch panes.",
                Style::new().fg(TEXT_DIM),
            )),
            Line::from(Span::styled(
                "Press Esc to return to the session list.",
                Style::new().fg(TEXT_DIM),
            )),
        ];

        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::new().bg(SURFACE))
                .alignment(Alignment::Left),
            inner.inner(Margin::new(1, 0)),
        );
    }

    fn status_bar(state: &ReplayViewState) -> Paragraph<'static> {
        let mood = state.session.mood();
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", short_session_id(&state.session.session_id)),
                Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
            ),
            Span::styled("•", Style::new().fg(TEXT_DIM)),
            Span::styled(
                format!(" ${:.2} ", state.session.cost_usd()),
                Style::new().fg(TEXT_PRIMARY),
            ),
            Span::styled("•", Style::new().fg(TEXT_DIM)),
            Span::styled(
                format!(" {} tok ", estimated_token_count(&state.session)),
                Style::new().fg(TEXT_PRIMARY),
            ),
            Span::styled("•", Style::new().fg(TEXT_DIM)),
            Span::styled(
                format!(" {} tools ", state.session.tool_count()),
                Style::new().fg(TEXT_PRIMARY),
            ),
            Span::styled("•", Style::new().fg(TEXT_DIM)),
            Span::styled(
                format!(" {} {} ", mood.emoji(), mood.label()),
                mood.style().add_modifier(Modifier::BOLD),
            ),
            Span::styled("•", Style::new().fg(TEXT_DIM)),
            Span::styled(
                format!(" {} ", current_event_timestamp(state)),
                Style::new().fg(TEXT_DIM),
            ),
        ]))
        .style(Style::new().bg(BACKGROUND))
        .alignment(Alignment::Center)
    }
}

fn pane_block(title: &'static str, active: bool) -> Block<'static> {
    Block::default()
        .title(Line::from(title).bold())
        .borders(Borders::ALL)
        .border_style(Style::new().fg(if active { BORDER_ACTIVE } else { BORDER }))
        .style(Style::new().bg(SURFACE).fg(TEXT_PRIMARY))
}

fn short_session_id(session_id: &str) -> String {
    session_id.chars().take(8).collect()
}

fn estimated_token_count(session: &SessionListItem) -> usize {
    session.event_count() * 128
}

fn current_event_timestamp(state: &ReplayViewState) -> String {
    let timestamp = state
        .current_event()
        .map(|event| event.timestamp)
        .unwrap_or(state.session.last_updated);
    format_timestamp(timestamp)
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(height.min(area.height)),
            Constraint::Percentage(50),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(width.min(area.width)),
            Constraint::Percentage(50),
        ])
        .split(vertical[1]);
    horizontal[1]
}

#[cfg(test)]
fn render_replay(state: ReplayViewState, width: u16, height: u16) -> String {
    let mut backend = TestBackend::new(width, height);
    let terminal = Terminal::new(backend);
    let mut terminal = match terminal {
        Ok(terminal) => terminal,
        Err(error) => return format!("terminal error: {error}"),
    };

    let mut state = state;
    let draw_result = terminal.draw(|frame| ReplayView::render(frame, frame.area(), &mut state));
    if let Err(error) = draw_result {
        return format!("draw error: {error}");
    }

    backend = terminal.backend().clone();
    buffer_to_string(backend.buffer())
}

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};

    use super::*;
    use crate::session_list::SessionEventKind;
    use crate::widgets::mood_badge::Mood;

    #[test]
    fn replay_layout_180x50_snapshot() {
        insta::assert_snapshot!(render_replay(sample_state(), 180, 50));
    }

    #[test]
    fn replay_layout_120x40_snapshot() {
        insta::assert_snapshot!(render_replay(sample_state(), 120, 40));
    }

    #[test]
    fn replay_layout_60x30_snapshot() {
        insta::assert_snapshot!(render_replay(sample_state(), 60, 30));
    }

    #[test]
    fn replay_navigation_cycles_and_jumps_between_panes() {
        let mut state = sample_state();

        state.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(state.focus, ReplayPane::Transcript);

        state.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(state.focus, ReplayPane::Evidence);
        assert!(state.evidence_overlay_open);

        state.handle_key_event(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
        assert_eq!(state.focus, ReplayPane::Timeline);
        assert!(!state.evidence_overlay_open);

        state.handle_key_event(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        assert_eq!(state.focus, ReplayPane::Evidence);
        assert!(state.evidence_overlay_open);
    }

    #[test]
    fn replay_enter_opens_evidence_overlay() {
        let mut state = sample_state();

        state.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(state.focus, ReplayPane::Evidence);
        assert!(state.evidence_overlay_open);
    }

    #[test]
    fn replay_status_bar_uses_available_session_metadata() {
        let state = sample_state();

        assert_eq!(short_session_id(state.session_id()), "session-");
        assert_eq!(
            estimated_token_count(&state.session),
            state.session.event_count() * 128
        );
        assert_eq!(state.session.mood(), Mood::Errors);
    }

    #[test]
    fn replay_timeline_navigation_supports_first_last_and_tool_jumps() {
        let mut state = ReplayViewState::from_session(SessionListItem::new(
            "session-2",
            "feature/timeline-nav",
            parse_timestamp("2026-04-03T01:09:00Z"),
            0.11,
            vec![
                SessionEvent::new(
                    SessionEventKind::SessionBoundary,
                    parse_timestamp("2026-04-03T01:07:00Z"),
                ),
                SessionEvent::new(
                    SessionEventKind::UserPromptSubmit,
                    parse_timestamp("2026-04-03T01:07:05Z"),
                ),
                SessionEvent::tool("tool-1", parse_timestamp("2026-04-03T01:07:10Z")),
                SessionEvent::new(
                    SessionEventKind::InstructionsLoaded,
                    parse_timestamp("2026-04-03T01:07:15Z"),
                ),
                SessionEvent::tool("tool-2", parse_timestamp("2026-04-03T01:07:20Z")),
            ],
        ));

        state.handle_key_event(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(state.selected_event, 0);

        state.handle_key_event(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(state.selected_event, 2);

        state.handle_key_event(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(state.selected_event, 4);

        state.handle_key_event(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT));
        assert_eq!(state.selected_event, 2);

        state.handle_key_event(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(state.selected_event, 4);
    }

    #[test]
    fn replay_timeline_scroll_keeps_selection_visible() {
        let mut state = ReplayViewState::from_session(SessionListItem::new(
            "session-3",
            "feature/timeline-scroll",
            parse_timestamp("2026-04-03T01:09:00Z"),
            0.11,
            vec![
                SessionEvent::new(
                    SessionEventKind::SessionBoundary,
                    parse_timestamp("2026-04-03T01:07:00Z"),
                ),
                SessionEvent::new(
                    SessionEventKind::UserPromptSubmit,
                    parse_timestamp("2026-04-03T01:07:05Z"),
                ),
                SessionEvent::new(
                    SessionEventKind::InstructionsLoaded,
                    parse_timestamp("2026-04-03T01:07:10Z"),
                ),
                SessionEvent::tool("tool-1", parse_timestamp("2026-04-03T01:07:15Z")),
                SessionEvent::new(
                    SessionEventKind::PermissionDenied,
                    parse_timestamp("2026-04-03T01:07:20Z"),
                ),
            ],
        ));

        state.selected_event = 0;
        state.sync_timeline_scroll(2);
        assert_eq!(state.timeline_scroll, 0);

        state.selected_event = 3;
        state.sync_timeline_scroll(2);
        assert_eq!(state.timeline_scroll, 2);

        state.selected_event = 1;
        state.sync_timeline_scroll(2);
        assert_eq!(state.timeline_scroll, 1);
    }

    fn sample_state() -> ReplayViewState {
        ReplayViewState::from_session(SessionListItem::new(
            "session-1",
            "feature/replay-layout",
            parse_timestamp("2026-04-03T01:08:00Z"),
            0.42,
            vec![
                SessionEvent::tool("tool-1", parse_timestamp("2026-04-03T01:07:00Z")),
                SessionEvent::new(
                    SessionEventKind::PermissionRequest,
                    parse_timestamp("2026-04-03T01:07:05Z"),
                ),
                SessionEvent::new(
                    SessionEventKind::PermissionDenied,
                    parse_timestamp("2026-04-03T01:07:10Z"),
                ),
            ],
        ))
    }

    fn parse_timestamp(input: &str) -> OffsetDateTime {
        match OffsetDateTime::parse(input, &Rfc3339) {
            Ok(value) => value,
            Err(error) => panic!("failed to parse timestamp {input}: {error}"),
        }
    }
}
