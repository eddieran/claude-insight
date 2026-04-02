use std::collections::BTreeSet;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    backend::TestBackend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    prelude::{Buffer, Frame, Terminal},
    style::{Color, Modifier, Style, Stylize},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Sparkline},
};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

use crate::widgets::{
    mood_badge::{compute_mood, render_mood_badge, Mood},
    sparkline::compute_sparkline_data,
};

pub const BACKGROUND: Color = Color::Rgb(0x0d, 0x11, 0x17);
pub const SURFACE: Color = Color::Rgb(0x16, 0x1b, 0x22);
pub const BORDER: Color = Color::Rgb(0x30, 0x36, 0x3d);
pub const BORDER_ACTIVE: Color = Color::Rgb(0x58, 0xa6, 0xff);
pub const TEXT_PRIMARY: Color = Color::Rgb(0xc9, 0xd1, 0xd9);
pub const TEXT_DIM: Color = Color::Rgb(0x8b, 0x94, 0x9e);
pub const ACCENT_CYAN: Color = Color::Rgb(0x58, 0xa6, 0xff);
pub const ACCENT_GREEN: Color = Color::Rgb(0x3f, 0xb9, 0x50);
pub const ACCENT_RED: Color = Color::Rgb(0xf8, 0x51, 0x49);
pub const ACCENT_AMBER: Color = Color::Rgb(0xd2, 0x99, 0x22);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoodFilter {
    All,
    Clean,
    Friction,
    Errors,
}

