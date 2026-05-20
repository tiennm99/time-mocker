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

/// Sane bounds for the date picker — clamp here so `unix_micros * 10` never overflows.
const MIN_YEAR: i32 = 1970;
const MAX_YEAR: i32 = 2200;

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
    fake_year: i32,
    fake_month: u32,
    fake_day: u32,
    fake_hour: u32,
    fake_minute: u32,
    fake_second: u32,
    current_delta_ticks: i64,
    status_msg: Option<String>,
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
        let date = now_local.date_naive();
        let time = now_local.time();
        use chrono::{Datelike, Timelike};
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
            fake_year: date.year(),
            fake_month: date.month(),
            fake_day: date.day(),
            fake_hour: time.hour(),
            fake_minute: time.minute(),
            fake_second: time.second(),
            current_delta_ticks: 0,
            status_msg: None,
        }
    }

    fn refresh_processes_if_due(&mut self) {
        if self.last_refresh.elapsed() >= Duration::from_millis(1500) {
            self.watcher.refresh();
            self.processes = self.watcher.list();
            self.processes
                .sort_by_key(|a| a.name.to_lowercase());
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
        let Some(manager) = self.manager.as_mut() else {
            return;
        };

        for proc in &self.processes {
            if manager.is_injected(proc.pid) {
                continue;
            }
            if compiled.matches(&proc.path, &proc.name) {
                let _ = manager.inject(proc.pid, &proc.name, &proc.path, self.current_delta_ticks);
            }
        }
    }

    fn picked_naive_dt(&self) -> Option<chrono::NaiveDateTime> {
        let date = NaiveDate::from_ymd_opt(self.fake_year, self.fake_month, self.fake_day)?;
        let time = NaiveTime::from_hms_opt(self.fake_hour, self.fake_minute, self.fake_second)?;
        Some(date.and_time(time))
    }

    fn apply_fake_time(&mut self) {
        let Some(naive) = self.picked_naive_dt() else {
            self.status_msg = Some("invalid date/time fields".into());
            return;
        };
        let local: DateTime<Local> = match Local.from_local_datetime(&naive).single() {
            Some(dt) => dt,
            None => {
                // DST gap (spring-forward) or ambiguous (fall-back) — surface so the
                // user knows the click was a no-op.
                self.status_msg =
                    Some("DST transition: time is ambiguous or skipped — pick a nearby minute".into());
                return;
            }
        };
        let utc: DateTime<Utc> = local.with_timezone(&Utc);
        let Some(fake_filetime) = unix_micros_to_filetime_ticks(utc.timestamp_micros()) else {
            self.status_msg = Some("fake time overflows FILETIME range".into());
            return;
        };
        let Some(real_filetime) = unix_micros_to_filetime_ticks(Utc::now().timestamp_micros())
        else {
            self.status_msg = Some("real time overflows FILETIME range".into());
            return;
        };
        self.current_delta_ticks = fake_filetime - real_filetime;
        if let Some(m) = self.manager.as_ref() {
            m.set_delta_all(self.current_delta_ticks);
        }
        self.persistent.last_fake_date = Some(utc.to_rfc3339());
        self.status_msg = None;
    }

    /// "Now" button — set the picker to current local time and apply, which
    /// drives delta ≈ 0 (i.e., disable any mock). Auto-apply matches the
    /// label's verb-form ("Now" = "go to now"), not a passive reset.
    fn reset_to_now_and_apply(&mut self) {
        use chrono::{Datelike, Timelike};
        let now = Local::now();
        let d = now.date_naive();
        let t = now.time();
        self.fake_year = d.year();
        self.fake_month = d.month();
        self.fake_day = d.day();
        self.fake_hour = t.hour();
        self.fake_minute = t.minute();
        self.fake_second = t.second();
        self.apply_fake_time();
    }

    fn ui_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Mock Time");
            ui.separator();

            ui.label("Date:");
            ui.add(egui::DragValue::new(&mut self.fake_year).range(MIN_YEAR..=MAX_YEAR));
            ui.label("-");
            ui.add(egui::DragValue::new(&mut self.fake_month).range(1..=12));
            ui.label("-");
            // Clamp day to the picked month's max so leap-year/short-month edits
            // don't roll over silently.
            let max_day = days_in_month(self.fake_year, self.fake_month);
            ui.add(egui::DragValue::new(&mut self.fake_day).range(1..=max_day));

            ui.label("Time:");
            ui.add(egui::DragValue::new(&mut self.fake_hour).range(0..=23));
            ui.label(":");
            ui.add(egui::DragValue::new(&mut self.fake_minute).range(0..=59));
            ui.label(":");
            ui.add(egui::DragValue::new(&mut self.fake_second).range(0..=59));

            if ui.button("Now").clicked() {
                self.reset_to_now_and_apply();
            }
            if ui.button("Set").clicked() {
                self.apply_fake_time();
            }
            ui.separator();
            let delta_secs = self.current_delta_ticks as f64 / 10_000_000.0;
            ui.label(format!("Δ = {delta_secs:+.1}s"));
        });

        if let Some(msg) = &self.status_msg {
            ui.colored_label(egui::Color32::YELLOW, msg);
        }
    }

    fn ui_processes(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.add(egui::TextEdit::singleline(&mut self.search).desired_width(240.0));
            if ui.button("⟳ Refresh").clicked() {
                self.watcher.refresh();
                self.processes = self.watcher.list();
                self.processes
                    .sort_by_key(|a| a.name.to_lowercase());
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
            .filter(|p| {
                needle.is_empty()
                    || p.name.to_lowercase().contains(&needle)
                    || p.path.to_lowercase().contains(&needle)
            })
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
                        let resp =
                            ui.add_enabled(manager_ready, egui::Checkbox::new(&mut checked, ""));
                        if resp.changed() {
                            if let Some(m) = self.manager.as_mut() {
                                if checked {
                                    if let Err(e) =
                                        m.inject(p.pid, &p.name, &p.path, self.current_delta_ticks)
                                    {
                                        self.status_msg = Some(format!("inject failed: {e}"));
                                    }
                                } else {
                                    m.disable(p.pid);
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

/// Convert Unix microseconds to FILETIME ticks (100-ns since 1601-01-01 UTC).
/// Returns `None` if either arithmetic step overflows.
#[inline]
pub(crate) fn unix_micros_to_filetime_ticks(unix_micros: i64) -> Option<i64> {
    unix_micros
        .checked_mul(10)
        .and_then(|v| UNIX_TO_FILETIME_TICKS.checked_add(v))
}

pub(crate) fn days_in_month(year: i32, month: u32) -> u32 {
    // Compute via chrono so leap years are correct.
    let next_month = if month == 12 { 1 } else { month + 1 };
    let next_year = if month == 12 { year + 1 } else { year };
    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| {
            use chrono::Datelike;
            d.day()
        })
        .unwrap_or(31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_micros_to_filetime_ticks_zero() {
        let result = unix_micros_to_filetime_ticks(0);
        assert_eq!(
            result,
            Some(UNIX_TO_FILETIME_TICKS),
            "zero unix micros should map to UNIX_TO_FILETIME_TICKS"
        );
    }

    #[test]
    fn unix_micros_to_filetime_ticks_positive() {
        // 1 second = 1_000_000 microseconds
        // In FILETIME ticks: 1_000_000 * 10 = 10_000_000 ticks
        let one_sec_micros = 1_000_000;
        let result = unix_micros_to_filetime_ticks(one_sec_micros);
        assert!(result.is_some());
        let ticks = result.unwrap();
        assert_eq!(
            ticks,
            UNIX_TO_FILETIME_TICKS + 10_000_000,
            "one second should add 10M ticks"
        );
    }

    #[test]
    fn unix_micros_to_filetime_ticks_large_valid() {
        // Year 2100 in Unix micros: roughly 4_102_444_800 seconds = 4_102_444_800_000_000 micros
        let year_2100_approx = 4_102_444_800_000_000i64;
        let result = unix_micros_to_filetime_ticks(year_2100_approx);
        assert!(result.is_some(), "year 2100 should not overflow");
    }

    #[test]
    fn unix_micros_to_filetime_ticks_overflow_on_mul() {
        // i64::MAX / 10 ≈ 9.2e17
        // Multiplying anything larger by 10 will overflow
        let overflow_input = i64::MAX;
        let result = unix_micros_to_filetime_ticks(overflow_input);
        assert!(
            result.is_none(),
            "i64::MAX should overflow when multiplied by 10"
        );
    }

    #[test]
    fn unix_micros_to_filetime_ticks_overflow_on_add() {
        // Create a value that, when multiplied by 10, still fits i64
        // but adding UNIX_TO_FILETIME_TICKS causes overflow
        // UNIX_TO_FILETIME_TICKS is ~1.16e17, i64::MAX is ~9.2e18
        // So we need a very large multiplied value
        let _near_max = i64::MAX / 10 - 1; // safe for mul by 10
        // This should be OK since (v*10) + offset ≤ i64::MAX
        let v = (i64::MAX - UNIX_TO_FILETIME_TICKS + 1) / 10;
        let result = unix_micros_to_filetime_ticks(v);
        assert!(result.is_some());
    }

    #[test]
    fn days_in_month_feb_leap_2020() {
        assert_eq!(days_in_month(2020, 2), 29, "February 2020 is a leap year");
    }

    #[test]
    fn days_in_month_feb_non_leap_2021() {
        assert_eq!(days_in_month(2021, 2), 28, "February 2021 is not a leap year");
    }

    #[test]
    fn days_in_month_feb_non_leap_1900() {
        // 1900 is divisible by 100 but not 400, so not a leap year
        assert_eq!(days_in_month(1900, 2), 28, "February 1900 is not a leap year");
    }

    #[test]
    fn days_in_month_feb_leap_2000() {
        // 2000 is divisible by 400, so it is a leap year
        assert_eq!(days_in_month(2000, 2), 29, "February 2000 is a leap year");
    }

    #[test]
    fn days_in_month_apr() {
        assert_eq!(days_in_month(2024, 4), 30, "April has 30 days");
    }

    #[test]
    fn days_in_month_dec() {
        assert_eq!(days_in_month(2024, 12), 31, "December has 31 days");
    }

    #[test]
    fn days_in_month_jan() {
        assert_eq!(days_in_month(2024, 1), 31, "January has 31 days");
    }

    #[test]
    fn days_in_month_jun() {
        assert_eq!(days_in_month(2024, 6), 30, "June has 30 days");
    }

    #[test]
    fn days_in_month_rollover_dec_to_jan() {
        // When month=12, the function computes next_month=1, next_year=year+1
        // It should still return 31 for December
        assert_eq!(days_in_month(2024, 12), 31);
    }

    #[test]
    fn days_in_month_consistent_with_chrono() {
        use chrono::Datelike;
        for year in [1970, 2000, 2020, 2024, 2025, 2100] {
            for month in 1..=12 {
                let result = days_in_month(year, month);
                // Verify with chrono
                let next_month = if month == 12 { 1 } else { month + 1 };
                let next_year = if month == 12 { year + 1 } else { year };
                if let Some(first_of_next) = NaiveDate::from_ymd_opt(next_year, next_month, 1) {
                    if let Some(last_of_month) = first_of_next.pred_opt() {
                        let expected = last_of_month.day();
                        assert_eq!(
                            result, expected,
                            "days_in_month({}, {}) mismatch",
                            year, month
                        );
                    }
                }
            }
        }
    }
}
