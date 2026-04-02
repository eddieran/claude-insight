use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    prelude::Frame,
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    search_overlay::{render_search_overlay_widget, SearchOverlayAction, SearchOverlayState},
    session_list::{
        SessionEvent, SessionEventKind, SessionListItem, BACKGROUND, BORDER, BORDER_ACTIVE,
        SURFACE, TEXT_DIM, TEXT_PRIMARY,
    },
};

#[cfg(test)]
use ratatui::{
    backend::TestBackend,
    prelude::{Buffer, Terminal},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayAction {
    None,
    CopyEvidenceJson(String),
    OpenFileInEditor { path: String },
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneAreas {
    timeline: Rect,
    transcript: Rect,
    evidence: Option<Rect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayViewState {
    pub session: SessionListItem,
    pub focus: ReplayPane,
    pub selected_event: usize,
    pub evidence_overlay_open: bool,
    pub evidence_scroll: usize,
    pub causal_chain_highlight: bool,
    pub transcript_expanded: bool,
    pub linked_events_open: bool,
    pub search_overlay: Option<SearchOverlayState>,
    pub last_evidence_status: Option<String>,
}

impl ReplayViewState {
    pub fn from_session(session: SessionListItem) -> Self {
        Self {
            selected_event: session.event_count().saturating_sub(1),
            session,
            focus: ReplayPane::Timeline,
            evidence_overlay_open: false,
            evidence_scroll: 0,
            causal_chain_highlight: false,
            transcript_expanded: false,
            linked_events_open: false,
            search_overlay: None,
            last_evidence_status: None,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session.session_id
    }

    pub fn current_event(&self) -> Option<&SessionEvent> {
        self.session.events.get(self.selected_event)
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> ReplayAction {
        if let Some(search) = &mut self.search_overlay {
            let action = search.handle_key_event(key, &self.session);
            if let Some(index) = search.selected_index(&self.session) {
                self.selected_event = index;
            }
            return match action {
                SearchOverlayAction::None => ReplayAction::None,
                SearchOverlayAction::Close => {
                    self.close_search_overlay();
                    ReplayAction::None
                }
                SearchOverlayAction::Submit => {
                    self.close_search_overlay();
                    self.set_focus(ReplayPane::Timeline);
                    ReplayAction::None
                }
            };
        }

        match key.code {
            KeyCode::Tab => self.set_focus(self.focus.next()),
            KeyCode::Char('1') => self.set_focus(ReplayPane::Timeline),
            KeyCode::Char('2') => self.set_focus(ReplayPane::Transcript),
            KeyCode::Char('3') => self.set_focus(ReplayPane::Evidence),
            KeyCode::Enter => self.set_focus(ReplayPane::Evidence),
            KeyCode::Char('/') => self.open_search_overlay(),
            KeyCode::Char('j') | KeyCode::Down => self.navigate_forward(),
            KeyCode::Char('k') | KeyCode::Up => self.navigate_backward(),
            KeyCode::Char('c') => self.causal_chain_highlight = !self.causal_chain_highlight,
            KeyCode::Char('e') => self.transcript_expanded = !self.transcript_expanded,
            KeyCode::Char('p') => self.jump_to_parent_prompt(),
            KeyCode::Char('n') => self.jump_to_tool(1),
            KeyCode::Char('N') => self.jump_to_tool(-1),
            KeyCode::Char('[') => self.jump_to_prompt_boundary(-1),
            KeyCode::Char(']') => self.jump_to_prompt_boundary(1),
            KeyCode::Char('g') => self.select_event(0),
            KeyCode::Char('G') => {
                let last = self.session.events.len().saturating_sub(1);
                self.select_event(last);
            }
            KeyCode::Char('y') if self.focus == ReplayPane::Evidence => {
                if let Some(event) = self.current_event() {
                    return ReplayAction::CopyEvidenceJson(event.raw_json_text());
                }
            }
            KeyCode::Char('o') if self.focus == ReplayPane::Evidence => {
                if let Some(path) = self
                    .current_event()
                    .and_then(|event| event.file_path.clone())
                {
                    return ReplayAction::OpenFileInEditor { path };
                }
                self.last_evidence_status = Some("Selected event has no file path.".to_owned());
            }
            KeyCode::Char('l') if self.focus == ReplayPane::Evidence => {
                self.linked_events_open = !self.linked_events_open;
            }
            _ => {}
        }

        ReplayAction::None
    }

    pub fn handle_mouse_event(&mut self, mouse: MouseEvent, area: Rect) -> ReplayAction {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return ReplayAction::None;
        }

        let pane_areas = visible_pane_areas(area, self);
        if let Some(index) = hit_test_timeline(mouse, pane_areas.timeline, self) {
            self.set_focus(ReplayPane::Timeline);
            self.select_event(index);
            return ReplayAction::None;
        }

        if let Some(index) = hit_test_transcript(mouse, pane_areas.transcript, self) {
            self.set_focus(ReplayPane::Transcript);
            self.select_event(index);
        }

        ReplayAction::None
    }

    fn navigate_forward(&mut self) {
        if self.focus == ReplayPane::Evidence {
            self.evidence_scroll = self.evidence_scroll.saturating_add(1);
        } else {
            self.move_selection(1);
        }
    }

    fn navigate_backward(&mut self) {
        if self.focus == ReplayPane::Evidence {
            self.evidence_scroll = self.evidence_scroll.saturating_sub(1);
        } else {
            self.move_selection(-1);
        }
    }

    fn open_search_overlay(&mut self) {
        self.search_overlay = Some(SearchOverlayState::new(self.focus, self.selected_event));
    }

    fn close_search_overlay(&mut self) {
        if let Some(search) = &self.search_overlay {
            self.set_focus(search.previous_focus());
        }
        self.search_overlay = None;
    }

    fn set_focus(&mut self, focus: ReplayPane) {
        self.focus = focus;
        self.evidence_overlay_open = focus == ReplayPane::Evidence;
    }

    fn select_event(&mut self, index: usize) {
        if self.session.events.is_empty() {
            self.selected_event = 0;
            return;
        }

        self.selected_event = index.min(self.session.events.len() - 1);
        self.evidence_scroll = 0;
        self.last_evidence_status = None;
    }

    fn move_selection(&mut self, delta: isize) {
        if self.session.events.is_empty() {
            self.selected_event = 0;
            return;
        }

        let next = self.selected_event as isize + delta;
        self.select_event(next.clamp(0, self.session.events.len() as isize - 1) as usize);
    }

    fn jump_to_tool(&mut self, direction: isize) {
        if self.session.events.is_empty() {
            return;
        }

        let start = self.selected_event as isize + direction.signum();
        let mut index = start;
        while index >= 0 && index < self.session.events.len() as isize {
            if self.session.events[index as usize].kind == SessionEventKind::Tool {
                self.select_event(index as usize);
                return;
            }
            index += direction.signum();
        }
    }

    fn jump_to_prompt_boundary(&mut self, direction: isize) {
        if self.session.events.is_empty() {
            return;
        }

        let start = self.selected_event as isize + direction.signum();
        let mut index = start;
        while index >= 0 && index < self.session.events.len() as isize {
            if self.session.events[index as usize].is_prompt_boundary() {
                self.select_event(index as usize);
                return;
            }
            index += direction.signum();
        }
    }

    fn jump_to_parent_prompt(&mut self) {
        let Some(current) = self.current_event() else {
            return;
        };

        if current.kind != SessionEventKind::Tool {
            return;
        }

        let prompt_index = current
            .prompt_id
            .as_deref()
            .and_then(|prompt_id| {
                self.session
                    .events
                    .iter()
                    .enumerate()
                    .take(self.selected_event)
                    .rev()
                    .find(|(_, event)| {
                        event.is_prompt_boundary() && event.prompt_id.as_deref() == Some(prompt_id)
                    })
                    .map(|(index, _)| index)
            })
            .or_else(|| {
                self.session
                    .events
                    .iter()
                    .enumerate()
                    .take(self.selected_event)
                    .rev()
                    .find(|(_, event)| event.is_prompt_boundary())
                    .map(|(index, _)| index)
            });

        if let Some(index) = prompt_index {
            self.set_focus(ReplayPane::Timeline);
            self.select_event(index);
        }
    }

    fn linked_event_indices(&self) -> Vec<usize> {
        let Some(current) = self.current_event() else {
            return Vec::new();
        };

        self.session
            .events
            .iter()
            .enumerate()
            .filter(|(index, event)| {
                if *index == self.selected_event {
                    return false;
                }

                let shared_prompt =
                    current.prompt_id.is_some() && current.prompt_id == event.prompt_id;
                let shared_tool_use =
                    current.tool_use_id.is_some() && current.tool_use_id == event.tool_use_id;
                shared_prompt || shared_tool_use
            })
            .map(|(index, _)| index)
            .collect()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReplayView;

impl ReplayView {
    pub fn render(frame: &mut Frame<'_>, area: Rect, state: &ReplayViewState) {
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

        if let Some(search) = &state.search_overlay {
            render_search_overlay_widget(frame, area, &state.session, search);
        }
    }

    fn title_line(state: &ReplayViewState, collapse_footer: bool) -> Line<'static> {
        let short_id = short_session_id(&state.session.session_id);
        let title = if collapse_footer {
            format!(
                " Replay {short_id}  ${:.2}  {} tok  {} tools  {} ",
                state.session.cost_usd(),
                estimated_token_count(&state.session),
                state.session.tool_count(),
                current_event_timestamp(state)
            )
        } else {
            format!(
                " Replay {short_id}  [Tab] focus  [1/2/3] jump  [?] help  [/] search  [Esc] back "
            )
        };

        Line::from(title).bold()
    }

    fn render_wide(frame: &mut Frame<'_>, area: Rect, state: &ReplayViewState) {
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

    fn render_medium(frame: &mut Frame<'_>, area: Rect, state: &ReplayViewState) {
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

    fn render_narrow(frame: &mut Frame<'_>, area: Rect, state: &ReplayViewState) {
        match state.focus {
            ReplayPane::Timeline => Self::render_timeline(frame, area, state),
            ReplayPane::Transcript => Self::render_transcript(frame, area, state),
            ReplayPane::Evidence => Self::render_evidence(frame, area, state),
        }
    }

    fn render_timeline(frame: &mut Frame<'_>, area: Rect, state: &ReplayViewState) {
        let block = pane_block(
            ReplayPane::Timeline.title(),
            state.focus == ReplayPane::Timeline,
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let lines = if state.session.events.is_empty() {
            vec![Line::from(Span::styled(
                "No events in this session yet.",
                Style::new().fg(TEXT_DIM),
            ))]
        } else {
            let content_area = inner.inner(Margin::new(1, 0));
            let (start, end) = selection_window(
                state.selected_event,
                state.session.events.len(),
                content_area.height.max(1) as usize,
            );
            state.session.events[start..end]
                .iter()
                .enumerate()
                .map(|(offset, event)| {
                    let index = start + offset;
                    let marker = if index == state.selected_event {
                        "▸"
                    } else {
                        " "
                    };
                    Line::from(vec![
                        Span::styled(
                            format!("{marker} {} ", event_emoji(event.kind)),
                            event_style(state, index),
                        ),
                        Span::styled(event.event_type.clone(), event_style(state, index)),
                        Span::styled(
                            format!("  {}", format_timestamp(event.timestamp)),
                            Style::new().fg(TEXT_DIM),
                        ),
                    ])
                })
                .collect()
        };

        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::new().bg(SURFACE))
                .alignment(Alignment::Left),
            inner.inner(Margin::new(1, 0)),
        );
    }

    fn render_transcript(frame: &mut Frame<'_>, area: Rect, state: &ReplayViewState) {
        let block = pane_block(
            ReplayPane::Transcript.title(),
            state.focus == ReplayPane::Transcript,
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let lines = if state.session.events.is_empty() {
            vec![Line::from(Span::styled(
                "No transcript found for this session.",
                Style::new().fg(TEXT_DIM),
            ))]
        } else {
            let content_area = inner.inner(Margin::new(1, 0));
            let (start, end) = selection_window(
                state.selected_event,
                state.session.events.len(),
                content_area.height.max(1) as usize,
            );
            state.session.events[start..end]
                .iter()
                .enumerate()
                .map(|(offset, event)| {
                    let index = start + offset;
                    let marker = if index == state.selected_event {
                        "▸"
                    } else {
                        " "
                    };
                    let summary = transcript_summary(
                        event,
                        state.transcript_expanded && index == state.selected_event,
                    );
                    Line::from(vec![
                        Span::styled(format!("{marker} "), event_style(state, index)),
                        Span::styled(summary, event_style(state, index)),
                    ])
                })
                .collect()
        };

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

        let lines = evidence_lines(state);
        let content_area = inner.inner(Margin::new(1, 0));
        let visible = lines
            .into_iter()
            .skip(state.evidence_scroll)
            .take(content_area.height.max(1) as usize)
            .collect::<Vec<_>>();

        frame.render_widget(
            Paragraph::new(visible)
                .style(Style::new().bg(SURFACE))
                .alignment(Alignment::Left),
            content_area,
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
                format!(
                    " chain:{} ",
                    if state.causal_chain_highlight {
                        "on"
                    } else {
                        "off"
                    }
                ),
                Style::new().fg(TEXT_DIM),
            ),
            Span::styled("•", Style::new().fg(TEXT_DIM)),
            Span::styled(
                format!(" {} {} ", mood.emoji(), mood.label()),
                mood.style().add_modifier(Modifier::BOLD),
            ),
        ]))
        .style(Style::new().bg(BACKGROUND))
        .alignment(Alignment::Center)
    }
}

fn evidence_lines(state: &ReplayViewState) -> Vec<Line<'static>> {
    let Some(event) = state.current_event() else {
        return vec![Line::from(Span::styled(
            "Select an event to see details.",
            Style::new().fg(TEXT_DIM),
        ))];
    };

    let mut lines = vec![
        Line::from(Span::styled(
            format!("event {}", state.selected_event.saturating_add(1)),
            Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            current_event_timestamp(state),
            Style::new().fg(TEXT_DIM),
        )),
        Line::from(Span::styled(
            format!("type {}", event.event_type),
            Style::new().fg(TEXT_DIM),
        )),
    ];

    if let Some(tool_name) = &event.tool_name {
        lines.push(Line::from(Span::styled(
            format!("tool {tool_name}"),
            Style::new().fg(TEXT_DIM),
        )));
    }
    if let Some(file_path) = &event.file_path {
        lines.push(Line::from(Span::styled(
            format!("path {file_path}"),
            Style::new().fg(TEXT_DIM),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        event
            .content
            .clone()
            .unwrap_or_else(|| "No event content captured.".to_owned()),
        Style::new().fg(TEXT_PRIMARY),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "y copy JSON  o open file  l linked events",
        Style::new().fg(TEXT_DIM),
    )));

    if state.linked_events_open {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "linked events",
            Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
        )));
        let linked = state.linked_event_indices();
        if linked.is_empty() {
            lines.push(Line::from(Span::styled(
                "No linked prompt/tool events found.",
                Style::new().fg(TEXT_DIM),
            )));
        } else {
            for index in linked {
                let linked_event = &state.session.events[index];
                lines.push(Line::from(Span::styled(
                    format!(
                        "↳ #{:02} {} {}",
                        index + 1,
                        linked_event.event_type,
                        format_timestamp(linked_event.timestamp)
                    ),
                    Style::new().fg(TEXT_DIM),
                )));
            }
        }
    }

    if let Some(status) = &state.last_evidence_status {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            status.clone(),
            Style::new().fg(TEXT_DIM),
        )));
    }

    lines
}

