use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::OnceLock,
};

use ratatui::{
    prelude::Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use serde_json::Value;
use syntect::{
    easy::HighlightLines,
    highlighting::{
        Color as SyntectColor, FontStyle, StyleModifier, Theme, ThemeItem, ThemeSettings,
    },
    parsing::SyntaxSet,
};
use time::OffsetDateTime;

use crate::causal_chain::CausalChainState;
use crate::session_list::{
    SessionEvent, ACCENT_AMBER, ACCENT_CYAN, ACCENT_GREEN, ACCENT_RED, BACKGROUND, SURFACE,
    TEXT_DIM, TEXT_PRIMARY,
};

pub const ACCENT_PURPLE: ratatui::style::Color = ratatui::style::Color::Rgb(0xbc, 0x8c, 0xff);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedEvent {
    pub event_type: String,
    pub timestamp: OffsetDateTime,
    pub event_index: Option<usize>,
}

impl LinkedEvent {
    pub fn new(event_type: impl Into<String>, timestamp: OffsetDateTime) -> Self {
        Self {
            event_type: event_type.into(),
            timestamp,
            event_index: None,
        }
    }

    pub fn with_event_index(mut self, event_index: usize) -> Self {
        self.event_index = Some(event_index);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecisionKind {
    Allow,
    Deny,
    Request,
}

impl PermissionDecisionKind {
    fn label(self) -> &'static str {
        match self {
            Self::Allow => "ALLOW",
            Self::Deny => "DENY",
            Self::Request => "REQUEST",
        }
    }

    fn style(self) -> Style {
        match self {
            Self::Allow => Style::new().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD),
            Self::Deny => Style::new().fg(ACCENT_RED).add_modifier(Modifier::BOLD),
            Self::Request => Style::new().fg(ACCENT_AMBER).add_modifier(Modifier::BOLD),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDetails {
    pub decision: PermissionDecisionKind,
    pub source: Option<String>,
    pub rule_text: Option<String>,
    pub permission_mode: Option<String>,
}

impl PermissionDetails {
    pub fn new(decision: PermissionDecisionKind) -> Self {
        Self {
            decision,
            source: None,
            rule_text: None,
            permission_mode: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionProvenance {
    pub file_path: String,
    pub memory_type: Option<String>,
    pub load_reason: Option<String>,
}

impl InstructionProvenance {
    pub fn new(file_path: impl Into<String>) -> Self {
        Self {
            file_path: file_path.into(),
            memory_type: None,
            load_reason: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EvidenceDetails {
    pub raw_json: String,
    pub linked_events: Vec<LinkedEvent>,
    pub permission: Option<PermissionDetails>,
    pub instruction_provenance: Option<InstructionProvenance>,
    pub file_path: Option<PathBuf>,
}

impl EvidenceDetails {
    pub fn with_raw_json(mut self, event_type: &str, raw_json: impl Into<String>) -> Self {
        self.raw_json = raw_json.into();
        self.populate_inferred_fields(event_type);
        self
    }

    pub fn primary_file_path(&self) -> Option<&Path> {
        self.file_path.as_deref().or_else(|| {
            self.instruction_provenance
                .as_ref()
                .map(|item| Path::new(&item.file_path))
        })
    }

    fn populate_inferred_fields(&mut self, event_type: &str) {
        let value = match serde_json::from_str::<Value>(&self.raw_json) {
            Ok(value) => value,
            Err(_) => return,
        };

        if self.file_path.is_none() {
            self.file_path = extract_file_path(&value).map(PathBuf::from);
        }

        if self.permission.is_none() {
            self.permission = infer_permission_details(event_type, &value);
        }

        if self.instruction_provenance.is_none() {
            self.instruction_provenance = infer_instruction_provenance(&value);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidencePaneState {
    pub scroll: u16,
    pub show_linked_events: bool,
}

impl Default for EvidencePaneState {
    fn default() -> Self {
        Self {
            scroll: 0,
            show_linked_events: true,
        }
    }
}

impl EvidencePaneState {
    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    pub fn reset_scroll(&mut self) {
        self.scroll = 0;
    }

    pub fn toggle_linked_events(&mut self) {
        self.show_linked_events = !self.show_linked_events;
    }
}

pub fn render(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    event: Option<&SessionEvent>,
    pane: &EvidencePaneState,
    causal_chain: Option<&CausalChainState>,
) {
    let lines = build_lines(event, pane, causal_chain);
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().bg(SURFACE).fg(TEXT_PRIMARY))
            .scroll((pane.scroll, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub fn copy_event_json(event: &SessionEvent) -> io::Result<()> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| io::Error::other(format!("clipboard unavailable: {error}")))?;
    clipboard
        .set_text(event.evidence().raw_json.clone())
        .map_err(|error| io::Error::other(format!("failed to copy JSON to clipboard: {error}")))
}

pub fn open_event_file_path(event: &SessionEvent) -> io::Result<()> {
    let editor = std::env::var("EDITOR")
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "EDITOR is not set"))?;
    let path = event
        .evidence()
        .primary_file_path()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "event payload has no file_path"))?;

    if !path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("file does not exist: {}", path.display()),
        ));
    }

    Command::new(editor)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| io::Error::other(format!("failed to spawn editor: {error}")))
}

fn build_lines(
    event: Option<&SessionEvent>,
    pane: &EvidencePaneState,
    causal_chain: Option<&CausalChainState>,
) -> Vec<Line<'static>> {
    let Some(event) = event else {
        return vec![Line::from(Span::styled(
            "Select an event to see details",
            Style::new().fg(TEXT_DIM),
        ))];
    };

    let mut lines = vec![
        Line::from(Span::styled(
            event.event_type().to_string(),
            Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(
                "{}  {}",
                event.kind_icon(),
                format_tree_time(event.timestamp)
            ),
            Style::new().fg(TEXT_DIM),
        )),
        Line::from(""),
    ];

    append_permission(lines.as_mut(), event.evidence().permission.as_ref());
    append_instruction_provenance(
        lines.as_mut(),
        event.evidence().instruction_provenance.as_ref(),
    );
    append_linked_events(
        lines.as_mut(),
        &event.evidence().linked_events,
        pane.show_linked_events,
        causal_chain,
    );
    append_raw_json(lines.as_mut(), &event.evidence().raw_json);

    lines
}

fn append_permission(lines: &mut Vec<Line<'static>>, permission: Option<&PermissionDetails>) {
    let Some(permission) = permission else {
        return;
    };

    push_section_heading(lines, "Permission");
    lines.push(Line::from(vec![
        Span::styled("Decision: ", Style::new().fg(TEXT_DIM)),
        Span::styled(permission.decision.label(), permission.decision.style()),
    ]));
    lines.push(key_value_line(
        "Source",
        permission.source.as_deref().unwrap_or("unknown"),
    ));
    lines.push(key_value_line(
        "Rule",
        permission.rule_text.as_deref().unwrap_or("n/a"),
    ));
    lines.push(key_value_line(
        "Mode",
        permission.permission_mode.as_deref().unwrap_or("n/a"),
    ));
    lines.push(Line::from(""));
}

fn append_instruction_provenance(
    lines: &mut Vec<Line<'static>>,
    provenance: Option<&InstructionProvenance>,
) {
    let Some(provenance) = provenance else {
        return;
    };

    push_section_heading(lines, "Instruction Provenance");
    lines.push(key_value_line("File", provenance.file_path.as_str()));
    lines.push(key_value_line(
        "Memory",
        provenance.memory_type.as_deref().unwrap_or("n/a"),
    ));
    lines.push(key_value_line(
        "Reason",
        provenance.load_reason.as_deref().unwrap_or("n/a"),
    ));
    lines.push(Line::from(""));
}

fn append_linked_events(
    lines: &mut Vec<Line<'static>>,
    linked_events: &[LinkedEvent],
    show: bool,
    causal_chain: Option<&CausalChainState>,
) {
    if linked_events.is_empty() && show {
        return;
    }

    push_section_heading(lines, "Linked Events");
    if !show {
        lines.push(Line::from(Span::styled(
            "Hidden. Press l to toggle the tree.",
            Style::new().fg(TEXT_DIM),
        )));
        lines.push(Line::from(""));
        return;
    }

    for (index, event) in linked_events.iter().enumerate() {
        let marker = if index + 1 == linked_events.len() {
            "└──"
        } else {
            "├──"
        };
        let line = Line::from(vec![
            Span::styled(format!("{marker} "), Style::new().fg(TEXT_DIM)),
            Span::styled(event.event_type.clone(), Style::new().fg(TEXT_PRIMARY)),
            Span::styled(
                format!(" ({})", format_tree_time(event.timestamp)),
                Style::new().fg(TEXT_DIM),
            ),
        ]);
        let relation = event.event_index.and_then(|event_index| {
            causal_chain.and_then(|chain| chain.highlight_for(event_index))
        });
        lines.push(apply_line_highlight(line, relation));
    }
    lines.push(Line::from(""));
}

fn append_raw_json(lines: &mut Vec<Line<'static>>, raw_json: &str) {
    push_section_heading(lines, "Raw JSON");
    lines.extend(highlight_json(raw_json));
}

fn push_section_heading(lines: &mut Vec<Line<'static>>, heading: &str) {
    lines.push(Line::from(Span::styled(
        heading.to_string(),
        Style::new().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
    )));
}

fn key_value_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::new().fg(TEXT_DIM)),
        Span::styled(value.to_string(), Style::new().fg(TEXT_PRIMARY)),
    ])
}

