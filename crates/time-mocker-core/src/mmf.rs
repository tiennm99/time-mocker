//! Named memory-mapped file wrapper around the 8-byte `MockTimeInfo` payload.
//!
//! Two distinct types model the access split: `SharedDeltaWriter` (controller
//! side, FILE_MAP_ALL_ACCESS) and `SharedDeltaReader` (injected hook DLL,
//! FILE_MAP_READ). Both alias the same kernel object via its name.
//!
//! Raw `windows-sys` is used because `memmap2` doesn't expose named
//! pagefile-backed mappings on Windows.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::sync::atomic::{AtomicI64, Ordering};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
    FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
};

use crate::types::MockTimeInfo;

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// Shared bookkeeping for an open mapping. Owns the handle + view and frees
/// both on Drop. `view` is page-aligned (`MapViewOfFile` guarantee) so the
/// underlying i64 is 8-byte aligned and tear-free under `AtomicI64::from_ptr`.
struct MappingHandle {
    handle: HANDLE,
    view: *mut i64,
}

// Safety: the handle/view are stable for the lifetime of `MappingHandle` and
// the i64 is accessed only through `AtomicI64::from_ptr` (Relaxed ordering).
unsafe impl Send for MappingHandle {}
unsafe impl Sync for MappingHandle {}

impl MappingHandle {
    #[inline]
    fn as_atomic(&self) -> &AtomicI64 {
        // Safety: view is non-null and 8-byte aligned; sole-purpose memory.
        unsafe { AtomicI64::from_ptr(self.view) }
    }
}

impl Drop for MappingHandle {
    fn drop(&mut self) {
        unsafe {
            if !self.view.is_null() {
                let addr = MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.view as *mut _,
                };
                UnmapViewOfFile(addr);
            }
            if !self.handle.is_null() {
                CloseHandle(self.handle);
            }
        }
    }
}

unsafe fn map_view(handle: HANDLE, access: u32) -> io::Result<*mut i64> {
    let view: MEMORY_MAPPED_VIEW_ADDRESS = MapViewOfFile(handle, access, 0, 0, MockTimeInfo::SIZE);
    if view.Value.is_null() {
        let err = GetLastError() as i32;
        CloseHandle(handle);
        return Err(io::Error::from_raw_os_error(err));
    }
    Ok(view.Value as *mut i64)
}

/// Did `SharedDeltaWriter::create` create a fresh kernel object, or attach to
/// a pre-existing one (typically from a crashed prior controller session)?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateOutcome {
    Fresh,
    Existed,
}

/// Read/write handle. Used by the controller to publish the current delta.
pub struct SharedDeltaWriter(MappingHandle);

impl SharedDeltaWriter {
    /// Create or attach to the named mapping.
    ///
    /// Returns `(writer, CreateOutcome::Existed)` if the mapping was already
    /// present — the caller should log a warning but may proceed (the payload
    /// is just an 8-byte delta and will be overwritten).
    pub fn create(name: &str) -> io::Result<(Self, CreateOutcome)> {
        let wname = wide(name);
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                ptr::null(),
                PAGE_READWRITE,
                0,
                MockTimeInfo::SIZE as u32,
                wname.as_ptr(),
            )
        };
        if handle.is_null() {
            return Err(io::Error::from_raw_os_error(unsafe { GetLastError() } as i32));
        }
        // Capture ERROR_ALREADY_EXISTS *before* any other syscall that may overwrite it.
        let outcome = if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            CreateOutcome::Existed
        } else {
            CreateOutcome::Fresh
        };
        let view = unsafe { map_view(handle, FILE_MAP_ALL_ACCESS)? };
        Ok((Self(MappingHandle { handle, view }), outcome))
    }

    #[inline]
    pub fn write_delta(&self, ticks: i64) {
        self.0.as_atomic().store(ticks, Ordering::Relaxed);
    }
}

/// Read-only handle. Used by the injected hook DLL on every time-API call.
pub struct SharedDeltaReader(MappingHandle);

impl SharedDeltaReader {
    pub fn open(name: &str) -> io::Result<Self> {
        let wname = wide(name);
        let handle = unsafe { OpenFileMappingW(FILE_MAP_READ, 0, wname.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::from_raw_os_error(unsafe { GetLastError() } as i32));
        }
        let view = unsafe { map_view(handle, FILE_MAP_READ)? };
        Ok(Self(MappingHandle { handle, view }))
    }

