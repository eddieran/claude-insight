#![deny(clippy::expect_used, clippy::unwrap_used)]

pub const CRATE_NAME: &str = "claude-insight-types";

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlaceholderEvent {
    pub source: String,
    pub payload: serde_json::Value,
}

pub fn placeholder_event() -> PlaceholderEvent {
    let _span = tracing::trace_span!("types_placeholder_event");

    PlaceholderEvent {
        source: "hook".to_owned(),
        payload: serde_json::json!({ "status": "placeholder" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_event_has_expected_source() {
        let event = placeholder_event();

        assert_eq!(event.source, "hook");
    }
}
