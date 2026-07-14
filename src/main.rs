#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use anyhow::Result;
use desktop_countdown::app::AppCore;
use desktop_countdown::platform::{self, SingleInstance};
use desktop_countdown::{logging, paths, settings};

/// Distinct names so the renderer and the settings window never contend with each other.
const RENDERER_INSTANCE: &str = "DesktopCountdown";
const SETTINGS_INSTANCE: &str = "DesktopCountdown-Settings";

fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--settings") {
        // The settings window is a plain GUI process: no per-monitor DPI setup, no
        // renderer lock, no wallpaper surface. It only edits config.toml.
        let _guard = logging::init(&paths::log_dir()?);
        return settings::run(SETTINGS_INSTANCE);
    }

    // Must happen before any window or monitor query.
    platform::init()?;

    let _guard = logging::init(&paths::log_dir()?);
    let Some(_instance) = SingleInstance::acquire(RENDERER_INSTANCE)? else {
        // A second launch opens the settings window instead of dying on the lock.
        // It is how a user who cannot reach the tray / menu bar icon -- macOS hides
        // overflowed menu bar items outright -- still gets to the app.
        //
        // Spawned detached, like any settings window the user starts by hand: the
        // running renderer does not know about it, so quitting from the tray leaves
        // it open (see `AppCore::open_settings` for the tray-owned counterpart).
        tracing::info!("already running; opening the settings window instead");
        let exe = std::env::current_exe()?;
        std::process::Command::new(exe).arg("--settings").spawn()?;
        return Ok(());
    };

    let cfg_path = paths::config_path()?;
    tracing::info!(?cfg_path, "starting");

    run(cfg_path).inspect_err(|e| tracing::error!("fatal: {e:#}"))
}

/// Split out of `main` so that one `inspect_err` covers *both* startup and the event
/// loop. A release build is `windows_subsystem = "windows"` and has no console, so an
/// `Err` that escapes `main` prints to a stderr nobody is reading -- the log file is the
/// only place a user can ever see why the app died. Failing to build the painter, the
/// compositor, or the tray is exactly the kind of thing that has to end up there.
fn run(cfg_path: std::path::PathBuf) -> Result<()> {
    let core = AppCore::new(cfg_path)?;
    platform::run(core)
}
