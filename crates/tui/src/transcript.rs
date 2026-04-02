use std::collections::BTreeSet;

use ratatui::{
    layout::{Margin, Rect},
    prelude::Frame,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::session_list::{
    ACCENT_AMBER, ACCENT_CYAN, BACKGROUND, BORDER, SURFACE, TEXT_DIM, TEXT_PRIMARY,
};

pub const ACCENT_PURPLE: Color = Color::Rgb(0xbc, 0x8c, 0xff);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayTranscript {
    entries: Vec<TranscriptEntry>,
    expanded_tools: BTreeSet<usize>,
    collapsed_subagents: BTreeSet<usize>,
}

impl ReplayTranscript {
    pub fn new(entries: Vec<TranscriptEntry>) -> Self {
        Self {
            entries,
            expanded_tools: BTreeSet::new(),
            collapsed_subagents: BTreeSet::new(),
        }
    }

    pub fn from_session_events(event_count: usize) -> Self {
        let entries = if event_count == 0 {
            Vec::new()
        } else {
            (0..event_count)
                .map(|event_index| {
                    TranscriptEntry::assistant(
                        event_index,
                        format!("Captured replay event {}.", event_index + 1),
                    )
                })
                .collect()
        };
        Self::new(entries)
    }

    pub fn toggle_selected_entry(&mut self, selected_event: usize) -> bool {
        let Some(entry_index) = self.selected_entry_index(selected_event) else {
            return false;
        };

        match &self.entries[entry_index].kind {
            TranscriptEntryKind::ToolCall(_) => {
                if !self.expanded_tools.insert(entry_index) {
                    self.expanded_tools.remove(&entry_index);
                }
                true
            }
            TranscriptEntryKind::SubagentHeader(header) => {
                if !self.collapsed_subagents.insert(header.section_id) {
                    self.collapsed_subagents.remove(&header.section_id);
                }
                true
            }
            TranscriptEntryKind::Message(_) => false,
        }
    }

    pub fn reveal_selected_event(&mut self, selected_event: usize) {
        if let Some(section_id) = self.section_for_event(selected_event) {
            self.collapsed_subagents.remove(&section_id);
        }
    }

    pub fn selected_entry_index(&self, selected_event: usize) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.event_index == selected_event)
    }

    pub fn is_tool_expanded(&self, entry_index: usize) -> bool {
        self.expanded_tools.contains(&entry_index)
    }

    pub fn is_subagent_collapsed(&self, section_id: usize) -> bool {
        self.collapsed_subagents.contains(&section_id)
    }

    pub fn build_lines(&self, content_width: u16, selected_event: usize) -> TranscriptLayout {
        if self.entries.is_empty() {
            return TranscriptLayout {
                lines: vec![Line::from(Span::styled(
                    "No transcript found for this session.",
                    Style::new().fg(TEXT_DIM),
                ))],
                selected_line: 0,
            };
        }

        let mut lines = Vec::new();
        let mut selected_line = 0usize;

        for (entry_index, entry) in self.entries.iter().enumerate() {
            let hidden_by_collapse = entry
                .section_id
                .is_some_and(|section_id| self.collapsed_subagents.contains(&section_id))
                && !matches!(entry.kind, TranscriptEntryKind::SubagentHeader(_));
            if hidden_by_collapse {
                continue;
            }

            if entry.event_index == selected_event {
                selected_line = lines.len();
            }

            let selected = entry.event_index == selected_event;
            let entry_lines = match &entry.kind {
                TranscriptEntryKind::Message(message) => {
                    render_message_lines(message, entry.indent, content_width, selected)
                }
                TranscriptEntryKind::ToolCall(tool) => render_tool_lines(
                    tool,
                    entry.indent,
                    content_width,
                    self.expanded_tools.contains(&entry_index),
                    selected,
                ),
                TranscriptEntryKind::SubagentHeader(header) => render_subagent_header_lines(
                    header,
                    content_width,
                    self.collapsed_subagents.contains(&header.section_id),
                    selected,
                ),
            };

            lines.extend(entry_lines);
        }

        TranscriptLayout {
            lines,
            selected_line,
        }
    }

    fn section_for_event(&self, selected_event: usize) -> Option<usize> {
        self.entries
            .iter()
            .find(|entry| entry.event_index == selected_event)
            .and_then(|entry| entry.section_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptLayout {
    pub lines: Vec<Line<'static>>,
    pub selected_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    pub event_index: usize,
    pub indent: u16,
    pub section_id: Option<usize>,
    pub kind: TranscriptEntryKind,
}

impl TranscriptEntry {
    pub fn user(event_index: usize, text: impl Into<String>) -> Self {
        Self {
            event_index,
            indent: 0,
            section_id: None,
            kind: TranscriptEntryKind::Message(TranscriptMessage {
                speaker: TranscriptSpeaker::User,
                text: text.into(),
            }),
        }
    }

    pub fn assistant(event_index: usize, text: impl Into<String>) -> Self {
        Self {
            event_index,
            indent: 0,
            section_id: None,
            kind: TranscriptEntryKind::Message(TranscriptMessage {
                speaker: TranscriptSpeaker::Assistant,
                text: text.into(),
            }),
        }
    }

    pub fn tool(
        event_index: usize,
        name: impl Into<String>,
        input_kind: ToolInputKind,
        input_summary: impl Into<String>,
        output_summary: impl Into<String>,
    ) -> Self {
        Self {
            event_index,
            indent: 0,
            section_id: None,
            kind: TranscriptEntryKind::ToolCall(ToolCallEntry {
                name: name.into(),
                input_kind,
                input_summary: input_summary.into(),
                output_summary: output_summary.into(),
            }),
        }
    }

    pub fn subagent_header(
        event_index: usize,
        section_id: usize,
        agent_type: impl Into<String>,
    ) -> Self {
        Self {
            event_index,
            indent: 0,
            section_id: Some(section_id),
            kind: TranscriptEntryKind::SubagentHeader(SubagentHeaderEntry {
                section_id,
                agent_type: agent_type.into(),
            }),
        }
    }

    pub fn nested_message(
        event_index: usize,
        section_id: usize,
        speaker: TranscriptSpeaker,
        text: impl Into<String>,
    ) -> Self {
        Self {
            event_index,
            indent: 1,
            section_id: Some(section_id),
            kind: TranscriptEntryKind::Message(TranscriptMessage {
                speaker,
                text: text.into(),
            }),
        }
    }

    pub fn nested_tool(
        event_index: usize,
        section_id: usize,
        name: impl Into<String>,
        input_kind: ToolInputKind,
        input_summary: impl Into<String>,
        output_summary: impl Into<String>,
    ) -> Self {
        Self {
            event_index,
            indent: 1,
            section_id: Some(section_id),
            kind: TranscriptEntryKind::ToolCall(ToolCallEntry {
                name: name.into(),
                input_kind,
                input_summary: input_summary.into(),
                output_summary: output_summary.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptEntryKind {
    Message(TranscriptMessage),
    ToolCall(ToolCallEntry),
    SubagentHeader(SubagentHeaderEntry),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptSpeaker {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptMessage {
    pub speaker: TranscriptSpeaker,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolInputKind {
    Command,
    File,
}

impl ToolInputKind {
    fn label(self) -> &'static str {
        match self {
            Self::Command => "command:",
            Self::File => "file:",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallEntry {
    pub name: String,
    pub input_kind: ToolInputKind,
    pub input_summary: String,
    pub output_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentHeaderEntry {
    pub section_id: usize,
    pub agent_type: String,
}

pub fn render_transcript_pane(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &'static str,
    active: bool,
    transcript: &ReplayTranscript,
    selected_event: usize,
) {
    let block = Block::default()
        .title(Line::from(title).bold())
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::new().fg(if active { ACCENT_CYAN } else { BORDER }))
        .style(Style::new().bg(SURFACE).fg(TEXT_PRIMARY));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let content_area = inner.inner(Margin::new(1, 0));
    let layout = transcript.build_lines(content_area.width.max(1), selected_event);
    let height = usize::from(content_area.height.max(1));
    let scroll = layout
        .selected_line
        .saturating_sub(height.saturating_sub(1) / 2);

    frame.render_widget(
        Paragraph::new(layout.lines)
            .style(Style::new().bg(BACKGROUND))
            .scroll((scroll as u16, 0)),
        content_area,
    );
}

fn render_message_lines(
    message: &TranscriptMessage,
    indent: u16,
    content_width: u16,
    selected: bool,
) -> Vec<Line<'static>> {
    let (prefix, prefix_style) = match message.speaker {
        TranscriptSpeaker::User => (
            "USER:",
            Style::new().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ),
        TranscriptSpeaker::Assistant => (
            "CLAUDE:",
            Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
        ),
    };
    let entry_style = entry_text_style(selected);
    let available_width = content_width
        .saturating_sub(left_prefix_width(indent))
        .saturating_sub(prefix.width() as u16 + 1)
        .max(1);
    let wrapped = wrap_plain_text(&message.text, available_width as usize);

    wrapped
        .into_iter()
        .enumerate()
        .map(|(line_index, segment)| {
            let mut spans = left_prefix_spans(indent);
            if line_index == 0 {
                spans.push(Span::styled(prefix.to_string(), prefix_style));
                spans.push(Span::raw(" "));
            } else {
                spans.push(Span::raw(format!(
                    "{:width$}",
                    "",
                    width = prefix.width() + 1
                )));
            }
            spans.push(Span::styled(segment, entry_style));
            Line::from(spans)
        })
        .collect()
}

fn render_tool_lines(
    tool: &ToolCallEntry,
    indent: u16,
    content_width: u16,
    expanded: bool,
    selected: bool,
) -> Vec<Line<'static>> {
    let left_prefix = left_prefix_width(indent);
    let box_width = content_width.saturating_sub(left_prefix).max(12) as usize;
    let inner_width = box_width.saturating_sub(2).max(1);
    let input_lines = wrap_plain_text(&tool.input_summary, inner_width.saturating_sub(9));
    let output_lines = wrap_plain_text(&tool.output_summary, inner_width.saturating_sub(3));
    let collapsed_input = truncate_lines(input_lines, 3);
    let collapsed_output = truncate_lines(output_lines, 3);
    let input_to_render = if expanded {
        wrap_plain_text(&tool.input_summary, inner_width.saturating_sub(9))
    } else {
        collapsed_input
    };
    let output_to_render = if expanded {
        wrap_plain_text(&tool.output_summary, inner_width.saturating_sub(3))
    } else {
        collapsed_output
    };
    let content_style = entry_text_style(selected);

    let mut lines = vec![tool_border_top(tool, indent, box_width, selected)];

    lines.extend(render_tool_section(
        indent,
        inner_width,
        tool.input_kind.label(),
        &input_to_render,
        content_style,
    ));
    lines.extend(render_tool_section(
        indent,
        inner_width,
        "→",
        &output_to_render,
        content_style,
    ));

    lines.push(tool_border_bottom(indent, box_width, selected));
    lines
}

fn render_tool_section(
    indent: u16,
    inner_width: usize,
    label: &str,
    lines: &[String],
    content_style: Style,
) -> Vec<Line<'static>> {
    lines
        .iter()
        .enumerate()
        .map(|(line_index, segment)| {
            let body = if line_index == 0 {
                format!("{label} {segment}")
            } else {
                format!("{:width$}{segment}", "", width = label.width() + 1)
            };
            let padded = pad_to_width(&body, inner_width);

            let mut spans = left_prefix_spans(indent);
            spans.push(Span::styled("│".to_string(), Style::new().fg(BORDER)));
            spans.push(Span::styled(padded, content_style));
            spans.push(Span::styled("│".to_string(), Style::new().fg(BORDER)));
            Line::from(spans)
        })
        .collect()
}

fn render_subagent_header_lines(
    header: &SubagentHeaderEntry,
    content_width: u16,
    collapsed: bool,
    selected: bool,
) -> Vec<Line<'static>> {
    let available_width = content_width.saturating_sub(left_prefix_width(0)).max(1) as usize;
    let state = if collapsed { "collapsed" } else { "expanded" };
    let header_line = format!("🤖 Agent: {} [{}]", header.agent_type, state);
    let header_text = truncate_single_line(&header_line, available_width);
    let info_text = truncate_single_line(
        "Use e to collapse or expand this subagent section.",
        available_width,
    );
    let text_style = entry_text_style(selected);

    vec![
        Line::from(vec![
            Span::styled("▏ ".to_string(), Style::new().fg(ACCENT_PURPLE)),
            Span::styled(
                header_text,
                text_style.fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("▏ ".to_string(), Style::new().fg(ACCENT_PURPLE)),
            Span::styled(info_text, Style::new().fg(TEXT_DIM)),
        ]),
    ]
}

fn tool_border_top(
    tool: &ToolCallEntry,
    indent: u16,
    box_width: usize,
    selected: bool,
) -> Line<'static> {
    let mut spans = left_prefix_spans(indent);
    let border_style = if selected {
        Style::new().fg(BORDER).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(BORDER)
    };
    let label = "Tool: ";
    let name = truncate_single_line(&tool.name, box_width.saturating_sub(label.width() + 6));
    let filler = "─".repeat(box_width.saturating_sub(label.width() + name.width() + 4));

    spans.push(Span::styled("┌─ ".to_string(), border_style));
    spans.push(Span::styled(label.to_string(), border_style));
    spans.push(Span::styled(
        name,
        Style::new().fg(ACCENT_AMBER).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(format!(" {filler}┐"), border_style));
    Line::from(spans)
}

fn tool_border_bottom(indent: u16, box_width: usize, selected: bool) -> Line<'static> {
    let mut spans = left_prefix_spans(indent);
    let border_style = if selected {
        Style::new().fg(BORDER).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(BORDER)
    };
    spans.push(Span::styled("└".to_string(), border_style));
    spans.push(Span::styled(
        "─".repeat(box_width.saturating_sub(2)),
        border_style,
    ));
    spans.push(Span::styled("┘".to_string(), border_style));
    Line::from(spans)
}

fn left_prefix_spans(indent: u16) -> Vec<Span<'static>> {
    if indent == 0 {
        Vec::new()
    } else {
        vec![
            Span::styled("▏ ".to_string(), Style::new().fg(ACCENT_PURPLE)),
            Span::raw("  ".repeat(indent as usize)),
        ]
    }
}

fn left_prefix_width(indent: u16) -> u16 {
    if indent == 0 {
        0
    } else {
        2 + indent * 2
    }
}

fn entry_text_style(selected: bool) -> Style {
    let style = Style::new().fg(TEXT_PRIMARY);
    if selected {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn truncate_lines(mut lines: Vec<String>, max_lines: usize) -> Vec<String> {
    if lines.len() <= max_lines {
        return lines;
    }

    lines.truncate(max_lines);
    if let Some(last) = lines.last_mut() {
        if !last.ends_with("...") {
            if last.width() > 3 {
                let mut truncated = String::new();
                for ch in last.chars() {
                    if truncated.width() + ch.len_utf8() >= last.width().saturating_sub(3) {
                        break;
                    }
                    truncated.push(ch);
                }
                *last = format!("{truncated}...");
            } else {
                *last = "...".to_string();
            }
        }
    }

    lines
}

fn truncate_single_line(text: &str, width: usize) -> String {
    let mut result = String::new();
    for ch in text.chars() {
        let ch_width = ch.len_utf8();
        if result.width() + ch_width > width.saturating_sub(3) {
            break;
        }
        result.push(ch);
    }

    if text.width() > width {
        format!("{result}...")
    } else {
        text.to_string()
    }
}

fn wrap_plain_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut wrapped = Vec::new();

    for raw_line in text.lines() {
        if raw_line.is_empty() {
            wrapped.push(String::new());
            continue;
        }

        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            if current.is_empty() {
                if word.width() <= width {
                    current.push_str(word);
                } else {
                    wrapped.extend(chunk_word(word, width));
                }
                continue;
            }

            let candidate = format!("{current} {word}");
            if candidate.width() <= width {
                current = candidate;
            } else {
                wrapped.push(current);
                if word.width() <= width {
                    current = word.to_string();
                } else {
                    let mut chunks = chunk_word(word, width);
                    current = chunks.pop().unwrap_or_default();
                    wrapped.extend(chunks);
                }
            }
        }

        if !current.is_empty() {
            wrapped.push(current);
        }
    }

    if wrapped.is_empty() {
        wrapped.push(String::new());
    }

    wrapped
}

fn chunk_word(word: &str, width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for ch in word.chars() {
        let ch_width = ch.len_utf8();
        if current.width() + ch_width > width && !current.is_empty() {
            chunks.push(current);
            current = String::new();
        }
        current.push(ch);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

fn pad_to_width(text: &str, width: usize) -> String {
    let padding = width.saturating_sub(text.width());
    format!("{text}{:padding$}", "", padding = padding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_message_prefix_styles_match_spec() {
        let user_lines = render_message_lines(
            &TranscriptMessage {
                speaker: TranscriptSpeaker::User,
                text: "Ship the transcript pane.".to_string(),
            },
            0,
            48,
            false,
        );
        let assistant_lines = render_message_lines(
            &TranscriptMessage {
                speaker: TranscriptSpeaker::Assistant,
                text: "Implemented the transcript pane.".to_string(),
            },
            0,
            48,
            false,
        );

        assert_eq!(user_lines[0].spans[0].content.as_ref(), "USER:");
        assert_eq!(assistant_lines[0].spans[0].content.as_ref(), "CLAUDE:");
        assert_eq!(user_lines[0].spans[0].style.fg, Some(ACCENT_CYAN));
        assert_eq!(assistant_lines[0].spans[0].style.fg, Some(TEXT_PRIMARY));
    }

    #[test]
    fn transcript_toggle_expands_selected_tool_card() {
        let mut transcript = ReplayTranscript::new(vec![TranscriptEntry::tool(
            0,
            "exec_command",
            ToolInputKind::Command,
            "cargo test -p tui -- transcript",
            "ok",
        )]);

        assert!(transcript.toggle_selected_entry(0));
        assert!(transcript.is_tool_expanded(0));

        assert!(transcript.toggle_selected_entry(0));
        assert!(!transcript.is_tool_expanded(0));
    }

    #[test]
    fn transcript_reveals_collapsed_subagent_for_selected_child_event() {
        let section_id = 7;
        let mut transcript = ReplayTranscript::new(vec![
            TranscriptEntry::subagent_header(0, section_id, "researcher"),
            TranscriptEntry::nested_message(
                1,
                section_id,
                TranscriptSpeaker::Assistant,
                "Subagent result.",
            ),
        ]);

        assert!(transcript.toggle_selected_entry(0));
        assert!(transcript.is_subagent_collapsed(section_id));

        transcript.reveal_selected_event(1);
        assert!(!transcript.is_subagent_collapsed(section_id));
    }

    #[test]
    fn transcript_layout_contains_tool_summary_and_subagent_header() {
        let transcript = ReplayTranscript::new(vec![
            TranscriptEntry::user(0, "Inspect the transcript pane."),
            TranscriptEntry::tool(
                1,
                "read_file",
                ToolInputKind::File,
                "crates/tui/src/replay.rs",
                "render_transcript currently shows placeholder content",
            ),
            TranscriptEntry::subagent_header(2, 11, "review"),
            TranscriptEntry::nested_message(
                3,
                11,
                TranscriptSpeaker::Assistant,
                "Subagent reviewed the tool card output.",
            ),
        ]);

        let layout = transcript.build_lines(52, 1);
        let rendered = layout
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("┌─ Tool: read_file"));
        assert!(rendered.contains("file: crates/tui/src/replay.rs"));
        assert!(rendered.contains("→ render_transcript currently shows"));
        assert!(rendered.contains("🤖 Agent: review [expanded]"));
    }
}
