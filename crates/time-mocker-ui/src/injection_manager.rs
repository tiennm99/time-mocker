//! Per-process injection state.
//!
//! For each injected PID we keep:
//! - the `dll-syringe` process handle (so the DLL stays loaded)
//! - a `SharedDelta` writer (so we can update the fake time)

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use dll_syringe::process::OwnedProcess;
use dll_syringe::Syringe;
use time_mocker_core::{mmf_name_for_pid, SharedDelta};

#[allow(dead_code)]
pub struct InjectedProcess {
    pub pid: u32,
    pub name: String,
    pub path: String,
    delta: SharedDelta,
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
    pub log: Vec<String>,
}

impl InjectionManager {
    pub fn new() -> Result<Self> {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .ok_or_else(|| anyhow!("cannot resolve exe directory"))?;
        let hook_dll_path = exe_dir.join("time_mocker_hook.dll");
        Ok(Self {
            injected: HashMap::new(),
            hook_dll_path,
            log: Vec::new(),
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
        if self.injected.contains_key(&pid) {
            return Ok(());
        }
        if !self.hook_dll_path.exists() {
            return Err(anyhow!(
                "hook DLL not found at {}",
                self.hook_dll_path.display()
            ));
        }

        let mmf_name = mmf_name_for_pid(pid);
        let delta = SharedDelta::create(&mmf_name)
            .with_context(|| format!("create MMF {mmf_name}"))?;
        delta.write_delta(initial_delta);

        let process = OwnedProcess::from_pid(pid)
            .with_context(|| format!("open process pid={pid}"))?;
        let syringe = Syringe::for_process(process);
        syringe
            .inject(&self.hook_dll_path)
            .with_context(|| format!("inject {} into pid={}", self.hook_dll_path.display(), pid))?;

        self.log
            .push(format!("Injected into [{pid}] {name}"));
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

    pub fn eject(&mut self, pid: u32) {
        if let Some(p) = self.injected.remove(&pid) {
            // Best-effort: writing 0 restores real time even if the DLL stays loaded.
            p.write_delta(0);
            self.log.push(format!("Ejected [{pid}] {}", p.name));
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
}
