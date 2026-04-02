use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    prelude::Frame,
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    replay::ReplayPane,
    session_list::{SessionListItem, BORDER_ACTIVE, SURFACE, TEXT_DIM, TEXT_PRIMARY},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOverlayAction {
    None,
    Close,
    Submit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOverlayState {
    query: String,
    previous_focus: ReplayPane,
    selected_result: usize,
}

impl SearchOverlayState {
    pub fn new(previous_focus: ReplayPane, selected_result: usize) -> Self {
        Self {
            query: String::new(),
            previous_focus,
            selected_result,
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn previous_focus(&self) -> ReplayPane {
        self.previous_focus
    }

    pub fn selected_result(&self) -> usize {
        self.selected_result
    }

    pub fn filtered_indices(&self, session: &SessionListItem) -> Vec<usize> {
        let query = self.query.trim().to_ascii_lowercase();
        session
            .events
            .iter()
            .enumerate()
            .filter(|(_, event)| {
                if query.is_empty() {
                    return true;
                }

                event.search_blob().to_ascii_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub fn selected_index(&self, session: &SessionListItem) -> Option<usize> {
        let matches = self.filtered_indices(session);
        matches
            .get(self.selected_result.min(matches.len().saturating_sub(1)))
            .copied()
    }

    pub fn handle_key_event(
        &mut self,
        key: KeyEvent,
        session: &SessionListItem,
    ) -> SearchOverlayAction {
        match key.code {
            KeyCode::Esc => SearchOverlayAction::Close,
            KeyCode::Enter => SearchOverlayAction::Submit,
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1, session);
                SearchOverlayAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1, session);
                SearchOverlayAction::None
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected_result = 0;
                SearchOverlayAction::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.push(character);
                self.selected_result = 0;
                SearchOverlayAction::None
            }
            _ => SearchOverlayAction::None,
        }
    }

    fn move_selection(&mut self, delta: isize, session: &SessionListItem) {
        let matches = self.filtered_indices(session);
        if matches.is_empty() {
            self.selected_result = 0;
            return;
        }

        let next = self.selected_result as isize + delta;
        self.selected_result = next.clamp(0, matches.len() as isize - 1) as usize;
    }
}

pub fn render_search_overlay_widget(
    frame: &mut Frame<'_>,
    area: Rect,
    session: &SessionListItem,
    search: &SearchOverlayState,
) {
    let popup = centered_rect(
        area.width.saturating_sub(20).clamp(72, 96),
        area.height.saturating_sub(10).clamp(16, 28),
        area,
    );
    let block = Block::default()
        .title(Line::from(" Search ").bold())
        .borders(Borders::ALL)
        .border_style(Style::new().fg(BORDER_ACTIVE))
        .style(Style::new().bg(SURFACE));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    let [query_area, results_area, footer_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(inner.inner(Margin::new(1, 1)));

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "type to filter event_type, tool_name, and content",
                Style::new().fg(TEXT_DIM),
            )),
            Line::from(vec![
                Span::styled(
                    "> ",
                    Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
                ),
                Span::styled(search.query(), Style::new().fg(TEXT_PRIMARY)),
            ]),
        ]),
        query_area,
    );

    let matches = search.filtered_indices(session);
    let result_lines = if matches.is_empty() {
        vec![Line::from(Span::styled(
            format!("No results for '{}'", search.query()),
            Style::new().fg(TEXT_DIM),
        ))]
    } else {
        matches
            .iter()
            .enumerate()
            .take(results_area.height.max(1) as usize)
            .map(|(result_index, event_index)| {
                let event = &session.events[*event_index];
                let marker = if result_index == search.selected_result() {
                    "▸"
                } else {
                    " "
                };
                let detail = event
                    .tool_name
                    .as_deref()
                    .or(event.content.as_deref())
                    .unwrap_or("");
                Line::from(vec![
                    Span::styled(
                        format!("{marker} {:<18}", event.event_type),
                        if result_index == search.selected_result() {
                            Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD)
                        } else {
                            Style::new().fg(TEXT_DIM)
                        },
                    ),
                    Span::styled(detail, Style::new().fg(TEXT_DIM)),
                ])
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(result_lines), results_area);

    frame.render_widget(
        Paragraph::new("Enter jump  j/k move  Esc close")
            .style(Style::new().fg(TEXT_DIM).add_modifier(Modifier::DIM)),
        footer_area,
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
