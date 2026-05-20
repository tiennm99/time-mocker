//! `eframe::App` impl — three tabs (Processes, Auto-Inject Rules, Log) and a
//! global Mock Time bar at the top.

use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use eframe::{egui, CreationContext};
use serde::{Deserialize, Serialize};

use crate::injection_manager::InjectionManager;
use crate::process_watcher::{ProcInfo, ProcessWatcher};
use crate::rules::{CompiledRules, PatternKind, Rule};

/// Difference between Unix epoch (1970) and FILETIME epoch (1601), in 100-ns ticks.
const UNIX_TO_FILETIME_TICKS: i64 = 116_444_736_000_000_000;

#[derive(Default, Serialize, Deserialize)]
struct Persistent {
    rules: Vec<Rule>,
    auto_inject_enabled: bool,
    last_fake_date: Option<String>, // ISO-8601 of last applied UTC fake time
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    Processes,
    Rules,
    Log,
}

pub struct TimeMockerApp {
    persistent: Persistent,
    tab: Tab,
    manager: Option<InjectionManager>,
    manager_err: Option<String>,
    watcher: ProcessWatcher,
    processes: Vec<ProcInfo>,
    last_refresh: Instant,
    last_auto_inject_scan: Instant,
    search: String,
    rule_input: String,
    rule_kind: PatternKind,
    fake_date: NaiveDate,
    fake_time: NaiveTime,
    current_delta_ticks: i64,
}

