#![deny(clippy::expect_used, clippy::unwrap_used)]

pub mod backlog;
pub mod transcript_tailer;

use std::{
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};

use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use claude_insight_storage::{Database, NewRawEvent};
use claude_insight_types::HookEvent;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub const CRATE_NAME: &str = "claude-insight-capture";
pub const DEFAULT_CAPTURE_PORT: u16 = 4180;
const HEALTH_STATUS: &str = "running";
const HOOK_SOURCE: &str = "hook";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureConfig {
    pub port: u16,
    pub database_path: PathBuf,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        let database_path = match Database::default_path() {
            Ok(path) => path,
            Err(_) => PathBuf::from(".claude-insight/insight.db"),
        };

        Self {
            port: DEFAULT_CAPTURE_PORT,
            database_path,
        }
    }
}

impl CaptureConfig {
    pub fn with_database_path(mut self, database_path: impl Into<PathBuf>) -> Self {
        self.database_path = database_path.into();
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn bind_addr(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, self.port))
    }
}

#[derive(Debug, Clone)]
struct CaptureState {
    config: CaptureConfig,
}

#[derive(Debug, serde::Serialize)]
struct HealthResponse {
    status: &'static str,
    event_count: u64,
}

#[derive(Debug, serde::Serialize)]
struct HookReceipt {
    status: &'static str,
}

#[derive(Debug)]
struct PersistedHookEvent {
    session_id: String,
    event_type: String,
    tool_use_id: Option<String>,
    agent_id: Option<String>,
    payload_json: String,
    ts: String,
}

impl PersistedHookEvent {
    fn from_hook_event(event: HookEvent, payload_json: String, ts: String) -> Self {
        let session_id = event.base().session_id.clone();
        let agent_id = event.base().agent_id.clone();
        let event_type = event.hook_event_name().to_string();
        let tool_use_id = match &event {
            HookEvent::PreToolUse(input) => Some(input.tool_use_id.clone()),
            HookEvent::PostToolUse(input) => Some(input.tool_use_id.clone()),
            HookEvent::PostToolUseFailure(input) => Some(input.tool_use_id.clone()),
            HookEvent::PermissionDenied(input) => Some(input.tool_use_id.clone()),
            _ => None,
        };

        Self {
            session_id,
            event_type,
            tool_use_id,
            agent_id,
            payload_json,
            ts,
        }
    }
}

#[derive(Debug)]
enum CaptureError {
    BadRequest(String),
    Storage(String),
}

impl CaptureError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    fn storage(message: impl Into<String>) -> Self {
        Self::Storage(message.into())
    }
}

impl IntoResponse for CaptureError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into_response(),
            Self::Storage(message) => (StatusCode::INTERNAL_SERVER_ERROR, message).into_response(),
        }
    }
}

pub fn hooks_router() -> Router {
    hooks_router_with_config(CaptureConfig::default())
}

pub fn hooks_router_with_config(config: CaptureConfig) -> Router {
    Router::new()
        .route("/hooks", post(receive_hook_event))
        .route("/health", get(health))
        .with_state(Arc::new(CaptureState { config }))
}

pub fn backlog_settings() -> CaptureConfig {
    CaptureConfig::default()
}

pub async fn yield_once() {
    tokio::task::yield_now().await;
}

pub fn sample_event() -> claude_insight_types::PlaceholderEvent {
    claude_insight_types::placeholder_event()
}

async fn receive_hook_event(
    State(state): State<Arc<CaptureState>>,
    body: Bytes,
) -> Result<impl IntoResponse, CaptureError> {
    let payload_json = std::str::from_utf8(&body)
        .map(str::to_owned)
        .map_err(|error| {
            CaptureError::bad_request(format!("request body must be valid UTF-8 JSON: {error}"))
        })?;
    let event = serde_json::from_slice::<HookEvent>(&body)
        .map_err(|error| CaptureError::bad_request(format!("invalid hook JSON: {error}")))?;
    let persisted_event =
        PersistedHookEvent::from_hook_event(event, payload_json, timestamp_now()?);
    let database_path = state.config.database_path.clone();

    tokio::task::spawn_blocking(move || -> Result<(), CaptureError> {
        let database = Database::new(&database_path)
            .map_err(|error| CaptureError::storage(format!("failed to open database: {error}")))?;
        let new_raw_event = NewRawEvent {
            session_id: Some(persisted_event.session_id.as_str()),
            source: HOOK_SOURCE,
            event_type: persisted_event.event_type.as_str(),
            ts: persisted_event.ts.as_str(),
            tool_use_id: persisted_event.tool_use_id.as_deref(),
            prompt_id: None,
            agent_id: persisted_event.agent_id.as_deref(),
            payload_json: persisted_event.payload_json.as_str(),
            claude_version: None,
            adapter_version: None,
        };
        database
            .insert_raw_event_record(&new_raw_event)
            .map_err(|error| {
                CaptureError::storage(format!("failed to persist raw event: {error}"))
            })?;

        Ok(())
    })
    .await
    .map_err(|error| CaptureError::storage(format!("hook persistence task failed: {error}")))??;

    Ok((StatusCode::OK, Json(HookReceipt { status: "ok" })))
}

async fn health(
    State(state): State<Arc<CaptureState>>,
) -> Result<Json<HealthResponse>, CaptureError> {
    let database_path = state.config.database_path.clone();
    let event_count = tokio::task::spawn_blocking(move || -> Result<u64, CaptureError> {
        let database = Database::new(&database_path)
            .map_err(|error| CaptureError::storage(format!("failed to open database: {error}")))?;
        database
            .count_raw_events()
            .map_err(|error| CaptureError::storage(format!("failed to count raw events: {error}")))
    })
    .await
    .map_err(|error| CaptureError::storage(format!("health query task failed: {error}")))??;

    Ok(Json(HealthResponse {
        status: HEALTH_STATUS,
        event_count,
    }))
}

