//! `DllMain` and worker-thread bootstrap.
//!
//! On `DLL_PROCESS_ATTACH` we MUST NOT do real work (loader lock). Instead we
//! spawn a worker thread that opens the shared MMF and installs hooks.

use std::ffi::c_void;
use std::thread;

use time_mocker_core::{mmf_name_for_pid, SharedDelta};
use windows_sys::Win32::Foundation::{BOOL, HMODULE, TRUE};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::hooks;

#[no_mangle]
#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe extern "system" fn DllMain(
    _hinst: HMODULE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        thread::spawn(bootstrap);
    }
    TRUE
}

fn bootstrap() {
    let pid = unsafe { GetCurrentProcessId() };
    let name = mmf_name_for_pid(pid);

    let shared = match SharedDelta::open(&name) {
        Ok(s) => s,
        Err(_) => return,
    };

    if hooks::install(shared).is_err() {
        // Hook failure is silent — the target process runs unmodified.
        return;
    }

    // Keep the thread alive; hooks live for the lifetime of the process.
    loop {
        thread::park();
    }
}
