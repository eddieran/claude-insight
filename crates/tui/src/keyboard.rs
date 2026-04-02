use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    prelude::Frame,
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::session_list::{BORDER_ACTIVE, SURFACE, TEXT_DIM, TEXT_PRIMARY};

#[cfg(test)]
use ratatui::{
    backend::TestBackend,
    prelude::{Buffer, Terminal},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keybinding {
    pub key: &'static str,
    pub action: &'static str,
}

const GLOBAL_KEYBINDINGS: &[Keybinding] = &[
    Keybinding {
        key: "q / Ctrl+C",
        action: "Quit",
    },
    Keybinding {
        key: "?",
        action: "Toggle help overlay",
    },
    Keybinding {
        key: "/",
        action: "Open search overlay",
    },
    Keybinding {
        key: "Tab",
        action: "Cycle panes",
    },
    Keybinding {
        key: "1 / 2 / 3",
        action: "Jump to pane",
    },
    Keybinding {
        key: "Esc",
        action: "Close overlay / back",
    },
];

const SESSION_LIST_KEYBINDINGS: &[Keybinding] = &[
    Keybinding {
        key: "j / Down",
        action: "Next session",
    },
    Keybinding {
        key: "k / Up",
        action: "Previous session",
    },
    Keybinding {
        key: "Enter",
        action: "Open replay",
    },
    Keybinding {
        key: "s",
        action: "Cycle sort",
    },
    Keybinding {
        key: "f",
        action: "Open filter overlay",
    },
];

const REPLAY_KEYBINDINGS: &[Keybinding] = &[
    Keybinding {
        key: "j / k",
        action: "Move timeline selection",
    },
    Keybinding {
        key: "Enter",
        action: "Show selected event in evidence",
    },
    Keybinding {
        key: "c",
        action: "Toggle causal chain highlight",
    },
    Keybinding {
        key: "e",
        action: "Expand transcript tool call",
    },
    Keybinding {
        key: "p",
        action: "Jump to parent prompt",
    },
    Keybinding {
        key: "n / N",
        action: "Next / previous tool call",
    },
    Keybinding {
        key: "[ / ]",
        action: "Previous / next prompt boundary",
    },
    Keybinding {
        key: "g / G",
        action: "First / last event",
    },
    Keybinding {
        key: "/",
        action: "Search within session",
    },
];

const EVIDENCE_KEYBINDINGS: &[Keybinding] = &[
    Keybinding {
        key: "j / k",
        action: "Scroll evidence",
    },
    Keybinding {
        key: "y",
        action: "Copy raw JSON",
    },
    Keybinding {
        key: "o",
        action: "Open file path in editor",
    },
    Keybinding {
        key: "l",
        action: "Toggle linked events tree",
    },
];

pub fn render_help_overlay_widget(frame: &mut Frame<'_>, area: Rect) {
    let popup = centered_rect(
        area.width.saturating_sub(20).clamp(72, 96),
        area.height.saturating_sub(8).clamp(20, 30),
        area,
    );
    let block = Block::default()
        .title(Line::from(" Keyboard Help ").bold())
        .borders(Borders::ALL)
        .border_style(Style::new().fg(BORDER_ACTIVE))
        .style(Style::new().bg(SURFACE));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .areas(inner.inner(Margin::new(1, 1)));

    frame.render_widget(
        render_help_column(
            "Global",
            GLOBAL_KEYBINDINGS,
            "Session List",
            SESSION_LIST_KEYBINDINGS,
        ),
        left,
    );
    frame.render_widget(
        render_help_column(
            "Replay",
            REPLAY_KEYBINDINGS,
            "Evidence",
            EVIDENCE_KEYBINDINGS,
        ),
        right,
    );
}

fn render_help_column(
    top_title: &'static str,
    top_rows: &[Keybinding],
    bottom_title: &'static str,
    bottom_rows: &[Keybinding],
) -> Paragraph<'static> {
    let mut lines = vec![section_title(top_title), header_line()];
    lines.extend(binding_lines(top_rows));
    lines.push(Line::from(""));
    lines.push(section_title(bottom_title));
    lines.push(header_line());
    lines.extend(binding_lines(bottom_rows));

    Paragraph::new(lines).style(Style::new().fg(TEXT_DIM))
}

fn binding_lines(bindings: &[Keybinding]) -> Vec<Line<'static>> {
    bindings
        .iter()
        .map(|binding| {
            let key = format!("{:<14}", binding.key);
            Line::from(vec![
                Span::styled(
                    key,
                    Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
                ),
                Span::styled(binding.action, Style::new().fg(TEXT_DIM)),
            ])
        })
        .collect()
}

fn section_title(title: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        title,
        Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
    ))
}

fn header_line() -> Line<'static> {
    Line::from(Span::styled(
        "key           action",
        Style::new().fg(TEXT_DIM).add_modifier(Modifier::DIM),
    ))
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
pub fn render_help_overlay(width: u16, height: u16) -> String {
    let mut backend = TestBackend::new(width, height);
    let terminal = Terminal::new(backend);
    let mut terminal = match terminal {
        Ok(terminal) => terminal,
        Err(error) => return format!("terminal error: {error}"),
    };

    let draw_result = terminal.draw(|frame| render_help_overlay_widget(frame, frame.area()));
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
    use super::*;

    #[test]
    fn render_help_overlay_snapshot() {
        insta::assert_snapshot!(render_help_overlay(120, 40));
    }
}
