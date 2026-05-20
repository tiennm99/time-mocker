//! Inline detours for 5 Win32 time APIs.
//!
//! Each hook resolves the current FILETIME via the real API (`.call()` on the
//! detour invokes the trampoline, not the detour itself, so no recursion),
//! adds the shared delta, and returns the adjusted value.
//!
//! Install is best-effort: failures are collected per-hook rather than aborting
//! mid-chain, so a single missing export (e.g., `GetSystemTimePreciseAsFileTime`
//! on pre-Win8) does not leave hooks #1-#3 armed while #4-#5 stay real.

use once_cell::sync::OnceCell;
use retour::static_detour;
use time_mocker_core::ticks::{filetime_to_i64, i64_to_filetime};
use time_mocker_core::SharedDeltaReader;
use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

static SHARED: OnceCell<SharedDeltaReader> = OnceCell::new();

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

/// Per-API outcome from `install`. Surfaces partial-install state for diagnostics.
#[derive(Debug, Default)]
pub struct InstallReport {
    pub installed: Vec<&'static str>,
    pub failed: Vec<(&'static str, String)>,
}

/// Resolve+initialize+enable a single detour. Pushes to `installed` on success,
/// to `failed` (with reason) on any failure — never aborts the wider install.
macro_rules! install_hook {
    ($report:ident, $detour:ident, $module:literal, $proc:literal, $fn_ty:ty, $callback:ident) => {{
        // Safety: `resolve` is unsafe because it dereferences whatever
        // GetProcAddress returns as `$fn_ty`; correctness rests on the
        // module+proc literal pair matching `$fn_ty`'s extern signature.
        match unsafe { resolve::<$fn_ty>($module, $proc) } {
            None => $report
                .failed
                .push(($proc, "GetProcAddress: not found".into())),
            Some(target) => match unsafe { $detour.initialize(target, $callback) } {
                Err(e) => $report.failed.push(($proc, format!("initialize: {e}"))),
                Ok(d) => match unsafe { d.enable() } {
                    Err(e) => $report.failed.push(($proc, format!("enable: {e}"))),
                    Ok(()) => $report.installed.push($proc),
                },
            },
        }
    }};
}

pub fn install(shared: SharedDeltaReader) -> InstallReport {
    let _ = SHARED.set(shared);

    let mut report = InstallReport::default();
    install_hook!(
        report, GetSystemTimeDetour, "kernel32.dll", "GetSystemTime",
        FnSystemTime, hook_get_system_time
    );
    install_hook!(
        report, GetLocalTimeDetour, "kernel32.dll", "GetLocalTime",
        FnSystemTime, hook_get_local_time
    );
    install_hook!(
        report, GetSystemTimeAsFileTimeDetour, "kernel32.dll", "GetSystemTimeAsFileTime",
        FnFileTime, hook_get_system_time_as_filetime
    );
    install_hook!(
        report, GetSystemTimePreciseAsFileTimeDetour, "kernel32.dll", "GetSystemTimePreciseAsFileTime",
        FnFileTime, hook_get_system_time_precise_as_filetime
    );
    install_hook!(
        report, NtQuerySystemTimeDetour, "ntdll.dll", "NtQuerySystemTime",
        FnNtQuerySystemTime, hook_nt_query_system_time
    );
    report
}

unsafe fn resolve<F: Copy>(module: &str, proc: &str) -> Option<F> {
    let module_c = std::ffi::CString::new(module).ok()?;
    let proc_c = std::ffi::CString::new(proc).ok()?;
    let h = GetModuleHandleA(module_c.as_ptr() as *const u8);
    if h.is_null() {
        return None;
    }
    let addr = GetProcAddress(h, proc_c.as_ptr() as *const u8)?;
    Some(std::mem::transmute_copy::<_, F>(&addr))
}

#[inline]
fn delta() -> i64 {
    SHARED.get().map(|s| s.read_delta()).unwrap_or(0)
}

/// Invoke the supplied trampoline to get the real FILETIME, then add the
/// shared delta. Saturating arithmetic — no panic-abort even at i64 bounds.
#[inline]
fn fake_filetime(call_real: impl FnOnce(*mut FILETIME)) -> FILETIME {
    let mut ft: FILETIME = unsafe { std::mem::zeroed() };
    call_real(&mut ft);
    let ticks = filetime_to_i64(ft).saturating_add(delta());
    i64_to_filetime(ticks)
}

fn hook_get_system_time(out: *mut SYSTEMTIME) {
    if out.is_null() {
        return;
    }
    let ft = fake_filetime(|p| unsafe { GetSystemTimeAsFileTimeDetour.call(p) });
    unsafe { FileTimeToSystemTime(&ft, out) };
}

fn hook_get_local_time(out: *mut SYSTEMTIME) {
    if out.is_null() {
        return;
    }
    // Real GetLocalTime applies the timezone offset *of the FILETIME's own date*
    // — DST is determined by the source date, not by "today". `FileTimeToLocalFileTime`
    // uses today's DST flag, which is wrong when the fake time crosses a DST
    // boundary relative to wall-clock. `SystemTimeToTzSpecificLocalTime` with a
    // NULL tz pointer uses the active tz AND the source date's DST — what we want.
    let ft_utc = fake_filetime(|p| unsafe { GetSystemTimeAsFileTimeDetour.call(p) });
    let mut st_utc: SYSTEMTIME = unsafe { std::mem::zeroed() };
    if unsafe { FileTimeToSystemTime(&ft_utc, &mut st_utc) } == 0 {
        return;
    }
    if unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &st_utc, out) } == 0 {
        // Tz conversion failed — degrade to UTC rather than leaving `out` uninit.
        unsafe { *out = st_utc };
    }
}

fn hook_get_system_time_as_filetime(out: *mut FILETIME) {
    if out.is_null() {
        return;
    }
    let ft = fake_filetime(|p| unsafe { GetSystemTimeAsFileTimeDetour.call(p) });
    unsafe { *out = ft };
}

fn hook_get_system_time_precise_as_filetime(out: *mut FILETIME) {
    if out.is_null() {
        return;
    }
    let ft = fake_filetime(|p| unsafe { GetSystemTimePreciseAsFileTimeDetour.call(p) });
    unsafe { *out = ft };
}

/// NTSTATUS for null pointer write — matches what the real NtQuerySystemTime
/// would emit when the kernel dereferences the user buffer.
const STATUS_ACCESS_VIOLATION: i32 = 0xC0000005_u32 as i32;

fn hook_nt_query_system_time(out: *mut i64) -> i32 {
    if out.is_null() {
        return STATUS_ACCESS_VIOLATION;
    }
    let mut real: i64 = 0;
    let status = unsafe { NtQuerySystemTimeDetour.call(&mut real) };
    if status != 0 {
        // Propagate the trampoline's NTSTATUS — otherwise a caller would see
        // delta+0 with a bogus STATUS_SUCCESS if the real API ever failed.
        return status;
    }
    unsafe { *out = real.saturating_add(delta()) };
    0
}