fn transcript_summary(event: &SessionEvent, expanded: bool) -> String {
    let mut parts = vec![event.event_type.clone()];
    if let Some(tool_name) = &event.tool_name {
        parts.push(tool_name.clone());
    }
    if expanded {
        if let Some(content) = &event.content {
            parts.push(content.clone());
        }
    } else if let Some(content) = &event.content {
        let preview = content.chars().take(36).collect::<String>();
        if !preview.is_empty() {
            parts.push(preview);
        }
    }
    parts.join(" · ")
}

fn event_style(state: &ReplayViewState, index: usize) -> Style {
    if index == state.selected_event {
        return Style::new().fg(BORDER_ACTIVE).add_modifier(Modifier::BOLD);
    }

    if state.causal_chain_highlight {
        if index < state.selected_event {
            return Style::new()
                .fg(TEXT_PRIMARY)
                .bg(crate::session_list::ACCENT_CYAN);
        }
        if index > state.selected_event {
            return Style::new()
                .fg(TEXT_PRIMARY)
                .bg(crate::session_list::ACCENT_GREEN);
        }
    }

    Style::new().fg(TEXT_PRIMARY)
}

fn visible_pane_areas(area: Rect, state: &ReplayViewState) -> PaneAreas {
    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);
    let body_area = if area.height < 24 {
        inner
    } else {
        let [body, _footer] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .areas(inner);
        body
    };

    match ReplayLayoutMode::from_width(area.width) {
        ReplayLayoutMode::Wide => {
            let [timeline, transcript, evidence] = Layout::horizontal([
                Constraint::Percentage(40),
                Constraint::Percentage(35),
                Constraint::Percentage(25),
            ])
            .areas(body_area);
            PaneAreas {
                timeline,
                transcript,
                evidence: Some(evidence),
            }
        }
        ReplayLayoutMode::Medium => {
            let [timeline, transcript] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(body_area);
            let evidence = if state.evidence_overlay_open {
                Some(centered_rect(
                    body_area.width.saturating_sub(8).clamp(28, 60),
                    body_area.height.saturating_sub(6).clamp(10, 18),
                    body_area,
                ))
            } else {
                None
            };
            PaneAreas {
                timeline,
                transcript,
                evidence,
            }
        }
        ReplayLayoutMode::Narrow => PaneAreas {
            timeline: body_area,
            transcript: body_area,
            evidence: Some(body_area),
        },
    }
}

