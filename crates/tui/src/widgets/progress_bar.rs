use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

pub const PROGRESS_BAR_WIDTH: usize = 16;
pub const PROGRESS_BAR_FILLED_COLOR: Color = Color::Rgb(0x3f, 0xb9, 0x50);
pub const PROGRESS_BAR_EMPTY_COLOR: Color = Color::Rgb(0x30, 0x36, 0x3d);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressBar {
    current: u64,
    total: u64,
    width: usize,
}

impl ProgressBar {
    pub fn new(current: u64, total: u64) -> Self {
        Self {
            current,
            total,
            width: PROGRESS_BAR_WIDTH,
        }
    }

    pub fn with_width(mut self, width: usize) -> Self {
        self.width = width.max(1);
        self
    }

    pub fn filled_width(self) -> usize {
        if self.total == 0 {
            return 0;
        }

        let clamped_current = self.current.min(self.total);
        ((clamped_current * self.width as u64) + (self.total / 2)) as usize / self.total as usize
    }

    pub fn render(self) -> Line<'static> {
        let filled = self.filled_width();
        let empty = self.width.saturating_sub(filled);

        Line::from(vec![
            Span::raw("["),
            Span::styled(
                "█".repeat(filled),
                Style::new().fg(PROGRESS_BAR_FILLED_COLOR),
            ),
            Span::styled("░".repeat(empty), Style::new().fg(PROGRESS_BAR_EMPTY_COLOR)),
            Span::raw(format!("] {}/{}", self.current, self.total)),
        ])
    }
}

#[cfg(test)]
mod animations_widgets_tests {
    use super::*;

    #[test]
    fn progress_bar_matches_spec_rendering() {
        let line = ProgressBar::new(47, 100).render();

        assert_eq!(line.to_string(), "[████████░░░░░░░░] 47/100");
        assert_eq!(line.spans[1].style.fg, Some(PROGRESS_BAR_FILLED_COLOR));
        assert_eq!(line.spans[2].style.fg, Some(PROGRESS_BAR_EMPTY_COLOR));
    }
}
