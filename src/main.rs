#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use desktop_countdown::{logging, paths, single_instance::SingleInstance};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--settings") {
        // The settings window is a plain GUI process: no DPI-per-monitor setup,
        // no renderer mutex, no WorkerW. It only edits config.toml.
        let _guard = logging::init(&paths::log_dir()?);
        return desktop_countdown::settings::run();
    }

    // Must happen before any window or monitor query.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)? };

    let _guard = logging::init(&paths::log_dir()?);
    let _instance = SingleInstance::acquire()?;

    let cfg_path = paths::config_path()?;
    tracing::info!(?cfg_path, "starting");

    if let Err(e) = desktop_countdown::app::App::run(cfg_path) {
        tracing::error!("fatal: {e:#}");
        return Err(e);
    }
    Ok(())
}