    #[inline]
    pub fn read_delta(&self) -> i64 {
        self.0.as_atomic().load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_mmf_name(tag: &str) -> String {
        // Use unprefixed names for tests to avoid Global\ namespace issues
        format!("TimeMockerTest_{}_{}", tag, std::process::id())
    }

    #[test]
    fn mmf_write_read_roundtrip_zero() {
        let name = test_mmf_name("zero");
        let (writer, outcome) =
            SharedDeltaWriter::create(&name).expect("create writer for zero test");
        assert_eq!(outcome, CreateOutcome::Fresh);

        writer.write_delta(0);
        let reader = SharedDeltaReader::open(&name).expect("open reader for zero test");
        let read_val = reader.read_delta();
        assert_eq!(read_val, 0);
    }

    #[test]
    fn mmf_write_read_roundtrip_positive() {
        let name = test_mmf_name("positive");
        let (writer, outcome) =
            SharedDeltaWriter::create(&name).expect("create writer for positive test");
        assert_eq!(outcome, CreateOutcome::Fresh);

        let test_val: i64 = 1_234_567_890_123_456;
        writer.write_delta(test_val);
        let reader = SharedDeltaReader::open(&name).expect("open reader for positive test");
        let read_val = reader.read_delta();
        assert_eq!(read_val, test_val);
    }

    #[test]
    fn mmf_write_read_roundtrip_negative() {
        let name = test_mmf_name("negative");
        let (writer, outcome) =
            SharedDeltaWriter::create(&name).expect("create writer for negative test");
        assert_eq!(outcome, CreateOutcome::Fresh);

        let test_val: i64 = -1_234_567_890_123_456;
        writer.write_delta(test_val);
        let reader = SharedDeltaReader::open(&name).expect("open reader for negative test");
        let read_val = reader.read_delta();
        assert_eq!(read_val, test_val);
    }

    #[test]
    fn mmf_write_read_roundtrip_i64_max() {
        let name = test_mmf_name("i64_max");
        let (writer, outcome) =
            SharedDeltaWriter::create(&name).expect("create writer for i64::MAX test");
        assert_eq!(outcome, CreateOutcome::Fresh);

        writer.write_delta(i64::MAX);
        let reader = SharedDeltaReader::open(&name).expect("open reader for i64::MAX test");
        let read_val = reader.read_delta();
        assert_eq!(read_val, i64::MAX);
    }

    #[test]
    fn mmf_write_read_roundtrip_i64_min() {
        let name = test_mmf_name("i64_min");
        let (writer, outcome) =
            SharedDeltaWriter::create(&name).expect("create writer for i64::MIN test");
        assert_eq!(outcome, CreateOutcome::Fresh);

        writer.write_delta(i64::MIN);
        let reader = SharedDeltaReader::open(&name).expect("open reader for i64::MIN test");
        let read_val = reader.read_delta();
        assert_eq!(read_val, i64::MIN);
    }

    #[test]
    fn mmf_detect_preexisting_mapping() {
        let name = test_mmf_name("preexist");
        let (writer1, outcome1) =
            SharedDeltaWriter::create(&name).expect("first create should succeed");
        assert_eq!(outcome1, CreateOutcome::Fresh);

        // Write a test value via the first writer
        let test_val: i64 = 42;
        writer1.write_delta(test_val);

        // Second create against the same name should detect it exists
        let (_writer2, outcome2) =
            SharedDeltaWriter::create(&name).expect("second create should succeed");
        assert_eq!(outcome2, CreateOutcome::Existed);

        // Both writers should alias the same kernel object; read via reader
        let reader = SharedDeltaReader::open(&name).expect("open reader for preexist test");
        let read_val = reader.read_delta();
        assert_eq!(
            read_val, test_val,
            "readers should observe writes from either writer"
        );
    }

    #[test]
    fn mmf_multiple_readers_see_same_value() {
        let name = test_mmf_name("multi_reader");
        let (writer, outcome) =
            SharedDeltaWriter::create(&name).expect("create writer for multi_reader test");
        assert_eq!(outcome, CreateOutcome::Fresh);

        let test_val: i64 = 99_999_999;
        writer.write_delta(test_val);

        let reader1 = SharedDeltaReader::open(&name).expect("open reader1");
        let reader2 = SharedDeltaReader::open(&name).expect("open reader2");

        assert_eq!(reader1.read_delta(), test_val);
        assert_eq!(reader2.read_delta(), test_val);
    }

    #[test]
    fn mmf_write_visibility() {
        let name = test_mmf_name("write_vis");
        let (writer, _) = SharedDeltaWriter::create(&name).expect("create writer");
        let reader = SharedDeltaReader::open(&name).expect("open reader");

        // Write via writer, immediately read via reader
        writer.write_delta(111);
        assert_eq!(reader.read_delta(), 111);

        writer.write_delta(222);
        assert_eq!(reader.read_delta(), 222);

        writer.write_delta(-999);
        assert_eq!(reader.read_delta(), -999);
    }
}
