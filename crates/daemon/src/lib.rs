#![deny(clippy::expect_used, clippy::unwrap_used)]

pub const CRATE_NAME: &str = "claude-insight-daemon";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonStub {
    pub capture_addr: String,
    pub api_addr: String,
}

impl DaemonStub {
    pub fn new(capture_addr: impl Into<String>, api_addr: impl Into<String>) -> Self {
        Self {
            capture_addr: capture_addr.into(),
            api_addr: api_addr.into(),
        }
    }

    pub async fn router(&self) -> axum::Router {
        tokio::task::yield_now().await;
        tracing::trace!("building placeholder daemon router");
        claude_insight_capture::hooks_router()
    }

    pub fn storage(&self) -> claude_insight_storage::StorageStub {
        let _ = self;
        claude_insight_storage::StorageStub::new("sqlite::memory:")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_stub_preserves_capture_address() {
        let stub = DaemonStub::new("127.0.0.1:4180", "127.0.0.1:4181");

        assert_eq!(stub.capture_addr, "127.0.0.1:4180");
    }
}
