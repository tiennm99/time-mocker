//! Named memory-mapped file wrapper around the 8-byte `MockTimeInfo` payload.
//!
//! Both the controller (writer) and the injected hook DLL (reader) attach to
//! the same `TimeMocker_<pid>` mapping. We use raw `windows-sys` because
//! `memmap2` doesn't expose named pagefile-backed mappings on Windows.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::sync::atomic::{AtomicI64, Ordering};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
    FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
};

use crate::types::MockTimeInfo;

/// Read/write handle to the shared delta.
pub struct SharedDelta {
    handle: HANDLE,
    view: *mut AtomicI64,
    #[allow(dead_code)]
    name: String,
}

unsafe impl Send for SharedDelta {}
unsafe impl Sync for SharedDelta {}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

impl SharedDelta {
    /// Create (or open) the named mapping. Used by the controller side.
    pub fn create(name: &str) -> io::Result<Self> {
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
        Self::map_view(handle, name, FILE_MAP_ALL_ACCESS)
    }

    /// Open an existing mapping. Used by the injected hook DLL.
    pub fn open(name: &str) -> io::Result<Self> {
        let wname = wide(name);
        let handle = unsafe { OpenFileMappingW(FILE_MAP_READ, 0, wname.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::from_raw_os_error(unsafe { GetLastError() } as i32));
        }
        Self::map_view(handle, name, FILE_MAP_READ)
    }

    fn map_view(handle: HANDLE, name: &str, access: u32) -> io::Result<Self> {
        let view: MEMORY_MAPPED_VIEW_ADDRESS = unsafe {
            MapViewOfFile(handle, access, 0, 0, MockTimeInfo::SIZE)
        };
        if view.Value.is_null() {
            let err = unsafe { GetLastError() } as i32;
            unsafe { CloseHandle(handle) };
            return Err(io::Error::from_raw_os_error(err));
        }
        Ok(Self {
            handle,
            view: view.Value as *mut AtomicI64,
            name: name.to_owned(),
        })
    }

    /// Atomically write the delta. Safe for concurrent reads from the hook.
    #[inline]
    pub fn write_delta(&self, ticks: i64) {
        unsafe { (*self.view).store(ticks, Ordering::Relaxed) }
    }

    /// Atomically read the delta. Hot path on the hook side.
    #[inline]
    pub fn read_delta(&self) -> i64 {
        unsafe { (*self.view).load(Ordering::Relaxed) }
    }
}

impl Drop for SharedDelta {
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
