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
        Box::new(|cc| {
            install_korean_font(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::new()?))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe run failed: {e}"))?;
    Ok(())
}

/// Windows fonts that cover Hangul, in preference order. Malgun Gothic ships on Vista+.
const KOREAN_FONTS: [&str; 3] = [
    r"C:\Windows\Fonts\malgun.ttf",
    r"C:\Windows\Fonts\gulim.ttc",
    r"C:\Windows\Fonts\batang.ttc",
];

/// Reads the first available Korean font's bytes. egui's bundled fonts contain no CJK glyphs, so
/// without installing one, every Hangul label in this UI renders as tofu (□) — the egui docs say
/// asian characters require `Context::set_fonts` with your own font.
fn korean_font_data() -> Option<Vec<u8>> {
    for path in KOREAN_FONTS {
        match std::fs::read(path) {
            Ok(data) if !data.is_empty() => return Some(data),
            _ => continue,
        }
    }
    None
}

/// Installs a Korean font as a fallback in egui's font families. Latin text still uses egui's
/// default font first; Hangul glyphs (absent there) fall through to this font.
fn install_korean_font(ctx: &eframe::egui::Context) {
    use eframe::egui::{FontData, FontDefinitions, FontFamily};

    let Some(data) = korean_font_data() else {
        tracing::warn!(
            "no Korean font found under C:\\Windows\\Fonts; Hangul UI will show as tofu"
        );
        return;
    };
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "korean".to_owned(),
        std::sync::Arc::new(FontData::from_owned(data)),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("korean".to_owned());
    }
    ctx.set_fonts(fonts);
    tracing::info!("Korean font installed for the settings UI");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_korean_font_is_available_on_this_system() {
        // Windows ships Malgun Gothic; without a Korean font the settings UI is unreadable.
        let data = korean_font_data();
        assert!(
            data.is_some(),
            "no Korean font found under C:\\Windows\\Fonts"
        );
        assert!(!data.unwrap().is_empty(), "Korean font file was empty");
    }
}
