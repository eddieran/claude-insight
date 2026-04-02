use ratatui::{style::Style, symbols, widgets::Sparkline};

use crate::session_list::SessionEvent;
use crate::session_list::ACCENT_GREEN;

pub fn compute_sparkline_data(events: &[SessionEvent], bucket_size_secs: i64) -> Vec<u64> {
    if events.is_empty() {
        return vec![0];
    }

    let bucket_size_secs = bucket_size_secs.max(1);
    let start = events
        .iter()
        .map(|event| event.timestamp)
        .min()
        .unwrap_or(events[0].timestamp);
    let end = events
        .iter()
        .map(|event| event.timestamp)
        .max()
        .unwrap_or(start);
    let bucket_count = (((end - start).whole_seconds() / bucket_size_secs).max(0) as usize) + 1;
    let mut buckets = vec![0_u64; bucket_count.max(1)];

    for event in events {
        let bucket = ((event.timestamp - start).whole_seconds() / bucket_size_secs).max(0) as usize;
        if let Some(slot) = buckets.get_mut(bucket) {
            *slot += 1;
        }
    }

    buckets
}

pub fn activity_sparkline<'a>(data: &'a [u64]) -> Sparkline<'a> {
    Sparkline::default()
        .data(data.to_vec())
        .bar_set(symbols::bar::NINE_LEVELS)
        .style(Style::new().fg(ACCENT_GREEN))
        .max(data.iter().copied().max().unwrap_or(1))
}

#[cfg(test)]
mod animations_widgets_tests {
    use time::OffsetDateTime;

    use super::*;
    use crate::session_list::{SessionEvent, SessionEventKind};

    #[test]
    fn sparkline_data_uses_five_second_buckets_across_sixty_seconds() {
        let start = parse_timestamp("2026-04-03T01:00:00Z");
        let events = (0..20)
            .map(|index| {
                let second = (index * 3) as i64;
                SessionEvent::new(
                    SessionEventKind::Other,
                    start + time::Duration::seconds(second),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            compute_sparkline_data(&events, 5),
            vec![2, 2, 1, 2, 2, 1, 2, 2, 1, 2, 2, 1]
        );
    }

    fn parse_timestamp(input: &str) -> OffsetDateTime {
        match OffsetDateTime::parse(input, &time::format_description::well_known::Rfc3339) {
            Ok(value) => value,
            Err(error) => panic!("failed to parse timestamp {input}: {error}"),
        }
    }
}
