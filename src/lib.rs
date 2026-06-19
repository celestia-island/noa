pub mod cli;
pub mod coauthor;
pub mod config;
pub mod error;
pub mod git;
pub mod ignore;
pub mod log;
pub mod merge;
pub mod object;
pub mod precheck;
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
/// Returns 0 if the system clock is before the Unix epoch.
#[must_use]
pub fn now_micros() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_micros()).unwrap_or(0)
}
