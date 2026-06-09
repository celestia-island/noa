pub mod cli;
pub mod config;
pub mod error;
pub mod git;
pub mod ignore;
pub mod log;
pub mod merge;
pub mod object;
pub mod refs;
pub mod remote;
pub mod repo;
pub mod server;
pub mod snapshot;
pub mod sync;
pub mod tui;
pub mod workspace;

/// Returns the current timestamp in microseconds since Unix epoch.
/// This is the standard timestamp unit used across the entire Noa project.
pub fn now_micros() -> u64 {
    chrono::Utc::now().timestamp_micros().max(0).unsigned_abs()
}
