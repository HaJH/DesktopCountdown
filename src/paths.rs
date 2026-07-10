//! Filesystem locations. Uses the standard Windows environment variables.

use std::path::PathBuf;

use anyhow::{anyhow, Result};

const APP_DIR: &str = "DesktopCountdown";

pub fn config_path() -> Result<PathBuf> {
    let base = std::env::var_os("APPDATA").ok_or_else(|| anyhow!("APPDATA is not set"))?;
    let mut p = PathBuf::from(base);
    p.push(APP_DIR);
    std::fs::create_dir_all(&p)?;
    p.push("config.toml");
    Ok(p)
}

pub fn log_dir() -> Result<PathBuf> {
    let base =
        std::env::var_os("LOCALAPPDATA").ok_or_else(|| anyhow!("LOCALAPPDATA is not set"))?;
    let mut p = PathBuf::from(base);
    p.push(APP_DIR);
    std::fs::create_dir_all(&p)?;
    Ok(p)
}
