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
            install_cjk_fallback(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::new()?))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe run failed: {e}"))?;
    Ok(())
}

/// English family names that resolve their Korean/Japanese/simplified-Chinese
/// counterparts via DirectWrite (`fonts::font_file`), tried in this order. Preferred
/// over the CJK names themselves (e.g. "맑은 고딕") since those are non-Latin and
/// therefore a fragile literal match against however the OS happens to report them;
/// the English names are stable across locales.
const CJK_FALLBACK_FAMILIES: [(&str, &str); 3] = [
    ("cjk_ko", "Malgun Gothic"),
    ("cjk_ja", "MS Gothic"),
    ("cjk_zh", "Microsoft YaHei"),
];

/// Installs Korean/Japanese/Chinese fonts as LOW-priority fallbacks for all UI text.
///
/// This is unrelated to the per-font-name rendering in `settings::app` (which binds one
/// specific family to `FontFamily::Name` at `Highest` priority so a font *picker row*
/// renders in its own font). This fallback instead covers arbitrary CJK the user
/// *types* -- e.g. searching the font picker for "고딕" -- which the per-name mechanism
/// doesn't reach, since free-form text isn't tied to any known family. The fonts here
/// are pushed (not inserted first) onto the Proportional/Monospace fallback chains, so
/// egui's bundled Latin font is still tried first; only glyphs missing there (CJK) fall
/// through to these.
///
/// `ctx.set_fonts` replaces the whole font definition set, but doing that here is safe:
/// this runs once, in eframe's app-creation closure, before the first frame is drawn.
/// `app::FontRegistry::ensure` only calls `ctx.add_font` -- which *adds* to whatever
/// `set_fonts` last installed -- and only during later frames, so the per-name fonts
/// layer on top of this CJK-fallback base instead of being wiped by it.
fn install_cjk_fallback(ctx: &eframe::egui::Context) {
    use eframe::egui::{FontData, FontDefinitions, FontFamily};

    let mut fonts = FontDefinitions::default();
    let mut installed = 0u32;

    for (key, family_name) in CJK_FALLBACK_FAMILIES {
        // `fonts::font_file` already validates the bytes with skrifa before returning
        // (see its doc comment), so a font that would panic epaint's parser never
        // reaches `font_data` below -- a missing or unparseable font is just skipped.
        let Some(file) = crate::fonts::font_file(family_name) else {
            tracing::warn!("CJK fallback font '{family_name}' not found, skipping");
            continue;
        };
        let mut data = FontData::from_owned(file.bytes);
        data.index = file.index;
        fonts
            .font_data
            .insert(key.to_owned(), std::sync::Arc::new(data));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push(key.to_owned());
        }
        installed += 1;
    }

    ctx.set_fonts(fonts);
    tracing::info!("installed {installed} CJK fallback fonts");
}

#[cfg(test)]
mod tests {
    #[test]
    fn at_least_one_cjk_fallback_font_resolves() {
        // On Windows, at least Malgun Gothic should resolve for the CJK fallback.
        let any = ["Malgun Gothic", "MS Gothic", "Microsoft YaHei"]
            .iter()
            .any(|n| crate::fonts::font_file(n).is_some());
        assert!(any, "no CJK fallback font resolved");
    }
}
