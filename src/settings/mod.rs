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

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ui, |ui| {
            ui.label("설정 창 (UI 구현 예정)");
        });
        let now = self.now_ms();
        self.save_if_due(now);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(200));
    }
}