impl MoodFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Clean,
            Self::Clean => Self::Friction,
            Self::Friction => Self::Errors,
            Self::Errors => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Clean => "🟢 clean",
            Self::Friction => "🟡 friction",
            Self::Errors => "🔴 errors",
        }
    }

    fn matches(self, mood: Mood) -> bool {
        match self {
            Self::All => true,
            Self::Clean => mood == Mood::Clean,
            Self::Friction => mood == Mood::Friction,
            Self::Errors => mood == Mood::Errors,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Date,
    Cost,
    Events,
}

impl SortOrder {
    pub fn next(self) -> Self {
        match self {
            Self::Date => Self::Cost,
            Self::Cost => Self::Events,
            Self::Events => Self::Date,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Date => "date",
            Self::Cost => "cost",
            Self::Events => "events",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionListOverlay {
    None,
    Filter,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEventKind {
    SessionBoundary,
    UserPromptSubmit,
    InstructionsLoaded,
    Subagent,
    Other,
    Tool,
    PermissionRequest,
    Retry,
    PermissionDenied,
    PostToolUseFailure,
    StopFailure,
}

impl SessionEventKind {
    pub fn from_event_type(event_type: &str) -> Self {
        match event_type {
            "SessionStart" | "SessionEnd" | "TaskCreated" | "TaskCompleted" => {
                Self::SessionBoundary
            }
            "UserPromptSubmit" => Self::UserPromptSubmit,
            "InstructionsLoaded" => Self::InstructionsLoaded,
            "SubagentStart" | "SubagentStop" => Self::Subagent,
            "PreToolUse" | "PostToolUse" => Self::Tool,
            "PermissionRequest" => Self::PermissionRequest,
            "Retry" => Self::Retry,
            "PermissionDenied" => Self::PermissionDenied,
            "PostToolUseFailure" => Self::PostToolUseFailure,
            "StopFailure" => Self::StopFailure,
            _ => Self::Other,
        }
    }

    pub fn default_label(self) -> &'static str {
        match self {
            Self::SessionBoundary => "Session boundary",
            Self::UserPromptSubmit => "User prompt",
            Self::InstructionsLoaded => "Instructions loaded",
            Self::Subagent => "Subagent lifecycle",
            Self::Other => "Event",
            Self::Tool => "Tool call",
            Self::PermissionRequest => "Permission allowed",
            Self::Retry => "Retry",
            Self::PermissionDenied => "Permission denied",
            Self::PostToolUseFailure => "Tool failure",
            Self::StopFailure => "Stop failure",
        }
    }

    pub fn is_tool_call(self) -> bool {
        matches!(self, Self::Tool)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEvent {
    pub kind: SessionEventKind,
    pub timestamp: OffsetDateTime,
    pub tool_use_id: Option<String>,
    pub label: String,
}

impl SessionEvent {
    pub fn new(kind: SessionEventKind, timestamp: OffsetDateTime) -> Self {
        Self {
            kind,
            timestamp,
            tool_use_id: None,
            label: kind.default_label().to_string(),
        }
    }

    pub fn tool(tool_use_id: impl Into<String>, timestamp: OffsetDateTime) -> Self {
        Self {
            kind: SessionEventKind::Tool,
            timestamp,
            tool_use_id: Some(tool_use_id.into()),
            label: SessionEventKind::Tool.default_label().to_string(),
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionListItem {
    pub session_id: String,
    pub git_branch: String,
    pub last_updated: OffsetDateTime,
    pub cost_micros: u64,
    pub events: Vec<SessionEvent>,
}

impl SessionListItem {
    pub fn new(
        session_id: impl Into<String>,
        git_branch: impl Into<String>,
        last_updated: OffsetDateTime,
        cost_usd: f64,
        events: Vec<SessionEvent>,
    ) -> Self {
        let rounded = (cost_usd.max(0.0) * 100_0000.0).round();
        let cost_micros = if rounded.is_finite() && rounded >= 0.0 {
            rounded as u64
        } else {
            0
        };

        Self {
            session_id: session_id.into(),
            git_branch: git_branch.into(),
            last_updated,
            cost_micros,
            events,
        }
    }

    pub fn mood(&self) -> Mood {
        compute_mood(&self.events)
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn tool_count(&self) -> usize {
        let unique = self
            .events
            .iter()
            .filter_map(|event| event.tool_use_id.as_deref())
            .collect::<BTreeSet<_>>();
        if unique.is_empty() {
            self.events
                .iter()
                .filter(|event| event.kind == SessionEventKind::Tool)
                .count()
        } else {
            unique.len()
        }
    }

    pub fn cost_usd(&self) -> f64 {
        self.cost_micros as f64 / 100_0000.0
    }

    pub fn relative_time(&self, now: OffsetDateTime) -> String {
        format_relative_time(now - self.last_updated)
    }

    pub fn activity_buckets(&self) -> Vec<u64> {
        compute_sparkline_data(&self.events, 5)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionListView {
    sessions: Vec<SessionListItem>,
    now: OffsetDateTime,
    selected: usize,
    scroll: usize,
    sort_order: SortOrder,
    overlay: SessionListOverlay,
    branch_filter_index: usize,
    mood_filter: MoodFilter,
    empty_state_message: Option<String>,
}

impl SessionListView {
    pub fn new(sessions: Vec<SessionListItem>, now: OffsetDateTime) -> Self {
        let mut view = Self {
            sessions,
            now,
            selected: 0,
            scroll: 0,
            sort_order: SortOrder::Date,
            overlay: SessionListOverlay::None,
            branch_filter_index: 0,
            mood_filter: MoodFilter::All,
            empty_state_message: None,
        };
        view.ensure_selection_in_bounds();
        view
    }

    pub fn overlay(&self) -> SessionListOverlay {
        self.overlay
    }

    pub fn sort_order(&self) -> SortOrder {
        self.sort_order
    }

    pub fn mood_filter(&self) -> MoodFilter {
        self.mood_filter
    }

    pub fn branch_filter_label(&self) -> &str {
        let branches = self.branch_options();
        let index = self
            .branch_filter_index
            .min(branches.len().saturating_sub(1));
        branches[index]
    }

    pub fn selected_session(&self) -> Option<&SessionListItem> {
        self.visible_indices()
            .get(self.selected)
            .and_then(|index| self.sessions.get(*index))
    }

    pub fn close_overlay(&mut self) {
        self.overlay = SessionListOverlay::None;
    }

    pub fn empty_state_message(&self) -> Option<&str> {
        self.empty_state_message.as_deref()
    }

    pub fn set_empty_state_message(&mut self, message: impl Into<String>) {
        self.empty_state_message = Some(message.into());
    }

    pub fn clear_empty_state_message(&mut self) {
        self.empty_state_message = None;
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default()
            .title(Line::from(" CLAUDE INSIGHT ").bold())
            .borders(Borders::ALL)
            .border_style(Style::new().fg(BORDER_ACTIVE))
            .style(Style::new().bg(BACKGROUND).fg(TEXT_PRIMARY));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let [title, body, legend, help] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .areas(inner);

        frame.render_widget(render_title(self.sort_order), title);
        if self.sessions.is_empty() || self.visible_indices().is_empty() {
            render_empty_state(frame, body, self.empty_state_message.as_deref());
        } else {
            self.render_rows(frame, body);
        }
        frame.render_widget(render_legend(), legend);
        frame.render_widget(render_help_bar(), help);

        match self.overlay {
            SessionListOverlay::None => {}
            SessionListOverlay::Filter => self.render_filter_popup(frame, area),
            SessionListOverlay::Search => self.render_search_popup(frame, area),
        }
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        match self.overlay {
            SessionListOverlay::Filter => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('f') => self.overlay = SessionListOverlay::None,
                    KeyCode::Char('b') => self.cycle_branch_filter(),
                    KeyCode::Char('m') => self.cycle_mood_filter(),
                    _ => {}
                }
                return;
            }
            SessionListOverlay::Search => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('/')) {
                    self.overlay = SessionListOverlay::None;
                }
                return;
            }
            SessionListOverlay::None => {}
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('s') => {
                self.sort_order = self.sort_order.next();
                self.ensure_selection_in_bounds();
            }
            KeyCode::Char('f') => self.overlay = SessionListOverlay::Filter,
            KeyCode::Char('/') => self.overlay = SessionListOverlay::Search,
            _ => {}
        }
    }

    fn render_rows(&self, frame: &mut Frame<'_>, area: Rect) {
        let visible = self.visible_indices();
        let max_rows = area.height.saturating_sub(1) as usize;
        let selection = self.selected.min(visible.len().saturating_sub(1));
        let scroll = if selection < self.scroll {
            selection
        } else if selection >= self.scroll + max_rows {
            selection + 1 - max_rows
        } else {
            self.scroll
        };

        let row_constraints = vec![Constraint::Length(1); max_rows.max(1)];
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(row_constraints)
            .split(area);

        for (row_idx, area) in rows.iter().enumerate() {
            let visible_index = scroll + row_idx;
            if visible_index >= visible.len() {
                break;
            }

            let session_index = visible[visible_index];
            let session = &self.sessions[session_index];
            let selected = visible_index == selection;
            render_session_row(frame, *area, visible_index + 1, session, self.now, selected);
        }
    }

    fn render_filter_popup(&self, frame: &mut Frame<'_>, area: Rect) {
        let popup = centered_rect(52, 8, area);
        let block = Block::default()
            .title(Line::from(" Filter ").bold())
            .borders(Borders::ALL)
            .border_style(Style::new().fg(BORDER_ACTIVE))
            .style(Style::new().bg(SURFACE));
        let inner = block.inner(popup);
        frame.render_widget(Clear, popup);
        frame.render_widget(block, popup);

        let branches = self.branch_options().join("  ");
        let lines = vec![
            Line::from(vec![
                Span::styled("b", Style::new().fg(ACCENT_CYAN).bold()),
                Span::raw(format!(" branch: {}", self.branch_filter_label())),
            ]),
            Line::from(vec![
                Span::styled("m", Style::new().fg(ACCENT_CYAN).bold()),
                Span::raw(format!(" mood: {}", self.mood_filter.label())),
            ]),
            Line::from(Span::styled(
                format!("options: {branches}"),
                Style::new().fg(TEXT_DIM),
            )),
            Line::from(Span::styled(
                "Esc close",
                Style::new().fg(TEXT_DIM).add_modifier(Modifier::DIM),
            )),
        ];
        frame.render_widget(Paragraph::new(lines), inner.inner(Margin::new(1, 1)));
    }

    fn render_search_popup(&self, frame: &mut Frame<'_>, area: Rect) {
        let popup = centered_rect(58, 6, area);
        let block = Block::default()
            .title(Line::from(" Search ").bold())
            .borders(Borders::ALL)
            .border_style(Style::new().fg(BORDER_ACTIVE))
            .style(Style::new().bg(SURFACE));
        let inner = block.inner(popup);
        frame.render_widget(Clear, popup);
        frame.render_widget(block, popup);
        let lines = vec![
            Line::from(Span::styled(
                "Search overlay implementation lands in CI-20.",
                Style::new().fg(TEXT_PRIMARY),
            )),
            Line::from(Span::styled(
                "Press Esc to return to the session list.",
                Style::new().fg(TEXT_DIM),
            )),
        ];
        frame.render_widget(
            Paragraph::new(lines).alignment(Alignment::Center),
            inner.inner(Margin::new(1, 1)),
        );
    }

    fn move_selection(&mut self, delta: isize) {
        let visible_len = self.visible_indices().len();
        if visible_len == 0 {
            self.selected = 0;
            return;
        }

        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, visible_len.saturating_sub(1) as isize) as usize;
    }

    fn cycle_branch_filter(&mut self) {
        let branches = self.branch_options();
        if !branches.is_empty() {
            self.branch_filter_index = (self.branch_filter_index + 1) % branches.len();
        }
        self.ensure_selection_in_bounds();
    }

    fn cycle_mood_filter(&mut self) {
        self.mood_filter = self.mood_filter.next();
        self.ensure_selection_in_bounds();
    }

    fn ensure_selection_in_bounds(&mut self) {
        let visible_len = self.visible_indices().len();
        if visible_len == 0 {
            self.selected = 0;
            self.scroll = 0;
        } else {
            self.selected = self.selected.min(visible_len - 1);
        }
    }

    fn visible_indices(&self) -> Vec<usize> {
        let branch_filter = self.current_branch_filter();
        let mut indices = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| {
                let branch_matches = match branch_filter {
                    Some(branch) => session.git_branch == branch,
                    None => true,
                };
                branch_matches && self.mood_filter.matches(session.mood())
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();

        indices.sort_by(|left, right| {
            let left = &self.sessions[*left];
            let right = &self.sessions[*right];
            match self.sort_order {
                SortOrder::Date => right
                    .last_updated
                    .cmp(&left.last_updated)
                    .then_with(|| left.session_id.cmp(&right.session_id)),
                SortOrder::Cost => right
                    .cost_micros
                    .cmp(&left.cost_micros)
                    .then_with(|| right.last_updated.cmp(&left.last_updated)),
                SortOrder::Events => right
                    .event_count()
                    .cmp(&left.event_count())
                    .then_with(|| right.last_updated.cmp(&left.last_updated)),
            }
        });

        indices
    }

    fn branch_options(&self) -> Vec<&str> {
        let mut branches = vec!["all"];
        branches.extend(
            self.sessions
                .iter()
                .map(|session| session.git_branch.as_str())
                .collect::<BTreeSet<_>>(),
        );
        branches
    }

    fn current_branch_filter(&self) -> Option<&str> {
        let branches = self.branch_options();
        let index = self
            .branch_filter_index
            .min(branches.len().saturating_sub(1));
        match branches.get(index).copied() {
            Some("all") | None => None,
            Some(branch) => Some(branch),
        }
    }
}

pub fn render_session_list(test_data: Vec<SessionListItem>, width: u16, height: u16) -> String {
    let mut backend = TestBackend::new(width, height);
    let terminal = Terminal::new(backend);
    let mut terminal = match terminal {
        Ok(terminal) => terminal,
        Err(error) => return format!("terminal error: {error}"),
    };
    let view = SessionListView::new(test_data, parse_timestamp("2026-04-03T01:10:00Z"));

    let draw_result = terminal.draw(|frame| view.render(frame, frame.area()));
    if let Err(error) = draw_result {
        return format!("draw error: {error}");
    }

    backend = terminal.backend().clone();
    buffer_to_string(backend.buffer())
}

fn render_title(sort_order: SortOrder) -> Paragraph<'static> {
    let left = Span::styled("◉ Sessions", Style::new().fg(TEXT_PRIMARY).bold());
    let right = Span::styled(
        format!("[/] Search  [s] {}", sort_order.label()),
        Style::new().fg(TEXT_DIM),
    );
    let spacer = " ".repeat(24);

    Paragraph::new(Line::from(vec![left, Span::raw(spacer), right]))
}

fn render_legend() -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::styled("▁▂▃▅▇", Style::new().fg(ACCENT_CYAN)),
        Span::styled(" = activity sparkline    ", Style::new().fg(TEXT_DIM)),
        Span::styled("🟢🟡🔴", Style::new()),
        Span::styled(" = session mood", Style::new().fg(TEXT_DIM)),
    ]))
}

fn render_help_bar() -> Paragraph<'static> {
    Paragraph::new("j/k navigate  Enter open  s sort  f filter  / search  q quit")
        .style(Style::new().fg(TEXT_DIM).add_modifier(Modifier::DIM))
        .alignment(Alignment::Center)
}

fn render_empty_state(frame: &mut Frame<'_>, area: Rect, message: Option<&str>) {
    let paragraph = Paragraph::new(
        message.unwrap_or("No sessions captured yet. Run `claude-insight init` to start."),
    )
    .style(Style::new().fg(TEXT_DIM))
    .alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

fn render_session_row(
    frame: &mut Frame<'_>,
    area: Rect,
    index: usize,
    session: &SessionListItem,
    now: OffsetDateTime,
    selected: bool,
) {
    let [marker, branch, time, sparkline, cost, tools, mood] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(6),
            Constraint::Length(24),
            Constraint::Length(12),
            Constraint::Length(14),
            Constraint::Length(9),
            Constraint::Length(11),
            Constraint::Min(12),
        ])
        .areas(area);

    let row_style = if selected {
        Style::new().bg(SURFACE)
    } else {
        Style::new().bg(BACKGROUND)
    };

    frame.render_widget(
        Paragraph::new(if selected {
            format!("▸ #{index:<2}")
        } else {
            format!("  #{index:<2}")
        })
        .style(row_style.fg(if selected { ACCENT_CYAN } else { TEXT_DIM })),
        marker,
    );
    frame.render_widget(
        Paragraph::new(truncate_label(&session.git_branch, branch.width as usize)).style(
            row_style.fg(TEXT_PRIMARY).add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        ),
        branch,
    );
    frame.render_widget(
        Paragraph::new(format!(" {}", session.relative_time(now))).style(row_style.fg(TEXT_DIM)),
        time,
    );
    frame.render_widget(
        Sparkline::default()
            .data(session.activity_buckets())
            .bar_set(symbols::bar::NINE_LEVELS)
            .style(row_style.fg(ACCENT_CYAN))
            .max(session.activity_buckets().into_iter().max().unwrap_or(1)),
        sparkline,
    );
    frame.render_widget(
        Paragraph::new(format!(" ${:.2}", session.cost_usd())).style(row_style.fg(TEXT_PRIMARY)),
        cost,
    );
    frame.render_widget(
        Paragraph::new(format!(" {} tools", session.tool_count()))
            .style(row_style.fg(TEXT_PRIMARY)),
        tools,
    );
    let mood_value = session.mood();
    frame.render_widget(
        Paragraph::new(render_mood_badge(mood_value)).style(row_style),
        mood,
    );
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

fn truncate_label(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return format!("{value:<width$}");
    }

    let visible = width.saturating_sub(1);
    let mut truncated = value.chars().take(visible).collect::<String>();
    truncated.push('…');
    truncated
}

fn format_relative_time(delta: Duration) -> String {
    let seconds = delta.whole_seconds().max(0);
    match seconds {
        0..=59 => format!("{seconds}s ago"),
        60..=3_599 => format!("{}min ago", seconds / 60),
        3_600..=86_399 => format!("{}hr ago", seconds / 3_600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

fn parse_timestamp(input: &str) -> OffsetDateTime {
    match OffsetDateTime::parse(input, &Rfc3339) {
        Ok(value) => value,
        Err(error) => panic!("failed to parse timestamp {input}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;

    #[test]
    fn render_session_list_snapshot() {
        insta::assert_snapshot!(render_session_list(sample_sessions(), 120, 40));
    }

    #[test]
    fn mood_prefers_errors_over_permission_asks() {
        let item = SessionListItem::new(
            "session-1",
            "main",
            parse_timestamp("2026-04-03T01:08:00Z"),
            0.42,
            vec![
                event(SessionEventKind::PermissionRequest, "2026-04-03T01:07:00Z"),
                event(SessionEventKind::PermissionRequest, "2026-04-03T01:07:05Z"),
                event(SessionEventKind::PermissionRequest, "2026-04-03T01:07:10Z"),
                event(SessionEventKind::PermissionDenied, "2026-04-03T01:07:12Z"),
            ],
        );

        assert_eq!(item.mood(), Mood::Errors);
    }

    #[test]
    fn mood_turns_friction_after_multiple_retries() {
        let item = SessionListItem::new(
            "session-1",
            "main",
            parse_timestamp("2026-04-03T01:08:00Z"),
            0.42,
            vec![
                event(SessionEventKind::Retry, "2026-04-03T01:07:00Z"),
                event(SessionEventKind::Retry, "2026-04-03T01:07:05Z"),
            ],
        );

        assert_eq!(item.mood(), Mood::Friction);
    }

    #[test]
    fn sparkline_groups_events_into_five_second_buckets() {
        let item = SessionListItem::new(
            "session-1",
            "main",
            parse_timestamp("2026-04-03T01:08:00Z"),
            0.42,
            vec![
                event(SessionEventKind::Other, "2026-04-03T01:07:00Z"),
                event(SessionEventKind::Other, "2026-04-03T01:07:04Z"),
                event(SessionEventKind::Other, "2026-04-03T01:07:05Z"),
                event(SessionEventKind::Other, "2026-04-03T01:07:10Z"),
            ],
        );

        assert_eq!(item.activity_buckets(), vec![2, 1, 1]);
    }

    #[test]
    fn session_list_keybindings_cover_navigation_sort_filter_and_search() {
        let mut view =
            SessionListView::new(sample_sessions(), parse_timestamp("2026-04-03T01:10:00Z"));

        view.handle_key_event(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(
            view.selected_session()
                .map(|session| session.session_id.as_str()),
            Some("session-2")
        );

        view.handle_key_event(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(view.sort_order(), SortOrder::Cost);

        view.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert_eq!(view.overlay(), SessionListOverlay::Filter);

        view.handle_key_event(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert_eq!(view.mood_filter(), MoodFilter::Clean);

        view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(view.overlay(), SessionListOverlay::None);

        view.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(view.overlay(), SessionListOverlay::Search);
    }

    fn sample_sessions() -> Vec<SessionListItem> {
        vec![
            SessionListItem::new(
                "session-1",
                "main",
                parse_timestamp("2026-04-03T01:08:00Z"),
                0.42,
                vec![
                    tool_event("tool-1", "2026-04-03T01:07:00Z"),
                    tool_event("tool-2", "2026-04-03T01:07:05Z"),
                    event(SessionEventKind::Other, "2026-04-03T01:07:10Z"),
                    event(SessionEventKind::Other, "2026-04-03T01:07:15Z"),
                ],
            ),
            SessionListItem::new(
                "session-2",
                "feature/tui-session-list",
                parse_timestamp("2026-04-03T00:05:00Z"),
                1.23,
                vec![
                    event(SessionEventKind::PermissionRequest, "2026-04-03T00:00:00Z"),
                    event(SessionEventKind::PermissionRequest, "2026-04-03T00:00:05Z"),
                    event(SessionEventKind::PermissionRequest, "2026-04-03T00:00:10Z"),
                    tool_event("tool-3", "2026-04-03T00:00:15Z"),
                ],
            ),
            SessionListItem::new(
                "session-3",
                "main",
                parse_timestamp("2026-04-02T20:10:00Z"),
                0.05,
                vec![
                    event(SessionEventKind::PostToolUseFailure, "2026-04-02T20:00:00Z"),
                    tool_event("tool-4", "2026-04-02T20:00:05Z"),
                ],
            ),
        ]
    }

    fn event(kind: SessionEventKind, timestamp: &str) -> SessionEvent {
        SessionEvent::new(kind, parse_timestamp(timestamp))
    }

    fn tool_event(tool_use_id: &str, timestamp: &str) -> SessionEvent {
        SessionEvent::tool(tool_use_id, parse_timestamp(timestamp))
    }
}