fn hit_test_timeline(mouse: MouseEvent, pane: Rect, state: &ReplayViewState) -> Option<usize> {
    let area = pane_content_area(pane);
    hit_test_event_list(mouse, area, state)
}

fn hit_test_transcript(mouse: MouseEvent, pane: Rect, state: &ReplayViewState) -> Option<usize> {
    let area = pane_content_area(pane);
    hit_test_event_list(mouse, area, state)
}

fn hit_test_event_list(mouse: MouseEvent, area: Rect, state: &ReplayViewState) -> Option<usize> {
    if !contains(area, mouse.column, mouse.row) || state.session.events.is_empty() {
        return None;
    }

    let row = mouse.row.saturating_sub(area.y) as usize;
    let (start, end) = selection_window(
        state.selected_event,
        state.session.events.len(),
        area.height.max(1) as usize,
    );
    let index = start + row;
    if index < end {
        Some(index)
    } else {
        None
    }
}

fn pane_content_area(pane: Rect) -> Rect {
    Block::default()
        .borders(Borders::ALL)
        .inner(pane)
        .inner(Margin::new(1, 0))
}

fn contains(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x
        && x < area.x.saturating_add(area.width)
        && y >= area.y
        && y < area.y.saturating_add(area.height)
}

fn selection_window(selected: usize, total: usize, max_lines: usize) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }

    let max_lines = max_lines.max(1);
    let start = selected.saturating_sub(max_lines.saturating_sub(1) / 2);
    let end = (start + max_lines).min(total);
    (start, end)
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

