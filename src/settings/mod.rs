//! The egui settings window, launched via `desktop-countdown.exe --settings`.
//! It edits config.toml; the renderer watches the file and applies changes.

pub mod app;
pub mod overrides;
pub mod widgets;

use anyhow::Result;

use app::SettingsApp;

/// Opens the settings window and blocks until it is closed.
pub fn run() -> Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([720.0, 560.0])
            .with_title("DesktopCountdown 설정"),
        ..Default::default()
    };
    eframe::run_native(
        "DesktopCountdown 설정",
        native_options,
        Box::new(|_cc| Ok(Box::new(SettingsApp::new()?))),
    )
    .map_err(|e| anyhow::anyhow!("eframe run failed: {e}"))?;
    Ok(())
}
