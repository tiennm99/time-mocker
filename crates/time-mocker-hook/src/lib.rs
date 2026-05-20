//! Injected DLL — hooks Win32 time APIs and rewrites them to read a shared delta.
//!
//! Implementation lives in `hooks.rs` and `entrypoint.rs`.

#![cfg(windows)]

mod entrypoint;
mod hooks;
