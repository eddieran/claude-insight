#![deny(clippy::expect_used, clippy::unwrap_used)]

use axum::{http::StatusCode, routing::post, Router};

pub const CRATE_NAME: &str = "claude-insight-capture";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CaptureStub {
    pub watch_mode: String,
    pub recursive_mode: String,
}

pub fn hooks_router() -> Router {
    Router::new().route("/events", post(|| async move { StatusCode::ACCEPTED }))
}

pub fn backlog_settings() -> CaptureStub {
    CaptureStub {
        watch_mode: "jsonl".to_owned(),
        recursive_mode: format!("{:?}", notify::RecursiveMode::NonRecursive),
    }
}

pub async fn yield_once() {
    tokio::task::yield_now().await;
}

pub fn sample_event() -> claude_insight_types::PlaceholderEvent {
    claude_insight_types::placeholder_event()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backlog_settings_use_jsonl_mode() {
        let stub = backlog_settings();

        assert_eq!(stub.watch_mode, "jsonl");
    }
}
