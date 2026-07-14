//! The egui settings window, launched via `desktop-countdown.exe --settings`.
//! It edits config.toml; the renderer watches the file and applies changes.

pub mod app;
pub mod lines;
pub mod overrides;
pub mod widgets;

use anyhow::Result;

use crate::platform::SingleInstance;
use app::SettingsApp;

/// Opens the settings window and blocks until it is closed. Exits quietly (without
/// opening a window) if a settings window is already running -- bringing the
/// existing window forward is a non-goal.
///
/// `instance_name` is the renderer's lock under a different name, so the two processes
/// never contend with each other.
pub fn run(instance_name: &str) -> Result<()> {
    let _instance = match SingleInstance::acquire(instance_name) {
        Ok(g) => g,
        Err(_) => {
            tracing::info!("settings window already open, exiting");
            return Ok(());
        }
    };

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([720.0, 560.0])
        .with_title("DesktopCountdown 설정");
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let native_options = eframe::NativeOptions {
        viewport,
        event_loop_builder: activation_policy_hook(),
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

/// Forces the settings process to be an ordinary, front-facing app on macOS.
///
/// The renderer runs as an *accessory*: no Dock tile, no Cmd-Tab entry, just a menu bar
/// item. The shipped `.app` says so in its `Info.plist` with `LSUIElement`, and both
/// processes -- the renderer and this one -- come out of the same bundle, so both inherit it.
///
/// winit honours the bundle's manifest unless told otherwise, which would leave the settings
/// window unable to come to the front: the user picks "설정 열기" from the menu, a window
/// appears somewhere behind everything, and nothing brings it forward. Overriding the policy
/// here is what makes the menu item do what it says. (`with_activate_ignoring_other_apps`
/// then puts it in front of whatever the user was looking at, which is the point of having
/// asked for it.)
///
/// Windows has no equivalent and needs none: the settings process there is an ordinary GUI
/// process already.
fn activation_policy_hook() -> Option<eframe::EventLoopBuilderHook> {
    #[cfg(target_os = "macos")]
    {
        Some(Box::new(|builder| {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            builder
                .with_activation_policy(ActivationPolicy::Regular)
                .with_activate_ignoring_other_apps(true);
        }))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// The icon for the settings window's title bar and taskbar button.
///
/// Windows-only in effect: winit has no window icon on macOS (the Dock tile comes from the
/// bundle's `.icns`), so this is decoded and handed over there for nothing -- a few hundred
/// microseconds at startup, against a `#[cfg]` split of both the call site and this function.
///
/// The asset is the same `assets/icon.ico` that `build.rs` compiles into the exe's icon
/// resource and that `platform::windows::tray` loads back out of it. It is compiled in a
/// second time here, as bytes rather than as a resource, because egui takes raw RGBA pixels
/// and not a Windows icon handle. `image`'s ICO decoder picks the file's largest entry
/// (256x256), which the window manager scales down for the title bar.
///
/// A file that fails to decode yields an iconless window, not a failed launch.
fn window_icon() -> Option<eframe::egui::IconData> {
    const ICON_BYTES: &[u8] = include_bytes!("../../assets/icon.ico");

    match image::load_from_memory_with_format(ICON_BYTES, image::ImageFormat::Ico) {
        Ok(image) => {
            let image = image.into_rgba8();
            let (width, height) = image.dimensions();
            Some(eframe::egui::IconData {
                rgba: image.into_raw(),
                width,
                height,
            })
        }
        Err(e) => {
            tracing::warn!("could not decode the window icon: {e}");
            None
        }
    }
}

/// English family names that resolve their Korean/Japanese/simplified-Chinese
/// counterparts via `platform::fonts::font_file`, tried in this order. Preferred over the
/// CJK names themselves (e.g. "맑은 고딕") since those are non-Latin and therefore a
/// fragile literal match against however the OS happens to report them; the English names
/// are stable across locales.
///
/// The two systems ship different fonts, so the list is per-platform -- but what it is
/// *for* is not: whatever the user types into the font picker's search box has to render.
#[cfg(windows)]
const CJK_FALLBACK_FAMILIES: [(&str, &str); 3] = [
    ("cjk_ko", "Malgun Gothic"),
    ("cjk_ja", "MS Gothic"),
    ("cjk_zh", "Microsoft YaHei"),
];

#[cfg(target_os = "macos")]
const CJK_FALLBACK_FAMILIES: [(&str, &str); 3] = [
    ("cjk_ko", "Apple SD Gothic Neo"),
    ("cjk_ja", "Hiragino Sans"),
    ("cjk_zh", "PingFang SC"),
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
        let Some(file) = crate::platform::fonts::font_file(family_name) else {
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
        // Against the real list, not a copy of it: the point is that whatever this
        // platform's list names is actually installed on this platform.
        let any = super::CJK_FALLBACK_FAMILIES
            .iter()
            .any(|(_, family)| crate::platform::fonts::font_file(family).is_some());
        assert!(
            any,
            "none of {:?} resolved; the CJK fallback list is wrong for this platform",
            super::CJK_FALLBACK_FAMILIES.map(|(_, f)| f)
        );
    }
}
