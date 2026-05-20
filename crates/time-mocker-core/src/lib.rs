//! Shared types and named MMF helper for time-mocker.
//!
//! IPC contract: an 8-byte memory-mapped file named `TimeMocker_<pid>` holds an
//! i64 `DeltaTicks` — the offset (in 100-ns FILETIME units) added to the real
//! system FILETIME by the injected hook.

#![cfg(windows)]

pub mod mmf;
pub mod ticks;
pub mod types;

pub use mmf::SharedDelta;
pub use types::MockTimeInfo;

pub const MMF_PREFIX: &str = "TimeMocker_";

#[inline]
pub fn mmf_name_for_pid(pid: u32) -> String {
    format!("{MMF_PREFIX}{pid}")
}
