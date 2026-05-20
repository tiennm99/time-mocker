//! Per-process injection state.
//!
//! For each injected PID we keep:
//! - the `dll-syringe` process handle (so the DLL stays loaded)
//! - a `SharedDeltaWriter` (so we can update the fake time)
//!
//! On UI shutdown, `Drop` zeroes every injected process's delta so the target
//! goes back to real time even though the hook DLL remains loaded.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use dll_syringe::process::OwnedProcess;
use dll_syringe::Syringe;
use time_mocker_core::{mmf_name_for_pid, CreateOutcome, SharedDeltaWriter};

use crate::win32_process_info::{is_native_x64, query_full_image_name, IMAGE_FILE_MACHINE_AMD64};

/// Critical Windows processes that must never be injected — they would
/// destabilize the OS, fail with access denied, or trigger AV alerts.
const SYSTEM_PROCESS_EXCLUDE: &[&str] = &[
    "system",
    "registry",
    "memory compression",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "svchost.exe",
    "audiodg.exe",
    "dwm.exe",
    "fontdrvhost.exe",
    "msmpeng.exe",
    "nissrv.exe",
    "securityhealthservice.exe",
];

const LOG_CAP: usize = 1000;

#[allow(dead_code)]
pub struct InjectedProcess {
    pub pid: u32,
    pub name: String,
    pub path: String,
    delta: SharedDeltaWriter,
    _syringe: Syringe,
}

impl InjectedProcess {
    pub fn write_delta(&self, ticks: i64) {
        self.delta.write_delta(ticks);
    }
}

pub struct InjectionManager {
    injected: HashMap<u32, InjectedProcess>,
    hook_dll_path: PathBuf,
    pub log: VecDeque<String>,
}

impl InjectionManager {
    pub fn new() -> Result<Self> {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .ok_or_else(|| anyhow!("cannot resolve exe directory"))?;
        let hook_dll_path = exe_dir.join("time_mocker_hook.dll");

        // Validate the hook DLL's architecture once at startup so we fail
        // loudly rather than producing opaque dll-syringe errors at inject time.
        if hook_dll_path.exists() {
            let machine = crate::win32_process_info::pe_machine(&hook_dll_path)
                .with_context(|| format!("read PE header of {}", hook_dll_path.display()))?;
            if machine != IMAGE_FILE_MACHINE_AMD64 {
                return Err(anyhow!(
                    "hook DLL machine={machine:#x}, expected AMD64 ({IMAGE_FILE_MACHINE_AMD64:#x})"
                ));
            }
        }

        Ok(Self {
            injected: HashMap::new(),
            hook_dll_path,
            log: VecDeque::new(),
        })
    }

    pub fn hook_dll_path(&self) -> &Path {
        &self.hook_dll_path
    }

    pub fn is_injected(&self, pid: u32) -> bool {
        self.injected.contains_key(&pid)
    }

    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = &InjectedProcess> {
        self.injected.values()
    }

    pub fn inject(&mut self, pid: u32, name: &str, path: &str, initial_delta: i64) -> Result<()> {
        match self.inject_inner(pid, name, path, initial_delta) {
            Ok(()) => Ok(()),
            Err(e) => {
                // Auto-inject scanner discards inject Errs; logging here makes
                // every failure (including silent auto-inject loops) visible in
                // the Log tab without forcing every caller to handle the Err.
                self.log_push(format!("inject pid={pid} ({name}) failed: {e:#}"));
                Err(e)
            }
        }
    }

    fn inject_inner(&mut self, pid: u32, name: &str, path: &str, initial_delta: i64) -> Result<()> {
        if self.injected.contains_key(&pid) {
            return Ok(());
        }
        if !self.hook_dll_path.exists() {
            return Err(anyhow!(
                "hook DLL not found at {}",
                self.hook_dll_path.display()
            ));
        }

        // PID-reuse guard: between the watcher snapshot and now, the PID may
        // have been recycled. Verify the image path still matches; derive the
        // LIVE filename from the live path so the system-process check below
        // doesn't trust the (possibly stale) snapshot name.
        let live_path = query_full_image_name(pid)
            .with_context(|| format!("query image name for pid={pid}"))?;
        if !paths_equivalent(&live_path, path) {
            return Err(anyhow!(
                "pid={pid} image mismatch: expected `{path}`, got `{live_path}` (PID reuse?)"
            ));
        }
        let live_name = Path::new(&live_path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.to_owned());
        if is_system_process(&live_name) {
            return Err(anyhow!(
                "refusing to inject system process {live_name} (pid={pid})"
            ));
        }

        if !is_native_x64(pid) {
            return Err(anyhow!(
                "pid={pid} is not a native x64 process; the AMD64 hook DLL cannot be injected"
            ));
        }

        let mmf_name = mmf_name_for_pid(pid);
        let (delta, outcome) = SharedDeltaWriter::create(&mmf_name)
            .with_context(|| format!("create MMF {mmf_name}"))?;
        if outcome == CreateOutcome::Existed {
            self.log_push(format!(
                "warn: MMF {mmf_name} pre-existed (stale prior session?)"
            ));
        }
        delta.write_delta(initial_delta);

        let process = OwnedProcess::from_pid(pid)
            .with_context(|| format!("open process pid={pid}"))?;
        let syringe = Syringe::for_process(process);
        syringe
            .inject(&self.hook_dll_path)
            .with_context(|| format!("inject {} into pid={}", self.hook_dll_path.display(), pid))?;

        self.log_push(format!("Injected into [{pid}] {name}"));
        self.injected.insert(
            pid,
            InjectedProcess {
                pid,
                name: name.to_owned(),
                path: path.to_owned(),
                delta,
                _syringe: syringe,
            },
        );
        Ok(())
    }

