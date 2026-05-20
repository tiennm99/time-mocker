//! Lightweight wrapper around `sysinfo` for the process tab + auto-inject scan.
//!
//! Pure data — no UI, no injection. The `App` polls `refresh()` and decides
//! what to inject based on `CompiledRules`.

use std::collections::HashSet;

use sysinfo::System;

#[derive(Debug, Clone)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub path: String,
}

pub struct ProcessWatcher {
    sys: System,
}

impl ProcessWatcher {
    pub fn new() -> Self {
        Self { sys: System::new() }
    }

    pub fn refresh(&mut self) {
        self.sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    }

    pub fn list(&self) -> Vec<ProcInfo> {
        self.sys
            .processes()
            .iter()
            .filter_map(|(pid, proc)| {
                let name = proc.name().to_string_lossy().into_owned();
                let path = proc
                    .exe()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if name.is_empty() {
                    return None;
                }
                Some(ProcInfo {
                    pid: pid.as_u32(),
                    name,
                    path,
                })
            })
            .collect()
    }

    pub fn alive_pids(&self) -> HashSet<u32> {
        self.sys.processes().keys().map(|p| p.as_u32()).collect()
    }
}