impl TimeMockerApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        let persistent: Persistent = cc
            .storage
            .and_then(|s| eframe::get_value(s, "time_mocker"))
            .unwrap_or_default();

        let (manager, manager_err) = match InjectionManager::new() {
            Ok(m) => (Some(m), None),
            Err(e) => (None, Some(e.to_string())),
        };

        let mut watcher = ProcessWatcher::new();
        watcher.refresh();
        let processes = watcher.list();

        let now_local = Local::now();
        Self {
            persistent,
            tab: Tab::Processes,
            manager,
            manager_err,
            watcher,
            processes,
            last_refresh: Instant::now(),
            last_auto_inject_scan: Instant::now(),
            search: String::new(),
            rule_input: String::new(),
            rule_kind: PatternKind::Glob,
            fake_date: now_local.date_naive(),
            fake_time: now_local.time(),
            current_delta_ticks: 0,
        }
    }

    fn refresh_processes_if_due(&mut self) {
        if self.last_refresh.elapsed() >= Duration::from_millis(1500) {
            self.watcher.refresh();
            self.processes = self.watcher.list();
            self.processes.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            if let Some(m) = self.manager.as_mut() {
                m.prune_dead(&self.watcher.alive_pids());
            }
            self.last_refresh = Instant::now();
        }
    }

    fn auto_inject_scan_if_due(&mut self) {
        if !self.persistent.auto_inject_enabled {
            return;
        }
        if self.last_auto_inject_scan.elapsed() < Duration::from_millis(1500) {
            return;
        }
        self.last_auto_inject_scan = Instant::now();

        let compiled = CompiledRules::compile(&self.persistent.rules);
        let Some(manager) = self.manager.as_mut() else { return };

        for proc in &self.processes {
            if manager.is_injected(proc.pid) {
                continue;
            }
            if compiled.matches(&proc.path, &proc.name) {
                let _ = manager.inject(proc.pid, &proc.name, &proc.path, self.current_delta_ticks);
            }
        }
    }

    fn apply_fake_time(&mut self) {
        let naive = self.fake_date.and_time(self.fake_time);
        let local: DateTime<Local> = match Local.from_local_datetime(&naive).single() {
            Some(dt) => dt,
            None => return,
        };
        let utc: DateTime<Utc> = local.with_timezone(&Utc);
        let fake_filetime = unix_micros_to_filetime_ticks(utc.timestamp_micros());
        let real_filetime = unix_micros_to_filetime_ticks(Utc::now().timestamp_micros());
        self.current_delta_ticks = fake_filetime - real_filetime;
        if let Some(m) = self.manager.as_ref() {
            m.set_delta_all(self.current_delta_ticks);
        }
        self.persistent.last_fake_date = Some(utc.to_rfc3339());
    }

    fn reset_to_now(&mut self) {
        let now = Local::now();
        self.fake_date = now.date_naive();
        self.fake_time = now.time();
    }

    fn ui_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Mock Time");
            ui.separator();
            let mut y = self.fake_date.format("%Y").to_string();
            let mut m = self.fake_date.format("%m").to_string();
            let mut d = self.fake_date.format("%d").to_string();
            ui.label("Date:");
            ui.add(egui::TextEdit::singleline(&mut y).desired_width(48.0));
            ui.label("-");
            ui.add(egui::TextEdit::singleline(&mut m).desired_width(28.0));
            ui.label("-");
            ui.add(egui::TextEdit::singleline(&mut d).desired_width(28.0));
            if let (Ok(yi), Ok(mi), Ok(di)) = (y.parse::<i32>(), m.parse::<u32>(), d.parse::<u32>()) {
                if let Some(date) = NaiveDate::from_ymd_opt(yi, mi, di) {
                    self.fake_date = date;
                }
            }

            ui.label("Time:");
            let mut h = self.fake_time.format("%H").to_string();
            let mut mn = self.fake_time.format("%M").to_string();
            let mut s = self.fake_time.format("%S").to_string();
            ui.add(egui::TextEdit::singleline(&mut h).desired_width(28.0));
            ui.label(":");
            ui.add(egui::TextEdit::singleline(&mut mn).desired_width(28.0));
            ui.label(":");
            ui.add(egui::TextEdit::singleline(&mut s).desired_width(28.0));
            if let (Ok(hi), Ok(mi), Ok(si)) = (h.parse::<u32>(), mn.parse::<u32>(), s.parse::<u32>()) {
                if let Some(time) = NaiveTime::from_hms_opt(hi, mi, si) {
                    self.fake_time = time;
                }
            }

            if ui.button("Now").clicked() {
                self.reset_to_now();
            }
            if ui.button("Set").clicked() {
                self.apply_fake_time();
            }
            ui.separator();
            let delta_secs = self.current_delta_ticks as f64 / 10_000_000.0;
            ui.label(format!("Δ = {delta_secs:+.1}s"));
        });
    }

    fn ui_processes(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.add(egui::TextEdit::singleline(&mut self.search).desired_width(240.0));
            if ui.button("⟳ Refresh").clicked() {
                self.watcher.refresh();
                self.processes = self.watcher.list();
                self.processes.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            }
        });
        ui.separator();

        let manager_ready = self.manager.is_some();
        if let Some(err) = &self.manager_err {
            ui.colored_label(egui::Color32::RED, format!("InjectionManager unavailable: {err}"));
        }

        let needle = self.search.to_lowercase();
        let rows: Vec<ProcInfo> = self
            .processes
            .iter()
            .filter(|p| needle.is_empty() || p.name.to_lowercase().contains(&needle) || p.path.to_lowercase().contains(&needle))
            .cloned()
            .collect();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("processes")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Inject");
                    ui.strong("PID");
                    ui.strong("Name");
                    ui.strong("Path");
                    ui.end_row();

                    for p in &rows {
                        let injected = self
                            .manager
                            .as_ref()
                            .map(|m| m.is_injected(p.pid))
                            .unwrap_or(false);
                        let mut checked = injected;
                        let resp = ui.add_enabled(manager_ready, egui::Checkbox::new(&mut checked, ""));
                        if resp.changed() {
                            if let Some(m) = self.manager.as_mut() {
                                if checked {
                                    let _ = m.inject(p.pid, &p.name, &p.path, self.current_delta_ticks);
                                } else {
                                    m.eject(p.pid);
                                }
                            }
                        }
                        ui.label(p.pid.to_string());
                        ui.label(&p.name);
                        ui.label(&p.path);
                        ui.end_row();
                    }
                });
        });
    }

    fn ui_rules(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(
            &mut self.persistent.auto_inject_enabled,
            "Enable auto-inject watcher",
        );
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Pattern:");
            ui.add(egui::TextEdit::singleline(&mut self.rule_input).desired_width(360.0));
            egui::ComboBox::from_id_salt("rule_kind")
                .selected_text(self.rule_kind.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.rule_kind, PatternKind::Glob, "Glob");
                    ui.selectable_value(&mut self.rule_kind, PatternKind::Regex, "Regex");
                });
            if ui.button("+ Add Rule").clicked() && !self.rule_input.trim().is_empty() {
                self.persistent.rules.push(Rule {
                    pattern: self.rule_input.trim().to_owned(),
                    kind: self.rule_kind,
                    enabled: true,
                });
                self.rule_input.clear();
            }
        });
        ui.separator();

        let mut remove_idx: Option<usize> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("rules")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("On");
                    ui.strong("Kind");
                    ui.strong("Pattern");
                    ui.strong("");
                    ui.end_row();
                    for (i, rule) in self.persistent.rules.iter_mut().enumerate() {
                        ui.checkbox(&mut rule.enabled, "");
                        ui.label(rule.kind.label());
                        ui.label(&rule.pattern);
                        if ui.button("✕").clicked() {
                            remove_idx = Some(i);
                        }
                        ui.end_row();
                    }
                });
        });
        if let Some(i) = remove_idx {
            self.persistent.rules.remove(i);
        }
    }

    fn ui_log(&mut self, ui: &mut egui::Ui) {
        let Some(m) = self.manager.as_ref() else {
            ui.label("InjectionManager unavailable.");
            return;
        };
        ui.label(format!("Hook DLL: {}", m.hook_dll_path().display()));
        ui.separator();
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &m.log {
                    ui.monospace(line);
                }
            });
    }
}

impl eframe::App for TimeMockerApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "time_mocker", &self.persistent);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_processes_if_due();
        self.auto_inject_scan_if_due();
        ctx.request_repaint_after(Duration::from_millis(500));

        egui::TopBottomPanel::top("topbar").show(ctx, |ui| {
            self.ui_top_bar(ui);
        });

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Processes, "Processes");
                ui.selectable_value(&mut self.tab, Tab::Rules, "Auto-Inject Rules");
                ui.selectable_value(&mut self.tab, Tab::Log, "Log");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Processes => self.ui_processes(ui),
            Tab::Rules => self.ui_rules(ui),
            Tab::Log => self.ui_log(ui),
        });
    }
}

#[inline]
fn unix_micros_to_filetime_ticks(unix_micros: i64) -> i64 {
    UNIX_TO_FILETIME_TICKS + unix_micros * 10
}
