//! Dedicated injection target for time-mocker.
//!
//! Prints its own PID at startup, then loops printing the five hooked time
//! APIs side-by-side every second. Lets you verify the hook DLL is working
//! against an isolated process you control, instead of injecting into
//! arbitrary running programs.
//!
//! Usage:
//!   1. Build the workspace: `cargo build --workspace`
//!   2. Run this binary in one terminal — it prints its PID on line 1.
//!   3. In the TimeMocker UI, type that PID into Inject by PID and set a fake time.
//!   4. Watch the times printed by this binary shift by the delta you set.

#![cfg(windows)]

use std::ffi::CString;
use std::mem::MaybeUninit;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

type FnGetSystemTimeAsFileTime = unsafe extern "system" fn(*mut FILETIME);
type FnNtQuerySystemTime = unsafe extern "system" fn(*mut i64) -> i32;

fn main() {
    let pid = unsafe { GetCurrentProcessId() };

    println!("================================================================");
    println!(" time-mocker test target");
    println!(" PID = {pid}");
    println!(" copy this PID into the TimeMocker UI to inject the hook here.");
    println!(" press Ctrl+C to stop.");
    println!("================================================================");
    println!();

    // Resolve the two APIs that windows-sys' default features don't always
    // surface (GetSystemTimeAsFileTime + NtQuerySystemTime). Matching the
    // hook DLL's resolution strategy (`hooks.rs::resolve`) keeps the call
    // sites symmetric — what the hook hooks, this binary calls.
    let get_system_time_as_file_time = unsafe {
        resolve::<FnGetSystemTimeAsFileTime>("kernel32.dll", "GetSystemTimeAsFileTime")
    };
    let get_system_time_precise_as_file_time = unsafe {
        resolve::<FnGetSystemTimeAsFileTime>("kernel32.dll", "GetSystemTimePreciseAsFileTime")
    };
    let nt_query_system_time =
        unsafe { resolve::<FnNtQuerySystemTime>("ntdll.dll", "NtQuerySystemTime") };

    if get_system_time_as_file_time.is_none() {
        eprintln!("warn: GetSystemTimeAsFileTime not found");
    }
    if get_system_time_precise_as_file_time.is_none() {
        eprintln!("warn: GetSystemTimePreciseAsFileTime not found (pre-Win8?)");
    }
    if nt_query_system_time.is_none() {
        eprintln!("warn: NtQuerySystemTime not found");
    }

    let mut tick: u64 = 0;
    loop {
        tick += 1;
        println!("--- sample #{tick} ---");

        // GetSystemTime — UTC SYSTEMTIME (kernel32).
        let mut st_utc: SYSTEMTIME = unsafe { MaybeUninit::zeroed().assume_init() };
        unsafe { GetSystemTime(&mut st_utc) };
        println!("  GetSystemTime           UTC   {}", fmt_systemtime(&st_utc));

        // GetLocalTime — local SYSTEMTIME (kernel32).
        let mut st_local: SYSTEMTIME = unsafe { MaybeUninit::zeroed().assume_init() };
        unsafe { GetLocalTime(&mut st_local) };
        println!("  GetLocalTime            local {}", fmt_systemtime(&st_local));

        // GetSystemTimeAsFileTime — FILETIME 100-ns ticks since 1601-01-01 UTC.
        if let Some(f) = get_system_time_as_file_time {
            let mut ft: FILETIME = unsafe { MaybeUninit::zeroed().assume_init() };
            unsafe { f(&mut ft) };
            println!(
                "  GetSystemTimeAsFileTime       {}",
                fmt_filetime_as_systemtime(&ft)
            );
        }

        // GetSystemTimePreciseAsFileTime — same units, sub-µs precision.
        if let Some(f) = get_system_time_precise_as_file_time {
            let mut ft: FILETIME = unsafe { MaybeUninit::zeroed().assume_init() };
            unsafe { f(&mut ft) };
            println!(
                "  GetSystemTimePreciseAsFT      {}",
                fmt_filetime_as_systemtime(&ft)
            );
        }

        // NtQuerySystemTime — raw i64 (also 100-ns ticks since 1601-01-01 UTC).
        if let Some(f) = nt_query_system_time {
            let mut ticks: i64 = 0;
            let status = unsafe { f(&mut ticks) };
            if status == 0 {
                let ft = FILETIME {
                    dwLowDateTime: (ticks as u64 & 0xFFFF_FFFF) as u32,
                    dwHighDateTime: ((ticks as u64) >> 32) as u32,
                };
                println!(
                    "  NtQuerySystemTime             {}",
                    fmt_filetime_as_systemtime(&ft)
                );
            } else {
                println!("  NtQuerySystemTime             NTSTATUS={status:#x}");
            }
        }

        println!();
        thread::sleep(Duration::from_secs(1));
    }
}

unsafe fn resolve<F: Copy>(module: &str, proc_name: &str) -> Option<F> {
    let module_c = CString::new(module).ok()?;
    let proc_c = CString::new(proc_name).ok()?;
    let h = GetModuleHandleA(module_c.as_ptr() as *const u8);
    if h.is_null() {
        return None;
    }
    let addr = GetProcAddress(h, proc_c.as_ptr() as *const u8)?;
    Some(std::mem::transmute_copy::<_, F>(&addr))
}

fn fmt_systemtime(st: &SYSTEMTIME) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
    )
}

fn fmt_filetime_as_systemtime(ft: &FILETIME) -> String {
    // Render the FILETIME via FileTimeToSystemTime so the columns line up
    // with the SYSTEMTIME-returning APIs above.
    use windows_sys::Win32::System::Time::FileTimeToSystemTime;
    let mut st: SYSTEMTIME = unsafe { MaybeUninit::zeroed().assume_init() };
    let ok = unsafe { FileTimeToSystemTime(ft, &mut st) };
    if ok == 0 {
        let raw: u64 = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
        format!("(raw {raw}; FileTimeToSystemTime failed)")
    } else {
        format!("UTC   {}", fmt_systemtime(&st))
    }
}
