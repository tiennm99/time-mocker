//! Shared types and named MMF helper for time-mocker.
//!
//! IPC contract: an 8-byte memory-mapped file named `Global\TimeMocker_<pid>`
//! (or `Local\TimeMocker_<pid>` as a session-scoped fallback) holds an i64
//! `DeltaTicks` — the offset (in 100-ns FILETIME units) added to the real
//! system FILETIME by the injected hook.
//!
//! The `Global\` namespace lets the admin controller in session 1 reach
//! processes in session 0 (services) and other sessions, but creating an
//! object there requires `SeCreateGlobalPrivilege` — granted only to elevated
//! tokens. Debug / non-elevated controllers fall back to `Local\` so dev
//! workflows can still hook same-session targets.
//!
//! Both endpoints (writer = UI, reader = hook DLL) try `Global\` first and
//! then `Local\`, so a controller's elevation state determines the namespace
//! used and the hook just probes both.

#![cfg(windows)]

pub mod mmf;
pub mod ticks;
pub mod types;

pub use mmf::{CreateOutcome, SharedDeltaReader, SharedDeltaWriter};
pub use types::MockTimeInfo;

pub const MMF_PREFIX_GLOBAL: &str = "Global\\TimeMocker_";
pub const MMF_PREFIX_LOCAL: &str = "Local\\TimeMocker_";

/// Back-compat alias: the primary (elevated) namespace.
pub const MMF_PREFIX: &str = MMF_PREFIX_GLOBAL;

#[inline]
pub fn mmf_name_for_pid(pid: u32) -> String {
    format!("{MMF_PREFIX_GLOBAL}{pid}")
}

#[inline]
pub fn local_mmf_name_for_pid(pid: u32) -> String {
    format!("{MMF_PREFIX_LOCAL}{pid}")
}
