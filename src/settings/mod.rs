//! The egui settings window, launched via `desktop-countdown.exe --settings`.
//! It edits config.toml; the renderer watches the file and applies changes.

pub mod app;
pub mod overrides;
pub mod widgets;

use anyhow::{bail, Result};
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

use app::SettingsApp;

/// Named-mutex single instance guard for the settings window, scoped to a
/// different name from the renderer's `single_instance::SingleInstance` (`Local\
/// DesktopCountdown`) so the two processes never contend with each other. Mirrors
/// `crate::single_instance::SingleInstance`'s structure exactly; kept local here
/// (rather than generalizing the shared module to take a name) since only this
/// module needs a second mutex.
struct SettingsInstance(HANDLE);

impl SettingsInstance {
    /// Returns `Err` if another settings window already holds the mutex.
    fn acquire() -> Result<Self> {
        unsafe {
            let handle = CreateMutexW(None, true, w!("Local\\DesktopCountdown-Settings"))?;
            if windows::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                bail!("settings window already open");
            }
            Ok(Self(handle))
        }
    }
}

impl Drop for SettingsInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Opens the settings window and blocks until it is closed. Exits quietly (without
/// opening a window) if a settings window is already running -- bringing the
/// existing window forward is a non-goal.
pub fn run() -> Result<()> {
    let _instance = match SettingsInstance::acquire() {
        Ok(g) => g,
        Err(_) => {
            tracing::info!("settings window already open, exiting");
            return Ok(());
        }
    };

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
