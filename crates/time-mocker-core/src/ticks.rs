//! FILETIME / SYSTEMTIME conversion helpers.
//!
//! FILETIME = 100-ns units since 1601-01-01 00:00:00 UTC. This crate uses
//! FILETIME ticks throughout to avoid extra arithmetic on the hot path.

use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToFileTime};

#[inline]
pub fn filetime_to_i64(ft: FILETIME) -> i64 {
    ((ft.dwHighDateTime as i64) << 32) | (ft.dwLowDateTime as i64 & 0xFFFF_FFFF)
}

#[inline]
pub fn i64_to_filetime(ticks: i64) -> FILETIME {
    FILETIME {
        dwLowDateTime: (ticks & 0xFFFF_FFFF) as u32,
        dwHighDateTime: ((ticks >> 32) & 0xFFFF_FFFF) as u32,
    }
}

/// Convert FILETIME ticks to SYSTEMTIME (UTC). Returns None on Win32 failure.
pub fn ticks_to_systemtime(ticks: i64) -> Option<SYSTEMTIME> {
    let ft = i64_to_filetime(ticks);
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let ok = unsafe { FileTimeToSystemTime(&ft, &mut st) };
    if ok == 0 {
        None
    } else {
        Some(st)
    }
}

/// Convert SYSTEMTIME (UTC) to FILETIME ticks. Returns None on Win32 failure.
pub fn systemtime_to_ticks(st: &SYSTEMTIME) -> Option<i64> {
    let mut ft: FILETIME = unsafe { std::mem::zeroed() };
    let ok = unsafe { SystemTimeToFileTime(st, &mut ft) };
    if ok == 0 {
        None
    } else {
        Some(filetime_to_i64(ft))
    }
}
