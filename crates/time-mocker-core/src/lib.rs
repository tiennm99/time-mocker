//! Shared types and named MMF helper for time-mocker.
//!
//! IPC contract: an 8-byte memory-mapped file named `Global\TimeMocker_<pid>`
//! holds an i64 `DeltaTicks` — the offset (in 100-ns FILETIME units) added to
//! the real system FILETIME by the injected hook.
//!
//! The `Global\` namespace lets the admin controller in session 1 reach
//! processes in session 0 (services) and other sessions. Single-user same-
//! session injection works too — `Global\` is a superset of `Local\`.

#![cfg(windows)]

pub mod mmf;
pub mod ticks;
pub mod types;

pub use mmf::{CreateOutcome, SharedDeltaReader, SharedDeltaWriter};
pub use types::MockTimeInfo;

pub const MMF_PREFIX: &str = "Global\\TimeMocker_";

#[inline]
pub fn mmf_name_for_pid(pid: u32) -> String {
    format!("{MMF_PREFIX}{pid}")
}
