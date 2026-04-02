#![deny(clippy::expect_used, clippy::unwrap_used)]

mod manager;

pub use manager::{
    DaemonConfig, DaemonError, DaemonManager, DaemonShutdownReport, DaemonStartReport,
};

pub const CRATE_NAME: &str = "claude-insight-daemon";