    pub fn set_delta_all(&self, ticks: i64) {
        for proc in self.injected.values() {
            proc.write_delta(ticks);
        }
    }

    /// Zero the delta and drop our handle. The DLL stays loaded in the target
    /// (dll-syringe `eject` is not wired up — see report Q2); from the target's
    /// perspective time is back to real once the delta is zero.
    pub fn disable(&mut self, pid: u32) {
        if let Some(p) = self.injected.remove(&pid) {
            p.write_delta(0);
            self.log_push(format!("Disabled [{pid}] {}", p.name));
        }
    }

    /// Drop entries for processes that have exited.
    pub fn prune_dead(&mut self, alive: &std::collections::HashSet<u32>) {
        let dead: Vec<u32> = self
            .injected
            .keys()
            .copied()
            .filter(|pid| !alive.contains(pid))
            .collect();
        for pid in dead {
            self.injected.remove(&pid);
        }
    }

    fn log_push(&mut self, line: String) {
        self.log.push_back(line);
        while self.log.len() > LOG_CAP {
            self.log.pop_front();
        }
    }
}

impl Drop for InjectionManager {
    fn drop(&mut self) {
        // Best-effort: zero every injected process's delta so targets return
        // to real time on UI exit. The MMF handles are dropped right after.
        for proc in self.injected.values() {
            proc.write_delta(0);
        }
    }
}

fn is_system_process(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SYSTEM_PROCESS_EXCLUDE.iter().any(|n| *n == lower)
}

pub(crate) fn paths_equivalent(a: &str, b: &str) -> bool {
    // Use Unicode-aware lowercasing so non-ASCII case differences (accented Latin,
    // Cyrillic, CJK) don't yield false-negative "PID reuse?" errors on i18n paths.
    // The two allocations are amortized — called once per inject, never in hot path.
    a.to_lowercase() == b.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_system_process_csrss() {
        assert!(is_system_process("csrss.exe"));
    }

    #[test]
    fn is_system_process_csrss_uppercase() {
        assert!(is_system_process("CSRSS.EXE"));
    }

    #[test]
    fn is_system_process_csrss_mixedcase() {
        assert!(is_system_process("CsRsS.ExE"));
    }

    #[test]
    fn is_system_process_system() {
        assert!(is_system_process("system"));
    }

    #[test]
    fn is_system_process_system_uppercase() {
        assert!(is_system_process("SYSTEM"));
    }

    #[test]
    fn is_system_process_registry() {
        assert!(is_system_process("registry"));
    }

    #[test]
    fn is_system_process_svchost() {
        assert!(is_system_process("svchost.exe"));
    }

    #[test]
    fn is_system_process_dwm() {
        assert!(is_system_process("dwm.exe"));
    }

    #[test]
    fn is_system_process_msmpeng() {
        assert!(is_system_process("msmpeng.exe"));
    }

    #[test]
    fn is_system_process_non_system() {
        assert!(!is_system_process("notepad.exe"));
    }

    #[test]
    fn is_system_process_non_system_uppercase() {
        assert!(!is_system_process("NOTEPAD.EXE"));
    }

    #[test]
    fn is_system_process_myapp() {
        assert!(!is_system_process("MyApp.exe"));
    }

    #[test]
    fn is_system_process_empty_string() {
        assert!(!is_system_process(""));
    }

    #[test]
    fn is_system_process_partial_match_should_not_match() {
        // "csrss" (without .exe) should not match "csrss.exe"
        assert!(!is_system_process("csrss"));
    }

    #[test]
    fn paths_equivalent_same() {
        assert!(paths_equivalent("C:\\foo\\bar.exe", "C:\\foo\\bar.exe"));
    }

    #[test]
    fn paths_equivalent_case_insensitive() {
        assert!(paths_equivalent("C:\\Foo\\Bar.exe", "c:\\foo\\bar.exe"));
    }

    #[test]
    fn paths_equivalent_mixed_case() {
        assert!(paths_equivalent(
            "C:\\Windows\\System32\\notepad.exe",
            "c:\\windows\\system32\\NOTEPAD.EXE"
        ));
    }

    #[test]
    fn paths_equivalent_different_paths() {
        assert!(!paths_equivalent("C:\\foo\\bar.exe", "C:\\baz\\bar.exe"));
    }

    #[test]
    fn paths_equivalent_different_filenames() {
        assert!(!paths_equivalent("C:\\foo\\bar.exe", "C:\\foo\\baz.exe"));
    }

    #[test]
    fn paths_equivalent_empty_strings() {
        assert!(paths_equivalent("", ""));
    }

    #[test]
    fn paths_equivalent_one_empty() {
        assert!(!paths_equivalent("C:\\foo.exe", ""));
    }

    #[test]
    fn injection_manager_log_bounded() {
        let mut manager = InjectionManager {
            injected: HashMap::new(),
            hook_dll_path: std::path::PathBuf::from("dummy.dll"),
            log: VecDeque::new(),
        };

        // Push LOG_CAP + 100 entries and verify only LOG_CAP remain
        for i in 0..(LOG_CAP + 100) {
            manager.log_push(format!("line {}", i));
        }

        assert_eq!(
            manager.log.len(),
            LOG_CAP,
            "log should stay bounded at LOG_CAP"
        );
        // First message should be gone (front was popped)
        let first_kept = manager.log.front().unwrap();
        assert!(
            first_kept.contains("line 100"),
            "oldest entry should be from the 100th push"
        );
    }
}
