//! Loading and saving `config.toml`.

use std::path::Path;

use anyhow::{Context, Result};

use super::{validate, Config};

/// Reads the config, creating it with defaults if it does not exist.
/// Returns `Err` on malformed TOML or values outside their allowed range;
/// callers keep their previous config in that case.
pub fn load_or_create(path: &Path) -> Result<Config> {
    if !path.exists() {
        let cfg = Config::default();
        save(path, &cfg)?;
        return Ok(cfg);
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let cfg: Config =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    validate(&cfg)?;
    Ok(cfg)
}

pub fn save(path: &Path, cfg: &Config) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string_pretty(cfg)?;
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("dc-test-{name}"));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p.push("config.toml");
        p
    }

    #[test]
    fn creates_default_file_when_missing() {
        let p = tmp("create");
        assert!(!p.exists());
        let cfg = load_or_create(&p).unwrap();
        assert!(p.exists());
        assert_eq!(cfg, Config::default());
        // The written file must parse back to the same config.
        let text = fs::read_to_string(&p).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn reads_existing_file() {
        let p = tmp("read");
        fs::write(
            &p,
            "target = \"2030-01-01T00:00:00\"\n[style]\nsize_px = 99.0\n",
        )
        .unwrap();
        let cfg = load_or_create(&p).unwrap();
        assert_eq!(cfg.style.size_px, 99.0);
    }

    #[test]
    fn rejects_invalid_values() {
        let p = tmp("invalid");
        fs::write(
            &p,
            "target = \"2030-01-01T00:00:00\"\n[style]\nopacity = 3.0\n",
        )
        .unwrap();
        assert!(load_or_create(&p).is_err());
    }

    #[test]
    fn rejects_malformed_toml() {
        let p = tmp("malformed");
        fs::write(&p, "target = \"2030-01-01T00:00:00\"\n[style\n").unwrap();
        assert!(load_or_create(&p).is_err());
    }
}
