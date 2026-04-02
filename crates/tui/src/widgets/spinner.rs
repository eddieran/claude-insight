use std::time::Duration as StdDuration;

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

pub const BRAILLE_SPINNER_INTERVAL_MS: u64 = 80;
pub const BRAILLE_SPINNER_COLOR: Color = Color::Rgb(0x58, 0xa6, 0xff);
pub const BRAILLE_SPINNER_FRAMES: [&str; 8] = ["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BrailleSpinner;

impl BrailleSpinner {
    pub fn frame(elapsed: StdDuration) -> &'static str {
        let ticks = elapsed.as_millis() / u128::from(BRAILLE_SPINNER_INTERVAL_MS);
        BRAILLE_SPINNER_FRAMES[ticks as usize % BRAILLE_SPINNER_FRAMES.len()]
    }

    pub fn render(message: impl Into<String>, elapsed: StdDuration) -> Line<'static> {
        let message = message.into();
        Line::from(vec![
            Span::styled(Self::frame(elapsed), Style::new().fg(BRAILLE_SPINNER_COLOR)),
            Span::raw(" "),
            Span::raw(message),
        ])
    }
}

#[cfg(test)]
mod animations_widgets_tests {
    use std::time::Duration as StdDuration;

    use super::*;

    #[test]
    fn spinner_cycles_every_80ms() {
        assert_eq!(BrailleSpinner::frame(StdDuration::from_millis(0)), "⣾");
        assert_eq!(BrailleSpinner::frame(StdDuration::from_millis(79)), "⣾");
        assert_eq!(BrailleSpinner::frame(StdDuration::from_millis(80)), "⣽");
        assert_eq!(BrailleSpinner::frame(StdDuration::from_millis(160)), "⣻");
        assert_eq!(BrailleSpinner::frame(StdDuration::from_millis(640)), "⣾");
    }

    #[test]
    fn spinner_renders_in_spec_color() {
        let line = BrailleSpinner::render("Scanning sessions...", StdDuration::from_millis(80));

        assert_eq!(line.to_string(), "⣽ Scanning sessions...");
        assert_eq!(line.spans[0].style.fg, Some(BRAILLE_SPINNER_COLOR));
    }
}
