use std::{path::PathBuf, time::Duration as StdDuration};

use claude_insight_tui::{
    evidence::{
        self, EvidenceDetails, EvidencePaneState, InstructionProvenance, LinkedEvent,
        PermissionDecisionKind, PermissionDetails,
    },
    replay::{ReplayView, ReplayViewState},
    session_list::{
        SessionEvent, SessionEventKind, SessionListItem, SessionListView, BACKGROUND, TEXT_PRIMARY,
    },
    timeline::TimelinePane,
    transcript::{
        render_transcript_pane, ReplayTranscript, ToolInputKind, TranscriptEntry, TranscriptSpeaker,
    },
    widgets::spinner::BrailleSpinner,
    wizard::WizardViewState,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    backend::TestBackend,
    prelude::{Buffer, Frame, Terminal},
    style::Style,
    widgets::Paragraph,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[test]
fn session_list_populated_snapshot() {
    let view = SessionListView::new(sample_sessions(), parse_timestamp("2026-04-03T01:10:00Z"));

    insta::assert_snapshot!(render_to_string(120, 40, |frame| view.render(frame, frame.area())));
}

#[test]
fn session_list_empty_snapshot() {
    let mut view = SessionListView::new(Vec::new(), parse_timestamp("2026-04-03T01:10:00Z"));
    view.set_empty_state_message("No sessions found");

    insta::assert_snapshot!(render_to_string(120, 40, |frame| view.render(frame, frame.area())));
}

#[test]
fn session_list_failed_to_load_snapshot() {
    let mut view = SessionListView::new(Vec::new(), parse_timestamp("2026-04-03T01:10:00Z"));
    view.set_empty_state_message("Failed to load sessions. Check daemon logs and try again.");

    insta::assert_snapshot!(render_to_string(120, 40, |frame| view.render(frame, frame.area())));
}

#[test]
fn session_list_filtered_snapshot() {
    let mut view = SessionListView::new(sample_sessions(), parse_timestamp("2026-04-03T01:10:00Z"));
    view.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
    view.handle_key_event(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    insta::assert_snapshot!(render_to_string(120, 40, |frame| view.render(frame, frame.area())));
}

#[test]
fn replay_3pane_snapshot() {
    let state = sample_replay_state();

    insta::assert_snapshot!(render_to_string(180, 50, |frame| {
        ReplayView::render(frame, frame.area(), &state);
    }));
}

#[test]
fn replay_2pane_snapshot() {
    let state = sample_replay_state();

    insta::assert_snapshot!(render_to_string(120, 40, |frame| {
        ReplayView::render(frame, frame.area(), &state);
    }));
}

#[test]
fn replay_1pane_snapshot() {
    let state = sample_replay_state();

    insta::assert_snapshot!(render_to_string(60, 30, |frame| {
        ReplayView::render(frame, frame.area(), &state);
    }));
}

#[test]
fn timeline_selected_snapshot() {
    let session = timeline_session();

    insta::assert_snapshot!(render_to_string(40, 30, |frame| {
        TimelinePane::render(frame, frame.area(), &session, 3, 0, true, None);
    }));
}

#[test]
fn transcript_tool_expanded_snapshot() {
    let mut transcript = rich_transcript();
    let _ = transcript.toggle_selected_entry(2);

    insta::assert_snapshot!(render_to_string(80, 30, |frame| {
        render_transcript_pane(
            frame,
            frame.area(),
            " Transcript [2] ",
            true,
            &transcript,
            2,
            None,
        );
    }));
}

#[test]
fn transcript_missing_data_snapshot() {
    let transcript = ReplayTranscript::new(Vec::new());

    insta::assert_snapshot!(render_to_string(80, 20, |frame| {
        render_transcript_pane(
            frame,
            frame.area(),
            " Transcript [2] ",
            false,
            &transcript,
            0,
            None,
        );
    }));
}

#[test]
fn evidence_json_snapshot() {
    let event = evidence_event();

    insta::assert_snapshot!(render_to_string(50, 30, |frame| {
        evidence::render(
            frame,
            frame.area(),
            Some(&event),
            &EvidencePaneState::default(),
            None,
        );
    }));
}

#[test]
fn evidence_missing_data_snapshot() {
    insta::assert_snapshot!(render_to_string(50, 12, |frame| {
        evidence::render(
            frame,
            frame.area(),
            None,
            &EvidencePaneState::default(),
            None,
        );
    }));
}

#[test]
fn wizard_step1_snapshot() {
    let wizard = WizardViewState::new();

    insta::assert_snapshot!(render_to_string(120, 40, |frame| wizard.render(frame, frame.area())));
}

#[test]
fn loading_spinner_snapshot() {
    insta::assert_snapshot!(render_to_string(120, 5, |frame| {
        frame.render_widget(
            Paragraph::new(BrailleSpinner::render(
                "Starting daemon...",
                StdDuration::from_millis(80),
            ))
            .style(Style::new().bg(BACKGROUND).fg(TEXT_PRIMARY)),
            frame.area(),
        );
    }));
}

fn render_to_string<F>(width: u16, height: u16, mut draw: F) -> String
where
    F: FnMut(&mut Frame<'_>),
{
    let mut backend = TestBackend::new(width, height);
    let terminal = Terminal::new(backend);
    let mut terminal = match terminal {
        Ok(terminal) => terminal,
        Err(error) => return format!("terminal error: {error}"),
    };

    let draw_result = terminal.draw(|frame| draw(frame));
    if let Err(error) = draw_result {
        return format!("draw error: {error}");
    }

    backend = terminal.backend().clone();
    buffer_to_string(backend.buffer())
}

fn buffer_to_string(buffer: &Buffer) -> String {
    let mut view = String::new();
    for y in 0..buffer.area.height {
        let mut skip = 0usize;
        for x in 0..buffer.area.width {
            let symbol = buffer[(x, y)].symbol();
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

fn sample_sessions() -> Vec<SessionListItem> {
    vec![
        SessionListItem::new(
            "session-1",
            "feature/replay-layout",
            parse_timestamp("2026-04-03T01:08:00Z"),
            0.42,
            vec![
                event(SessionEventKind::SessionBoundary, "2026-04-03T01:06:40Z"),
                event(SessionEventKind::Tool, "2026-04-03T01:07:00Z"),
                event(SessionEventKind::PermissionRequest, "2026-04-03T01:07:05Z"),
                event(SessionEventKind::PermissionDenied, "2026-04-03T01:07:10Z"),
            ],
        ),
        SessionListItem::new(
            "session-2",
            "main",
            parse_timestamp("2026-04-03T01:07:00Z"),
            0.18,
            vec![
                event(SessionEventKind::SessionBoundary, "2026-04-03T01:05:40Z"),
                event(SessionEventKind::UserPromptSubmit, "2026-04-03T01:06:00Z"),
                event(SessionEventKind::Tool, "2026-04-03T01:06:20Z"),
            ],
        ),
        SessionListItem::new(
            "session-3",
            "release/1.0",
            parse_timestamp("2026-04-02T20:01:00Z"),
            0.05,
            vec![
                event(SessionEventKind::InstructionsLoaded, "2026-04-02T19:58:00Z"),
                event(SessionEventKind::Retry, "2026-04-02T19:58:05Z"),
                event(SessionEventKind::Retry, "2026-04-02T19:58:10Z"),
            ],
        ),
        SessionListItem::new(
            "session-4",
            "main",
            parse_timestamp("2026-04-02T18:10:00Z"),
            0.01,
            vec![
                event(SessionEventKind::SessionBoundary, "2026-04-02T18:00:00Z"),
                event(SessionEventKind::Subagent, "2026-04-02T18:01:00Z"),
            ],
        ),
        SessionListItem::new(
            "session-5",
            "main",
            parse_timestamp("2026-04-02T10:30:00Z"),
            0.30,
            vec![
                event(SessionEventKind::SessionBoundary, "2026-04-02T10:10:00Z"),
                event(SessionEventKind::Tool, "2026-04-02T10:12:00Z"),
                event(SessionEventKind::Tool, "2026-04-02T10:13:00Z"),
                event(SessionEventKind::Tool, "2026-04-02T10:14:00Z"),
            ],
        ),
    ]
}

fn sample_replay_state() -> ReplayViewState {
    let session = SessionListItem::new(
        "session-1",
        "feature/replay-layout",
        parse_timestamp("2026-04-03T01:08:00Z"),
        0.42,
        vec![
            event(SessionEventKind::SessionBoundary, "2026-04-03T01:06:40Z"),
            event(SessionEventKind::Other, "2026-04-03T01:06:50Z"),
            SessionEvent::tool("tool-1", parse_timestamp("2026-04-03T01:07:00Z")),
            event(SessionEventKind::PermissionRequest, "2026-04-03T01:07:05Z"),
            SessionEvent::tool("tool-2", parse_timestamp("2026-04-03T01:07:07Z")),
            event(SessionEventKind::PermissionDenied, "2026-04-03T01:07:10Z"),
        ],
    );

    ReplayViewState::with_transcript(session, rich_transcript())
}

fn rich_transcript() -> ReplayTranscript {
    ReplayTranscript::new(vec![
        TranscriptEntry::user(0, "Build the transcript pane for this session replay."),
        TranscriptEntry::assistant(
            1,
            "I am rendering a real conversation view with tool cards and subagent sections.",
        ),
        TranscriptEntry::tool(
            2,
            "exec_command",
            ToolInputKind::Command,
            "cargo test -p tui -- transcript",
            "Compiling tui v0.1.0\nerror[E0004]: SessionEventKind::Retry not covered\nhelp: add a match arm for Retry",
        ),
        TranscriptEntry::subagent_header(3, 41, "review"),
        TranscriptEntry::nested_message(
            4,
            41,
            TranscriptSpeaker::Assistant,
            "Subagent reviewed the transcript widget output and confirmed the tool card shape.",
        ),
        TranscriptEntry::assistant(
            5,
            "The permission-denied event stays visible in the timeline while the transcript follows the selected entry.",
        ),
    ])
}

fn timeline_session() -> SessionListItem {
    SessionListItem::new(
        "session-9",
        "feature/timeline",
        parse_timestamp("2026-04-03T01:08:00Z"),
        0.42,
        vec![
            SessionEvent::new(
                SessionEventKind::SessionBoundary,
                parse_timestamp("2026-04-03T01:06:55Z"),
            )
            .with_label("Session start"),
            SessionEvent::new(
                SessionEventKind::UserPromptSubmit,
                parse_timestamp("2026-04-03T01:07:00Z"),
            )
            .with_label("Summarize the current branch"),
            SessionEvent::new(
                SessionEventKind::InstructionsLoaded,
                parse_timestamp("2026-04-03T01:07:05Z"),
            )
            .with_label("CLAUDE.md loaded"),
            SessionEvent::tool("tool-1", parse_timestamp("2026-04-03T01:07:10Z"))
                .with_label("Read crate graph"),
            SessionEvent::new(
                SessionEventKind::PermissionDenied,
                parse_timestamp("2026-04-03T01:07:15Z"),
            )
            .with_label("Permission denied"),
        ],
    )
}

fn evidence_event() -> SessionEvent {
    let mut permission = PermissionDetails::new(PermissionDecisionKind::Deny);
    permission.source = Some("workspace policy".to_string());
    permission.rule_text = Some("Write outside repo".to_string());
    permission.permission_mode = Some("ask".to_string());

    let mut provenance = InstructionProvenance::new(".claude/CLAUDE.md");
    provenance.memory_type = Some("project".to_string());
    provenance.load_reason = Some("auto".to_string());

    let evidence = EvidenceDetails {
        raw_json: r#"{
  "event_type": "PermissionDenied",
  "tool_name": "Write",
  "tool_input": {
    "file_path": "crates/tui/src/replay.rs"
  },
  "permission_mode": "ask",
  "reason": "Write outside repo"
}"#
        .to_string(),
        linked_events: vec![
            LinkedEvent::new("PreToolUse", parse_timestamp("2026-04-03T01:07:00Z")),
            LinkedEvent::new("PermissionRequest", parse_timestamp("2026-04-03T01:07:03Z")),
            LinkedEvent::new("PermissionDenied", parse_timestamp("2026-04-03T01:07:05Z")),
        ],
        permission: Some(permission),
        instruction_provenance: Some(provenance),
        file_path: Some(PathBuf::from("crates/tui/src/replay.rs")),
    };

    SessionEvent::named(
        SessionEventKind::PermissionDenied,
        "PermissionDenied",
        parse_timestamp("2026-04-03T01:07:05Z"),
    )
    .with_label("Permission denied")
    .with_evidence(evidence)
}

fn event(kind: SessionEventKind, timestamp: &str) -> SessionEvent {
    SessionEvent::new(kind, parse_timestamp(timestamp))
}

fn parse_timestamp(input: &str) -> OffsetDateTime {
    match OffsetDateTime::parse(input, &Rfc3339) {
        Ok(value) => value,
        Err(error) => panic!("failed to parse timestamp {input}: {error}"),
    }
}
