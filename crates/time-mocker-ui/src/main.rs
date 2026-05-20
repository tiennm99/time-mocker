//! Controller UI for time-mocker.
//!
//! Modules:
//! - `injection_manager`: dll-syringe-backed injector + per-PID `SharedDelta`
//! - `process_watcher`: poll-based auto-inject scanner
//! - `rules`: glob / regex pattern matcher
//! - `app`: eframe `App` impl, tabs, persistent settings

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod injection_manager;
mod process_watcher;
mod rules;
mod win32_process_info;

use anyhow::Result;

fn main() -> Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([640.0, 400.0])
            .with_title("TimeMocker"),
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "TimeMocker",
        native_options,
        Box::new(|cc| Ok(Box::new(app::TimeMockerApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}
