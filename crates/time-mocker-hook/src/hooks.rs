//! Inline detours for 5 Win32 time APIs.
//!
//! All hooks resolve the current FILETIME via the real API, add the shared
//! delta, and return the adjusted value. Reads are atomic and lock-free.

use std::ffi::CStr;

use once_cell::sync::OnceCell;
use retour::static_detour;
use time_mocker_core::ticks::{filetime_to_i64, i64_to_filetime, ticks_to_systemtime};
use time_mocker_core::SharedDelta;
use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows_sys::Win32::System::Time::FileTimeToSystemTime;

static SHARED: OnceCell<SharedDelta> = OnceCell::new();

static_detour! {
    static GetSystemTimeDetour: unsafe extern "system" fn(*mut SYSTEMTIME);
    static GetLocalTimeDetour: unsafe extern "system" fn(*mut SYSTEMTIME);
    static GetSystemTimeAsFileTimeDetour: unsafe extern "system" fn(*mut FILETIME);
    static GetSystemTimePreciseAsFileTimeDetour: unsafe extern "system" fn(*mut FILETIME);
    static NtQuerySystemTimeDetour: unsafe extern "system" fn(*mut i64) -> i32;
}

type FnSystemTime = unsafe extern "system" fn(*mut SYSTEMTIME);
type FnFileTime = unsafe extern "system" fn(*mut FILETIME);
type FnNtQuerySystemTime = unsafe extern "system" fn(*mut i64) -> i32;

pub fn install(shared: SharedDelta) -> Result<(), retour::Error> {
    let _ = SHARED.set(shared);

    unsafe {
        if let Some(target) = resolve::<FnSystemTime>("kernel32.dll", "GetSystemTime") {
            GetSystemTimeDetour
                .initialize(target, hook_get_system_time)?
                .enable()?;
        }
        if let Some(target) = resolve::<FnSystemTime>("kernel32.dll", "GetLocalTime") {
            GetLocalTimeDetour
                .initialize(target, hook_get_local_time)?
                .enable()?;
        }
        if let Some(target) = resolve::<FnFileTime>("kernel32.dll", "GetSystemTimeAsFileTime") {
            GetSystemTimeAsFileTimeDetour
                .initialize(target, hook_get_system_time_as_filetime)?
                .enable()?;
        }
        if let Some(target) =
            resolve::<FnFileTime>("kernel32.dll", "GetSystemTimePreciseAsFileTime")
        {
            GetSystemTimePreciseAsFileTimeDetour
                .initialize(target, hook_get_system_time_precise_as_filetime)?
                .enable()?;
        }
        if let Some(target) = resolve::<FnNtQuerySystemTime>("ntdll.dll", "NtQuerySystemTime") {
            NtQuerySystemTimeDetour
                .initialize(target, hook_nt_query_system_time)?
                .enable()?;
        }
    }
    Ok(())
}

unsafe fn resolve<F: Copy>(module: &str, proc: &str) -> Option<F> {
    let module_c = std::ffi::CString::new(module).ok()?;
    let proc_c = std::ffi::CString::new(proc).ok()?;
    let h = GetModuleHandleA(module_c.as_ptr() as *const u8);
    if h.is_null() {
        return None;
    }
    let _ = CStr::from_bytes_with_nul(b"\0");
    let addr = GetProcAddress(h, proc_c.as_ptr() as *const u8)?;
    Some(std::mem::transmute_copy::<_, F>(&addr))
}

#[inline]
fn delta() -> i64 {
    SHARED.get().map(|s| s.read_delta()).unwrap_or(0)
}

fn fake_filetime() -> FILETIME {
    let mut ft: FILETIME = unsafe { std::mem::zeroed() };
    unsafe { GetSystemTimeAsFileTimeDetour.call(&mut ft) };
    let ticks = filetime_to_i64(ft).saturating_add(delta());
    i64_to_filetime(ticks)
}

fn fake_filetime_precise() -> FILETIME {
    let mut ft: FILETIME = unsafe { std::mem::zeroed() };
    unsafe { GetSystemTimePreciseAsFileTimeDetour.call(&mut ft) };
    let ticks = filetime_to_i64(ft).saturating_add(delta());
    i64_to_filetime(ticks)
}

fn hook_get_system_time(out: *mut SYSTEMTIME) {
    if out.is_null() {
        return;
    }
    let ft = fake_filetime();
    unsafe { FileTimeToSystemTime(&ft, out) };
}

fn hook_get_local_time(out: *mut SYSTEMTIME) {
    if out.is_null() {
        return;
    }
    let ft = fake_filetime();
    let ticks = filetime_to_i64(ft);
    if let Some(st) = ticks_to_systemtime(ticks) {
        unsafe { *out = st };
    }
}

fn hook_get_system_time_as_filetime(out: *mut FILETIME) {
    if out.is_null() {
        return;
    }
    unsafe { *out = fake_filetime() };
}

fn hook_get_system_time_precise_as_filetime(out: *mut FILETIME) {
    if out.is_null() {
        return;
    }
    unsafe { *out = fake_filetime_precise() };
}

fn hook_nt_query_system_time(out: *mut i64) -> i32 {
    if out.is_null() {
        return -1;
    }
    let mut real: i64 = 0;
    unsafe { NtQuerySystemTimeDetour.call(&mut real) };
    unsafe { *out = real.saturating_add(delta()) };
    0
}