fn timestamp_now() -> Result<String, CaptureError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| CaptureError::storage(format!("failed to format timestamp: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Method, Request},
    };
    use claude_insight_storage::RawEventQuery;
    use tower::util::ServiceExt;

    fn temp_database_path(test_name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();

        std::env::temp_dir().join(format!(
            "claude-insight-{test_name}-{}-{nanos}.db",
            std::process::id()
        ))
    }

    fn router_for_test(test_name: &str) -> (Router, PathBuf) {
        let database_path = temp_database_path(test_name);
        let config = CaptureConfig::default().with_database_path(&database_path);

        (hooks_router_with_config(config), database_path)
    }

    #[test]
    fn backlog_settings_use_default_capture_port() {
        let config = backlog_settings();

        assert_eq!(config.port, DEFAULT_CAPTURE_PORT);
    }

    #[tokio::test]
    async fn hook_receiver_session_start_returns_ok() {
        let (app, _) = router_for_test("session-start");
        let payload = include_str!("../../../tests/fixtures/hooks/SessionStart.json");
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/hooks")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .unwrap_or_else(|error| panic!("failed to build request: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("request failed: {error}"));

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn hook_receiver_pre_tool_use_persists_raw_event() {
        let (app, database_path) = router_for_test("pre-tool-use");
        let payload = include_str!("../../../tests/fixtures/hooks/PreToolUse.json");
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/hooks")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .unwrap_or_else(|error| panic!("failed to build request: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("request failed: {error}"));

        assert_eq!(response.status(), StatusCode::OK);

        let database = Database::new(&database_path)
            .unwrap_or_else(|error| panic!("failed to open sqlite database: {error}"));
        let events = database
            .query_raw_events(RawEventQuery {
                event_type: Some("PreToolUse"),
                ..RawEventQuery::default()
            })
            .unwrap_or_else(|error| panic!("failed to query raw events: {error}"));

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].source, HOOK_SOURCE);
        assert_eq!(events[0].event_type, "PreToolUse");
        assert_eq!(
            events[0].tool_use_id.as_deref(),
            Some("toolu_01Bqr78WkjBpvgdnN3GGhDB1")
        );
    }

    #[tokio::test]
    async fn hook_receiver_unknown_event_type_is_persisted() {
        let (app, database_path) = router_for_test("unknown-hook-event");
        let mut payload = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../tests/fixtures/hooks/Notification.json"
        ))
        .unwrap_or_else(|error| panic!("failed to parse fixture json: {error}"));
        let object = payload
            .as_object_mut()
            .unwrap_or_else(|| panic!("notification fixture should be a json object"));
        object.insert(
            "hook_event_name".to_owned(),
            serde_json::json!("FutureHookEvent"),
        );
        object.insert("future_field".to_owned(), serde_json::json!(42));
        let payload = serde_json::to_vec(&payload)
            .unwrap_or_else(|error| panic!("failed to serialize request json: {error}"));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/hooks")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .unwrap_or_else(|error| panic!("failed to build request: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("request failed: {error}"));

        assert_eq!(response.status(), StatusCode::OK);

        let database = Database::new(&database_path)
            .unwrap_or_else(|error| panic!("failed to open sqlite database: {error}"));
        let events = database
            .query_raw_events(RawEventQuery {
                event_type: Some("Unknown"),
                ..RawEventQuery::default()
            })
            .unwrap_or_else(|error| panic!("failed to query raw events: {error}"));

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].source, HOOK_SOURCE);
        assert_eq!(events[0].event_type, "Unknown");
        assert!(events[0].payload_json.contains("\"FutureHookEvent\""));
    }

    #[tokio::test]
    async fn hook_receiver_bad_json_returns_bad_request() {
        let (app, _) = router_for_test("bad-json");
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/hooks")
                    .header("content-type", "application/json")
                    .body(Body::from("{not valid json"))
                    .unwrap_or_else(|error| panic!("failed to build request: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("request failed: {error}"));

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn hook_receiver_health_reports_status_and_event_count() {
        let (app, _) = router_for_test("health");
        let payload = include_str!("../../../tests/fixtures/hooks/SessionStart.json");
        let post_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/hooks")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .unwrap_or_else(|error| panic!("failed to build request: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("request failed: {error}"));

        assert_eq!(post_response.status(), StatusCode::OK);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("failed to build request: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("request failed: {error}"));
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap_or_else(|error| panic!("failed to read response body: {error}"));
        let payload = serde_json::from_slice::<serde_json::Value>(&body)
            .unwrap_or_else(|error| panic!("failed to parse response json: {error}"));

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["status"], HEALTH_STATUS);
        assert_eq!(payload["event_count"], 1);
    }

    #[tokio::test]
    async fn hook_receiver_storage_failure_returns_internal_server_error(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let database_path = std::env::temp_dir().join(format!(
            "claude-insight-hook-receiver-failure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir_all(&database_path)?;
        let config = CaptureConfig::default().with_database_path(&database_path);
        let app = hooks_router_with_config(config);
        let payload = include_str!("../../../tests/fixtures/hooks/SessionStart.json");

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/hooks")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .unwrap_or_else(|error| panic!("failed to build request: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("request failed: {error}"));
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap_or_else(|error| panic!("failed to read response body: {error}"));
        let body = String::from_utf8(body.to_vec())?;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.contains("failed to open database"));
        std::fs::remove_dir_all(&database_path)?;

        Ok(())
    }
}
