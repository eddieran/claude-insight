#![deny(clippy::expect_used, clippy::unwrap_used)]

pub const CRATE_NAME: &str = "claude-insight-storage";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StorageStub {
    pub database_url: String,
}

impl StorageStub {
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
        }
    }

    pub fn open_in_memory() -> rusqlite::Result<rusqlite::Connection> {
        tracing::trace!("opening in-memory placeholder database");
        rusqlite::Connection::open_in_memory()
    }

    pub fn sample_event() -> claude_insight_types::PlaceholderEvent {
        claude_insight_types::placeholder_event()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_stub_keeps_database_url() {
        let stub = StorageStub::new("sqlite::memory:");

        assert_eq!(stub.database_url, "sqlite::memory:");
    }
}
