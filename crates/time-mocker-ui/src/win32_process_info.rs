//! Thin Win32 helpers used by the injection manager:
//! - PE machine field of an on-disk DLL (architecture validation)
//! - QueryFullProcessImageName for live PID-reuse detection
//! - IsWow64Process2 for target-bitness check

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::os::windows::ffi::OsStringExt;
use std::path::Path;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    IsWow64Process2, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};

pub const IMAGE_FILE_MACHINE_UNKNOWN: u16 = 0;
pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;

/// Read the COFF Machine field from an on-disk PE (DLL or EXE).
///
/// Reads up to 64 KiB so packed / obfuscated PEs with large `e_lfanew` values
/// (typical PE files have `e_lfanew` ≈ 0x40..0x200, but the spec permits much
/// larger) still parse cleanly.
pub fn pe_machine(path: &Path) -> io::Result<u16> {
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut filled = 0usize;
    while filled < buf.len() {
        match file.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    let n = filled;
    if n < 0x40 || &buf[0..2] != b"MZ" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a PE file"));
    }
    let e_lfanew = u32::from_le_bytes([buf[0x3C], buf[0x3D], buf[0x3E], buf[0x3F]]) as usize;
    if e_lfanew.saturating_add(6) > n || &buf[e_lfanew..e_lfanew + 4] != b"PE\0\0" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "missing PE signature"));
    }
    Ok(u16::from_le_bytes([buf[e_lfanew + 4], buf[e_lfanew + 5]]))
}

/// Return the full image path of a live process, or `Err` if the process
/// is gone / inaccessible. Used to detect PID reuse between watcher
/// refresh and inject.
pub fn query_full_image_name(pid: u32) -> io::Result<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(h);
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(OsString::from_wide(&buf[..size as usize])
            .to_string_lossy()
            .into_owned())
    }
}

/// True iff the target process is native AMD64 (not WOW64). The hook DLL is
/// AMD64-only; injecting it into a 32-bit WOW64 process produces an opaque
/// dll-syringe error well after the user committed.
pub fn is_native_x64(pid: u32) -> bool {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut process_machine: u16 = 0;
        let mut native_machine: u16 = 0;
        let ok = IsWow64Process2(h, &mut process_machine, &mut native_machine);
        CloseHandle(h);
        if ok == 0 {
            return false;
        }
        // Native process: process_machine == UNKNOWN. Native arch must be AMD64.
        process_machine == IMAGE_FILE_MACHINE_UNKNOWN && native_machine == IMAGE_FILE_MACHINE_AMD64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn pe_machine_reads_amd64() {
        // Build path: ../../../target/release/time_mocker_hook.dll
        let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        let dll_path = std::path::PathBuf::from(&manifest)
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("target/release/time_mocker_hook.dll"))
            .expect("derive target/release path");

        // Skip test if DLL not found (e.g., release build not run yet)
        if !dll_path.exists() {
            eprintln!(
                "Skipping pe_machine test: DLL not found at {}",
                dll_path.display()
            );
            return;
        }

        let machine = pe_machine(&dll_path).expect("read PE machine from hook DLL");
        assert_eq!(
            machine, IMAGE_FILE_MACHINE_AMD64,
            "hook DLL must be AMD64 (0x8664), got {:#x}",
            machine
        );
    }

    #[test]
    fn pe_machine_rejects_short_file() {
        use tempfile::NamedTempFile;
        let mut f = NamedTempFile::new().expect("create temp file");
        f.write_all(b"MZ").expect("write short MZ header");
        f.flush().expect("flush");

        let result = pe_machine(f.path());
        assert!(
            result.is_err(),
            "should reject file shorter than PE header offset"
        );
    }

    #[test]
    fn pe_machine_rejects_no_mz() {
        use tempfile::NamedTempFile;
        let mut f = NamedTempFile::new().expect("create temp file");
        f.write_all(&[0u8; 0x40]).expect("write 64 zero bytes");
        f.flush().expect("flush");

        let result = pe_machine(f.path());
        assert!(result.is_err(), "should reject file without MZ signature");
    }

    #[test]
    fn pe_machine_rejects_invalid_e_lfanew() {
        use tempfile::NamedTempFile;
        let mut f = NamedTempFile::new().expect("create temp file");
        let mut buf = [0u8; 0x40];
        buf[0..2].copy_from_slice(b"MZ");
        // e_lfanew at offset 0x3C: set to a huge offset that exceeds file size
        buf[0x3C..0x40].copy_from_slice(&0x10000_u32.to_le_bytes());
        f.write_all(&buf).expect("write header");
        f.flush().expect("flush");

        let result = pe_machine(f.path());
        assert!(
            result.is_err(),
            "should reject file with e_lfanew beyond file bounds"
        );
    }

    #[test]
    fn pe_machine_rejects_missing_pe_signature() {
        use tempfile::NamedTempFile;
        let mut f = NamedTempFile::new().expect("create temp file");
        let mut buf = [0u8; 512];
        buf[0..2].copy_from_slice(b"MZ");
        // e_lfanew = 0x40 (valid offset)
        buf[0x3C..0x40].copy_from_slice(&0x40_u32.to_le_bytes());
        // Don't write PE signature at 0x40, leave zeros
        f.write_all(&buf).expect("write header");
        f.flush().expect("flush");

        let result = pe_machine(f.path());
        assert!(result.is_err(), "should reject file without PE signature");
    }

    #[test]
    fn is_native_x64_self() {
        // Test on the current process (which must be native x64 if tests run)
        let self_pid = std::process::id();
        let result = is_native_x64(self_pid);
        // If we're running in x64 mode, this should be true
        #[cfg(target_arch = "x86_64")]
        assert!(result, "self process (x64) should report as native x64");
        // x86 builds would report false, but we're x64-only for this project
    }
}
