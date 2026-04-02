use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    time::Duration,
};

use ratatui::style::{Color, Modifier, Style};

use crate::session_list::{SessionEvent, ACCENT_CYAN};

pub const BEFORE_HIGHLIGHT_BG: Color = Color::Rgb(25, 40, 60);
pub const AFTER_HIGHLIGHT_BG: Color = Color::Rgb(20, 50, 30);
pub const SELECTED_HIGHLIGHT_BG: Color = ACCENT_CYAN;
pub const SELECTED_HIGHLIGHT_FG: Color = Color::White;
pub const PROPAGATION_DELAY_MS: u64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalRelation {
    Before,
    Selected,
    After,
}

impl CausalRelation {
    pub fn apply_to(self, style: Style) -> Style {
        match self {
            Self::Before => style.bg(BEFORE_HIGHLIGHT_BG),
            Self::Selected => style
                .bg(SELECTED_HIGHLIGHT_BG)
                .fg(SELECTED_HIGHLIGHT_FG)
                .add_modifier(Modifier::BOLD),
            Self::After => style.bg(AFTER_HIGHLIGHT_BG),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CausalLink {
    pub source_event_id: i64,
    pub target_event_id: i64,
}

impl CausalLink {
    pub fn new(source_event_id: i64, target_event_id: i64) -> Self {
        Self {
            source_event_id,
            target_event_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HighlightStep {
    relation: CausalRelation,
    reveal_step: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CausalChainState {
    active: bool,
    anchor_index: Option<usize>,
    elapsed: Duration,
    steps: BTreeMap<usize, HighlightStep>,
}

impl CausalChainState {
    pub fn activate(selected_index: usize, events: &[SessionEvent], links: &[CausalLink]) -> Self {
        let Some(anchor_event_id) = events
            .get(selected_index)
            .and_then(|event| event.raw_event_id)
        else {
            return Self {
                active: true,
                anchor_index: Some(selected_index),
                elapsed: Duration::ZERO,
                steps: BTreeMap::from([(
                    selected_index,
                    HighlightStep {
                        relation: CausalRelation::Selected,
                        reveal_step: 0,
                    },
                )]),
            };
        };

        let mut id_to_index = HashMap::new();
        for (index, event) in events.iter().enumerate() {
            if let Some(event_id) = event.raw_event_id {
                id_to_index.insert(event_id, index);
            }
        }

        let distances = bfs_distances(anchor_event_id, links);
        let mut reachable = distances
            .into_iter()
            .filter_map(|(event_id, distance)| {
                let index = *id_to_index.get(&event_id)?;
                let relation = if index < selected_index {
                    CausalRelation::Before
                } else if index > selected_index {
                    CausalRelation::After
                } else {
                    CausalRelation::Selected
                };
                Some((index, relation, distance, index.abs_diff(selected_index)))
            })
            .collect::<Vec<_>>();

        if reachable
            .iter()
            .all(|(index, _, _, _)| *index != selected_index)
        {
            reachable.push((selected_index, CausalRelation::Selected, 0, 0));
        }

        reachable.sort_by(|left, right| (left.2, left.3, left.0).cmp(&(right.2, right.3, right.0)));

        let steps = reachable
            .into_iter()
            .enumerate()
            .map(|(reveal_step, (index, relation, _, _))| {
                (
                    index,
                    HighlightStep {
                        relation,
                        reveal_step,
                    },
                )
            })
            .collect();

        Self {
            active: true,
            anchor_index: Some(selected_index),
            elapsed: Duration::ZERO,
            steps,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn anchor_index(&self) -> Option<usize> {
        self.anchor_index
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn tick(&mut self, delta: Duration) {
        if self.active && self.steps.len() > 1 {
            self.elapsed = self.elapsed.saturating_add(delta);
        }
    }

    pub fn reveal_all_for_test(&mut self) {
        let remaining_steps = self.steps.len().saturating_sub(1) as u64;
        self.elapsed = Duration::from_millis(remaining_steps * PROPAGATION_DELAY_MS);
    }

    pub fn highlight_for(&self, event_index: usize) -> Option<CausalRelation> {
        let step = self.steps.get(&event_index)?;
        if step.reveal_step <= self.visible_step() {
            Some(step.relation)
        } else {
            None
        }
    }

    fn visible_step(&self) -> usize {
        if !self.active {
            return 0;
        }

        let revealed = (self.elapsed.as_millis() / u128::from(PROPAGATION_DELAY_MS)) as usize;
        revealed.min(self.steps.len().saturating_sub(1))
    }
}

fn bfs_distances(anchor_event_id: i64, links: &[CausalLink]) -> HashMap<i64, usize> {
    let mut adjacency = HashMap::<i64, Vec<i64>>::new();
    for link in links {
        adjacency
            .entry(link.source_event_id)
            .or_default()
            .push(link.target_event_id);
        adjacency
            .entry(link.target_event_id)
            .or_default()
            .push(link.source_event_id);
    }

    let mut distances = HashMap::from([(anchor_event_id, 0usize)]);
    let mut queue = VecDeque::from([anchor_event_id]);
    let mut visited = HashSet::from([anchor_event_id]);

    while let Some(event_id) = queue.pop_front() {
        let distance = distances.get(&event_id).copied().unwrap_or_default();
        for neighbor in adjacency.get(&event_id).into_iter().flatten() {
            if visited.insert(*neighbor) {
                distances.insert(*neighbor, distance + 1);
                queue.push_back(*neighbor);
            }
        }
    }

    distances
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        evidence::LinkedEvent,
        session_list::{SessionEventKind, SessionListItem},
        transcript::{ReplayTranscript, TranscriptEntry},
    };
    use time::OffsetDateTime;

    #[test]
    fn causal_chain_reveals_five_linked_events_after_activation() {
        let timestamps = [
            parse_timestamp("2026-04-03T01:06:40Z"),
            parse_timestamp("2026-04-03T01:06:50Z"),
            parse_timestamp("2026-04-03T01:07:00Z"),
            parse_timestamp("2026-04-03T01:07:10Z"),
            parse_timestamp("2026-04-03T01:07:20Z"),
        ];
        let events = vec![
            SessionEvent::named(
                SessionEventKind::UserPromptSubmit,
                "UserPromptSubmit",
                timestamps[0],
            )
            .with_raw_event_id(11),
            SessionEvent::named(
                SessionEventKind::InstructionsLoaded,
                "InstructionsLoaded",
                timestamps[1],
            )
            .with_raw_event_id(12),
            SessionEvent::named(SessionEventKind::Tool, "PreToolUse", timestamps[2])
                .with_raw_event_id(13)
                .with_linked_events(vec![
                    LinkedEvent::new("UserPromptSubmit", timestamps[0]).with_event_index(0),
                    LinkedEvent::new("InstructionsLoaded", timestamps[1]).with_event_index(1),
                    LinkedEvent::new("PermissionRequest", timestamps[3]).with_event_index(3),
                    LinkedEvent::new("PostToolUse", timestamps[4]).with_event_index(4),
                ]),
            SessionEvent::named(
                SessionEventKind::PermissionRequest,
                "PermissionRequest",
                timestamps[3],
            )
            .with_raw_event_id(14),
            SessionEvent::named(SessionEventKind::Tool, "PostToolUse", timestamps[4])
                .with_raw_event_id(15),
        ];
        let _session = SessionListItem::new(
            "session-causal",
            "feature/causal",
            timestamps[4],
            0.18,
            events.clone(),
        );
        let _transcript = ReplayTranscript::new(vec![
            TranscriptEntry::assistant(0, "Prompt submitted"),
            TranscriptEntry::assistant(1, "Instructions loaded"),
            TranscriptEntry::assistant(2, "Preparing tool call"),
            TranscriptEntry::assistant(3, "Permission granted"),
            TranscriptEntry::assistant(4, "Tool finished"),
        ]);

        let links = vec![
            CausalLink::new(11, 12),
            CausalLink::new(12, 13),
            CausalLink::new(13, 14),
            CausalLink::new(14, 15),
        ];

        let mut state = CausalChainState::activate(2, &events, &links);
        state.reveal_all_for_test();

        assert_eq!(state.highlight_for(0), Some(CausalRelation::Before));
        assert_eq!(state.highlight_for(1), Some(CausalRelation::Before));
        assert_eq!(state.highlight_for(2), Some(CausalRelation::Selected));
        assert_eq!(state.highlight_for(3), Some(CausalRelation::After));
        assert_eq!(state.highlight_for(4), Some(CausalRelation::After));
    }

    #[test]
    fn causal_chain_uses_50ms_propagation_steps() {
        let events = vec![
            SessionEvent::named(
                SessionEventKind::UserPromptSubmit,
                "UserPromptSubmit",
                parse_timestamp("2026-04-03T01:06:40Z"),
            )
            .with_raw_event_id(1),
            SessionEvent::named(
                SessionEventKind::Tool,
                "PreToolUse",
                parse_timestamp("2026-04-03T01:07:00Z"),
            )
            .with_raw_event_id(2),
            SessionEvent::named(
                SessionEventKind::Tool,
                "PostToolUse",
                parse_timestamp("2026-04-03T01:07:20Z"),
            )
            .with_raw_event_id(3),
        ];
        let links = vec![CausalLink::new(1, 2), CausalLink::new(2, 3)];

        let mut state = CausalChainState::activate(1, &events, &links);

        assert_eq!(state.highlight_for(1), Some(CausalRelation::Selected));
        assert_eq!(state.highlight_for(0), None);
        assert_eq!(state.highlight_for(2), None);

        state.tick(Duration::from_millis(49));
        assert_eq!(state.highlight_for(0), None);
        assert_eq!(state.highlight_for(2), None);

        state.tick(Duration::from_millis(1));
        assert_eq!(state.highlight_for(0), Some(CausalRelation::Before));

        state.tick(Duration::from_millis(50));
        assert_eq!(state.highlight_for(2), Some(CausalRelation::After));
    }

    fn parse_timestamp(input: &str) -> OffsetDateTime {
        match OffsetDateTime::parse(input, &time::format_description::well_known::Rfc3339) {
            Ok(value) => value,
            Err(error) => panic!("failed to parse timestamp {input}: {error}"),
        }
    }
}
