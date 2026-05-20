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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_i64_roundtrip() {
        // Covers: zero, small, Unix epoch (1970 in FILETIME ticks), a recent time,
        // and the i64 boundary (used as a saturating-add sentinel by the hooks).
        let cases = [
            0_i64,
            1,
            100,
            116_444_736_000_000_000, // 1970-01-01 UTC in FILETIME ticks
            132_000_000_000_000_000, // ~2019
            i64::MAX,
            i64::MIN,
            -1,
        ];
        for &t in &cases {
            let ft = i64_to_filetime(t);
            assert_eq!(filetime_to_i64(ft), t, "round-trip failed for {t}");
        }
    }

    #[test]
    fn systemtime_roundtrip_utc() {
        // 2020-06-15 12:34:56 UTC
        let st = SYSTEMTIME {
            wYear: 2020,
            wMonth: 6,
            wDayOfWeek: 0,
            wDay: 15,
            wHour: 12,
            wMinute: 34,
            wSecond: 56,
            wMilliseconds: 0,
        };
        let ticks = systemtime_to_ticks(&st).expect("systemtime_to_ticks");
        let back = ticks_to_systemtime(ticks).expect("ticks_to_systemtime");
        assert_eq!(back.wYear, st.wYear);
        assert_eq!(back.wMonth, st.wMonth);
        assert_eq!(back.wDay, st.wDay);
        assert_eq!(back.wHour, st.wHour);
        assert_eq!(back.wMinute, st.wMinute);
        assert_eq!(back.wSecond, st.wSecond);
    }
}