fn highlight_json(raw_json: &str) -> Vec<Line<'static>> {
    let pretty = match serde_json::from_str::<Value>(raw_json) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw_json.to_string()),
        Err(_) => raw_json.to_string(),
    };

    let syntax_set = syntax_set();
    let syntax = syntax_set
        .find_syntax_by_extension("json")
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, json_theme());

    let mut lines = Vec::new();
    for line in pretty.lines() {
        match highlighter.highlight_line(line, syntax_set) {
            Ok(_) => lines.push(Line::from(scan_json_line(line))),
            Err(_) => lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::new().fg(TEXT_PRIMARY),
            ))),
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled("{}", Style::new().fg(TEXT_DIM))));
    }

    lines
}

fn infer_permission_details(event_type: &str, value: &Value) -> Option<PermissionDetails> {
    match event_type {
        "PermissionRequest" => Some(PermissionDetails {
            decision: PermissionDecisionKind::Request,
            source: Some("hook".to_string()),
            rule_text: permission_rule_text(value),
            permission_mode: value
                .get("permission_mode")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
        "PermissionDenied" => Some(PermissionDetails {
            decision: PermissionDecisionKind::Deny,
            source: Some("hook".to_string()),
            rule_text: value
                .get("reason")
                .and_then(Value::as_str)
                .map(str::to_owned),
            permission_mode: value
                .get("permission_mode")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
        _ => None,
    }
}

fn permission_rule_text(value: &Value) -> Option<String> {
    value
        .get("permission_suggestions")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("rule").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|text| !text.is_empty())
}

fn infer_instruction_provenance(value: &Value) -> Option<InstructionProvenance> {
    let file_path = value.get("file_path").and_then(Value::as_str)?;
    Some(InstructionProvenance {
        file_path: file_path.to_string(),
        memory_type: value
            .get("memory_type")
            .and_then(Value::as_str)
            .map(str::to_owned),
        load_reason: value
            .get("load_reason")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn extract_file_path(value: &Value) -> Option<&str> {
    value.get("file_path").and_then(Value::as_str).or_else(|| {
        value
            .get("tool_input")
            .and_then(|item| item.get("file_path"))
            .and_then(Value::as_str)
    })
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn json_theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| Theme {
        name: Some("Claude Insight JSON".to_string()),
        author: Some("Codex".to_string()),
        settings: ThemeSettings {
            foreground: Some(syntect_color(TEXT_PRIMARY)),
            background: Some(syntect_color(BACKGROUND)),
            ..ThemeSettings::default()
        },
        scopes: vec![
            theme_item(
                "meta.mapping.key.json string.quoted.double.json, meta.mapping.key string.quoted.double",
                ACCENT_CYAN,
            ),
            theme_item("string.quoted.double.json, string.quoted.double", ACCENT_GREEN),
            theme_item("constant.numeric.json, constant.numeric", ACCENT_AMBER),
            theme_item(
                "constant.language.boolean.json, constant.language.null.json, constant.language.boolean, constant.language.null",
                ACCENT_PURPLE,
            ),
        ],
    })
}

fn theme_item(scope: &str, color: ratatui::style::Color) -> ThemeItem {
    ThemeItem {
        scope: syntect::highlighting::ScopeSelectors::from_str(scope).unwrap_or_default(),
        style: StyleModifier {
            foreground: Some(syntect_color(color)),
            background: None,
            font_style: Some(FontStyle::empty()),
        },
    }
}

fn format_tree_time(timestamp: OffsetDateTime) -> String {
    let (hour, minute, second) = timestamp.time().as_hms();
    format!("{hour:02}:{minute:02}:{second:02}")
}

fn syntect_color(color: ratatui::style::Color) -> SyntectColor {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => SyntectColor { r, g, b, a: 0xff },
        _ => SyntectColor {
            r: 0xc9,
            g: 0xd1,
            b: 0xd9,
            a: 0xff,
        },
    }
}

fn scan_json_line(line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0usize;
    let bytes = line.as_bytes();

    while index < bytes.len() {
        let current = bytes[index] as char;

        if current.is_ascii_whitespace() {
            let start = index;
            while index < bytes.len() && (bytes[index] as char).is_ascii_whitespace() {
                index += 1;
            }
            spans.push(styled_slice(line, start, index, TEXT_PRIMARY));
            continue;
        }

        if current == '"' {
            let start = index;
            index += 1;
            let mut escaped = false;
            while index < bytes.len() {
                let ch = bytes[index] as char;
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    index += 1;
                    break;
                }
                index += 1;
            }

            let mut lookahead = index;
            while lookahead < bytes.len() && (bytes[lookahead] as char).is_ascii_whitespace() {
                lookahead += 1;
            }
            let color = if lookahead < bytes.len() && bytes[lookahead] as char == ':' {
                ACCENT_CYAN
            } else {
                ACCENT_GREEN
            };
            spans.push(styled_slice(line, start, index, color));
            continue;
        }

        if current == '-' || current.is_ascii_digit() {
            let start = index;
            index += 1;
            while index < bytes.len() {
                let ch = bytes[index] as char;
                if ch.is_ascii_digit() || matches!(ch, '.' | 'e' | 'E' | '+' | '-') {
                    index += 1;
                } else {
                    break;
                }
            }
            spans.push(styled_slice(line, start, index, ACCENT_AMBER));
            continue;
        }

        if line[index..].starts_with("true") {
            spans.push(styled_slice(line, index, index + 4, ACCENT_PURPLE));
            index += 4;
            continue;
        }

        if line[index..].starts_with("false") {
            spans.push(styled_slice(line, index, index + 5, ACCENT_PURPLE));
            index += 5;
            continue;
        }

        if line[index..].starts_with("null") {
            spans.push(styled_slice(line, index, index + 4, ACCENT_PURPLE));
            index += 4;
            continue;
        }

        spans.push(styled_slice(
            line,
            index,
            index + current.len_utf8(),
            TEXT_PRIMARY,
        ));
        index += current.len_utf8();
    }

    spans
}

fn styled_slice(
    line: &str,
    start: usize,
    end: usize,
    color: ratatui::style::Color,
) -> Span<'static> {
    Span::styled(line[start..end].to_string(), Style::new().fg(color))
}

fn apply_line_highlight(
    line: Line<'static>,
    relation: Option<crate::causal_chain::CausalRelation>,
) -> Line<'static> {
    let Some(relation) = relation else {
        return line;
    };

    Line::from(
        line.spans
            .into_iter()
            .map(|span| {
                let style = relation.apply_to(span.style);
                Span::styled(span.content, style)
            })
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::causal_chain::{CausalChainState, CausalLink, CausalRelation};
    use crate::session_list::SessionEventKind;

    #[test]
    fn evidence_empty_state_is_dim() {
        let lines = build_lines(None, &EvidencePaneState::default(), None);

        assert_eq!(lines[0].to_string(), "Select an event to see details");
        assert_eq!(lines[0].spans[0].style.fg, Some(TEXT_DIM));
    }

    #[test]
    fn evidence_json_highlight_uses_ticket_palette() {
        let lines = highlight_json("{\"name\":\"Claude\",\"count\":7,\"ok\":true}");
        let rendered = lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("\"name\": \"Claude\""));

        let styles = lines
            .iter()
            .flat_map(|line| {
                line.spans
                    .iter()
                    .map(|span| (span.content.as_ref().to_string(), span.style.fg))
            })
            .collect::<Vec<_>>();

        assert!(styles
            .iter()
            .any(|(text, color)| text.contains("\"name\"") && *color == Some(ACCENT_CYAN)));
        assert!(styles
            .iter()
            .any(|(text, color)| text.contains("\"Claude\"") && *color == Some(ACCENT_GREEN)));
        assert!(styles
            .iter()
            .any(|(text, color)| text.contains('7') && *color == Some(ACCENT_AMBER)));
        assert!(styles
            .iter()
            .any(|(text, color)| text.contains("true") && *color == Some(ACCENT_PURPLE)));
    }

    #[test]
    fn evidence_linked_events_tree_uses_unicode_markers() {
        let event = SessionEvent::named(
            SessionEventKind::Tool,
            "PostToolUse",
            parse_timestamp("2026-04-03T14:32:05Z"),
        )
        .with_linked_events(vec![
            LinkedEvent::new("PreToolUse", parse_timestamp("2026-04-03T14:32:03Z"))
                .with_event_index(0),
            LinkedEvent::new("PostToolUse", parse_timestamp("2026-04-03T14:32:05Z"))
                .with_event_index(1),
        ]);

        let lines = build_lines(Some(&event), &EvidencePaneState::default(), None);
        let rendered = lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("├── PreToolUse (14:32:03)"));
        assert!(rendered.contains("└── PostToolUse (14:32:05)"));
    }

    #[test]
    fn evidence_causal_chain_highlights_all_linked_events() {
        let timestamps = [
            parse_timestamp("2026-04-03T01:06:40Z"),
            parse_timestamp("2026-04-03T01:06:50Z"),
            parse_timestamp("2026-04-03T01:07:00Z"),
            parse_timestamp("2026-04-03T01:07:10Z"),
            parse_timestamp("2026-04-03T01:07:20Z"),
        ];
        let event = SessionEvent::named(SessionEventKind::Tool, "PreToolUse", timestamps[2])
            .with_raw_event_id(3)
            .with_linked_events(vec![
                LinkedEvent::new("UserPromptSubmit", timestamps[0]).with_event_index(0),
                LinkedEvent::new("InstructionsLoaded", timestamps[1]).with_event_index(1),
                LinkedEvent::new("PreToolUse", timestamps[2]).with_event_index(2),
                LinkedEvent::new("PermissionRequest", timestamps[3]).with_event_index(3),
                LinkedEvent::new("PostToolUse", timestamps[4]).with_event_index(4),
            ]);
        let events = vec![
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
            event.clone(),
            SessionEvent::named(
                SessionEventKind::PermissionRequest,
                "PermissionRequest",
                timestamps[3],
            )
            .with_raw_event_id(4),
            SessionEvent::named(SessionEventKind::Tool, "PostToolUse", timestamps[4])
                .with_raw_event_id(5),
        ];
        let links = [
            CausalLink::new(1, 2),
            CausalLink::new(2, 3),
            CausalLink::new(3, 4),
            CausalLink::new(4, 5),
        ];
        let mut chain = CausalChainState::activate(2, &events, &links);
        chain.reveal_all_for_test();

        let lines = build_lines(Some(&event), &EvidencePaneState::default(), Some(&chain));
        let highlighted = lines
            .iter()
            .filter(|line| line.spans.iter().any(|span| span.style.bg.is_some()))
            .count();

        assert_eq!(highlighted, 5);
        assert_eq!(chain.highlight_for(2), Some(CausalRelation::Selected));
    }

    fn parse_timestamp(input: &str) -> OffsetDateTime {
        match OffsetDateTime::parse(input, &time::format_description::well_known::Rfc3339) {
            Ok(value) => value,
            Err(error) => panic!("failed to parse timestamp {input}: {error}"),
        }
    }
}
