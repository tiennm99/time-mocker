//! `DllMain` and worker-thread bootstrap.
//!
//! On `DLL_PROCESS_ATTACH` we MUST NOT do real work under the loader lock.
//! `thread::spawn` lazily wires up Rust's runtime hooks before the loader is
//! unlocked, which can deadlock against any third-party DLL that locks in its
//! `DLL_THREAD_ATTACH`. Instead we use raw `CreateThread` — its thread starts
//! after `DllMain` returns and the new thread's loader-lock interactions
//! (thread-attach callbacks) run cleanly.
//!
//! We also call `DisableThreadLibraryCalls(hinst)` to suppress all future
//! `DLL_THREAD_ATTACH`/`DETACH` notifications for this DLL — we don't need them.

use std::ffi::{c_void, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use time_mocker_core::{mmf_name_for_pid, SharedDeltaReader};
use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HMODULE, TRUE};
use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::{CreateThread, GetCurrentProcessId};

use crate::hooks::{self, InstallReport};

#[no_mangle]
#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe extern "system" fn DllMain(
    hinst: HMODULE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        DisableThreadLibraryCalls(hinst);
        let thread_handle = CreateThread(
            ptr::null(),
            0,
            Some(bootstrap_thread),
            ptr::null(),
            0,
            ptr::null_mut(),
        );
        // Close our reference immediately — the thread keeps running until it
        // exits naturally, and the kernel reclaims the object when its last
        // handle is closed. Without this, one kernel handle leaks per injection.
        if !thread_handle.is_null() {
            CloseHandle(thread_handle);
        }
    }
    TRUE
}

unsafe extern "system" fn bootstrap_thread(_param: *mut c_void) -> u32 {
    bootstrap();
    0
}

fn bootstrap() {
    let pid = unsafe { GetCurrentProcessId() };
    let name = mmf_name_for_pid(pid);

    let shared = match SharedDeltaReader::open(&name) {
        Ok(s) => s,
        Err(e) => {
            // Controller didn't set up the MMF (cross-session DACL, stale inject,
            // or never-injected debug case). Surface via OutputDebugStringW for DbgView.
            dbg_log(&format!("time-mocker: open MMF '{name}' failed: {e}"));
            return;
        }
    };

    // Best-effort install. Detours stay armed for the lifetime of the host
    // process; the bootstrap thread exits immediately after.
    let report = hooks::install(shared);
    log_install_report(&report);
}

fn log_install_report(report: &InstallReport) {
    if report.failed.is_empty() {
        dbg_log(&format!(
            "time-mocker: installed {} hooks: {:?}",
            report.installed.len(),
            report.installed
        ));
        return;
    }
    dbg_log(&format!(
        "time-mocker: installed={:?} failed={:?}",
        report.installed, report.failed
    ));
}

fn dbg_log(msg: &str) {
    let wide: Vec<u16> = OsStr::new(msg)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe { OutputDebugStringW(wide.as_ptr()) };
}
