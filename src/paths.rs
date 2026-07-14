//! Filesystem locations, each platform in its own convention.

use std::path::PathBuf;

use anyhow::{anyhow, Result};

const APP_DIR: &str = "DesktopCountdown";

/// `%APPDATA%\DesktopCountdown\config.toml`, or
/// `~/Library/Application Support/DesktopCountdown/config.toml`.
pub fn config_path() -> Result<PathBuf> {
    let mut p = config_dir()?;
    std::fs::create_dir_all(&p)?;
    p.push("config.toml");
    Ok(p)
}

/// `presets.toml`, next to `config.toml`. Only the settings window touches it -- the renderer
/// watches `config.toml` and knows nothing about presets, so saving one does not make the
/// wallpaper redraw.
pub fn presets_path() -> Result<PathBuf> {
    let mut p = config_dir()?;
    std::fs::create_dir_all(&p)?;
    p.push("presets.toml");
    Ok(p)
}

/// `%LOCALAPPDATA%\DesktopCountdown\`, or `~/Library/Logs/DesktopCountdown/`.
pub fn log_dir() -> Result<PathBuf> {
    let p = log_dir_path()?;
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

#[cfg(windows)]
fn config_dir() -> Result<PathBuf> {
    app_dir_under("APPDATA")
}

#[cfg(windows)]
fn log_dir_path() -> Result<PathBuf> {
    app_dir_under("LOCALAPPDATA")
}

/// The roaming (`APPDATA`) and local (`LOCALAPPDATA`) profiles both hold the app's data
/// under a directory of its own name.
#[cfg(windows)]
fn app_dir_under(var: &str) -> Result<PathBuf> {
    let base = std::env::var_os(var).ok_or_else(|| anyhow!("{var} is not set"))?;
    let mut p = PathBuf::from(base);
    p.push(APP_DIR);
    Ok(p)
}

#[cfg(target_os = "macos")]
fn config_dir() -> Result<PathBuf> {
    app_dir_under("Library/Application Support")
}

#[cfg(target_os = "macos")]
fn log_dir_path() -> Result<PathBuf> {
    app_dir_under("Library/Logs")
}

#[cfg(target_os = "macos")]
fn app_dir_under(subdir: &str) -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    let mut p = PathBuf::from(home);
    p.push(subdir);
    p.push(APP_DIR);
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_file_sits_in_the_app_directory() {
        let p = config_path().unwrap();
        assert_eq!(p.file_name().unwrap(), "config.toml");
        assert_eq!(p.parent().unwrap().file_name().unwrap(), APP_DIR);
    }

    /// Logs deliberately do not live next to the config: Windows keeps them out of the
    /// roaming profile, macOS out of Application Support.
    #[test]
    fn logs_do_not_live_next_to_the_config() {
        let logs = log_dir().unwrap();
        let cfg_dir = config_path().unwrap().parent().unwrap().to_path_buf();
        assert_eq!(logs.file_name().unwrap(), APP_DIR);
        assert_ne!(logs, cfg_dir);
    }

    #[test]
    fn the_presets_file_sits_next_to_the_config() {
        let presets = presets_path().unwrap();
        let cfg = config_path().unwrap();
        assert_eq!(presets.file_name().unwrap(), "presets.toml");
        assert_eq!(presets.parent(), cfg.parent());
    }
}