fn format_timestamp(timestamp: OffsetDateTime) -> String {
    match timestamp.format(&Rfc3339) {
        Ok(value) => value,
        Err(_) => timestamp.unix_timestamp().to_string(),
    }
}

fn event_emoji(kind: SessionEventKind) -> &'static str {
    match kind {
        SessionEventKind::Other => "📋",
        SessionEventKind::Tool => "🔧",
        SessionEventKind::PermissionRequest => "❓",
        SessionEventKind::Retry => "🔁",
        SessionEventKind::PermissionDenied => "🚫",
        SessionEventKind::PostToolUseFailure | SessionEventKind::StopFailure => "⚠️",
    }
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

    let draw_result = terminal.draw(|frame| ReplayView::render(frame, frame.area(), &state));
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
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    use super::*;
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
    fn keyboard_replay_navigation_cycles_and_jumps_between_panes() {
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
    fn keyboard_replay_enter_opens_evidence_overlay() {
        let mut state = sample_state();

        state.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(state.focus, ReplayPane::Evidence);
        assert!(state.evidence_overlay_open);
    }

    #[test]
    fn keyboard_replay_prompt_and_tool_jumps_work() {
        let mut state = sample_state();
        state.select_event(1);

        state.handle_key_event(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
        assert_eq!(state.selected_event, 3);

        state.handle_key_event(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE));
        assert_eq!(state.selected_event, 0);

        state.handle_key_event(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(state.selected_event, 1);

        state.handle_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(state.selected_event, 0);
    }

    #[test]
    fn search_overlay_filters_events_in_real_time() {
        let mut state = sample_state();

        state.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(state.search_overlay.is_some());

        state.handle_key_event(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        state.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        state.handle_key_event(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        state.handle_key_event(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));

        assert_eq!(state.selected_event, 1);
        assert_eq!(
            state.search_overlay.as_ref().map(|search| search.query()),
            Some("bash")
        );
    }

    #[test]
    fn search_overlay_escape_restores_previous_focus() {
        let mut state = sample_state();
        state.set_focus(ReplayPane::Transcript);

        state.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        state.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(state.search_overlay.is_none());
        assert_eq!(state.focus, ReplayPane::Transcript);
    }

    #[test]
    fn mouse_click_selects_timeline_and_transcript_rows() {
        let mut state = sample_state();

        state.handle_mouse_event(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 8,
                row: 4,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        );
        assert_eq!(state.focus, ReplayPane::Timeline);

        state.handle_mouse_event(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 70,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        );
        assert_eq!(state.focus, ReplayPane::Transcript);
    }

    #[test]
    fn evidence_actions_emit_side_effects() {
        let mut state = sample_state();
        state.set_focus(ReplayPane::Evidence);
        state.select_event(1);

        let copy = state.handle_key_event(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert!(matches!(copy, ReplayAction::CopyEvidenceJson(_)));

        let open = state.handle_key_event(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        assert_eq!(
            open,
            ReplayAction::OpenFileInEditor {
                path: "src/lib.rs".to_owned(),
            }
        );
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

    fn sample_state() -> ReplayViewState {
        ReplayViewState::from_session(SessionListItem::new(
            "session-1",
            "feature/replay-layout",
            parse_timestamp("2026-04-03T01:08:00Z"),
            0.42,
            vec![
                SessionEvent::new(
                    SessionEventKind::Other,
                    parse_timestamp("2026-04-03T01:07:00Z"),
                )
                .with_event_type("UserPromptSubmit")
                .with_prompt_id("prompt-1")
                .with_content("Open src/lib.rs and inspect the main entry point."),
                SessionEvent::tool("tool-1", parse_timestamp("2026-04-03T01:07:05Z"))
                    .with_tool_name("Bash")
                    .with_prompt_id("prompt-1")
                    .with_content("cat src/lib.rs")
                    .with_file_path("src/lib.rs")
                    .with_raw_json("{\"tool_name\":\"Bash\",\"tool_use_id\":\"tool-1\"}"),
                SessionEvent::new(
                    SessionEventKind::PermissionDenied,
                    parse_timestamp("2026-04-03T01:07:10Z"),
                )
                .with_content("Permission denied for unsafe command."),
                SessionEvent::new(
                    SessionEventKind::Other,
                    parse_timestamp("2026-04-03T01:07:15Z"),
                )
                .with_event_type("UserPromptSubmit")
                .with_prompt_id("prompt-2")
                .with_content("Search for Retry handling."),
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
