use crossterm::style::{Color as CrosstermColor, Stylize};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

pub const BANNER_COLOR: Color = Color::Rgb(0x58, 0xa6, 0xff);
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn ascii_banner() -> String {
    format!(
        concat!(
            "   _____ _                 _        ___           _       _     _\n",
            "  / ____| |               | |      |_ _|_ __  ___(_) __ _| |__ | |_\n",
            " | |    | | __ _ _   _  __| | ___   | || '_ \\/ __| |/ _` | '_ \\| __|\n",
            " | |____| |/ _` | | | |/ _` |/ _ \\  | || | | \\__ \\ | (_| | | | | |_\n",
            "  \\_____|_|\\__,_|\\__,_|\\__,_|\\___| |___|_| |_|___/_|\\__, |_| |_|\\__|\n",
            "                                                      |___/\n",
            "  Local observability for Claude Code          v{}"
        ),
        VERSION
    )
}

pub fn banner_width() -> usize {
    ascii_banner()
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0)
}

pub fn banner_lines() -> Vec<Line<'static>> {
    ascii_banner()
        .lines()
        .map(|line| {
            Line::from(Span::styled(
                line.to_string(),
                Style::new().fg(BANNER_COLOR),
            ))
        })
        .collect()
}

pub fn ansi_banner() -> String {
    let mut rendered = String::new();
    let color = CrosstermColor::Rgb {
        r: 0x58,
        g: 0xa6,
        b: 0xff,
    };

    for line in ascii_banner().lines() {
        rendered.push_str(&format!("{}\n", line.with(color)));
    }

    rendered
}

#[cfg(test)]
mod animations_widgets_tests {
    use super::*;

    #[test]
    fn banner_renders_without_wrapping_at_60_columns() {
        assert!(banner_width() >= 60);
        assert_eq!(banner_lines().len(), ascii_banner().lines().count());
        assert!(banner_lines()
            .iter()
            .all(|line| line.spans[0].style.fg == Some(BANNER_COLOR)));
    }

    #[test]
    fn banner_uses_package_version() {
        assert!(ascii_banner().contains(&format!("v{}", VERSION)));
    }
}
