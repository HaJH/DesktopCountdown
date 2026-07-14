//! Loading and saving `presets.toml`, the user's own preset library.
//!
//! Deliberately not `config.toml`: the renderer watches that file and redraws when it changes.
//! Saving or deleting a preset changes nothing on the wallpaper, so it must not land in a file
//! that would make the renderer reload. Nothing outside the settings window reads this one.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{self, Config};
use crate::settings::presets::Preset;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default, rename = "preset")]
    presets: Vec<Preset>,
}

/// Reads the user's presets. A file that is missing, unreadable, or malformed is not an error:
/// the settings window opens with the built-ins alone rather than refusing to start over a
/// broken preset library. The same goes for a single preset that would not validate -- it is
/// dropped and the rest load.
pub fn load(path: &Path) -> Vec<Preset> {
    if !path.exists() {
        return Vec::new();
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("could not read {}: {e}", path.display());
            return Vec::new();
        }
    };
    let file: File = match toml::from_str(&text) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("could not parse {}: {e}", path.display());
            return Vec::new();
        }
    };
    file.presets
        .into_iter()
        .filter(|p| match validates(p) {
            true => true,
            false => {
                tracing::warn!("dropping preset '{}': it does not validate", p.name);
                false
            }
        })
        .collect()
}

/// Whether a preset's look would survive `config::validate` -- the same gate the settings
/// window's own writes go through.
fn validates(p: &Preset) -> bool {
    let probe = Config {
        style: p.style.clone(),
        lines: p.lines.clone(),
        ..Config::default()
    };
    config::validate(&probe).is_ok()
}

/// Writes the library atomically, for the same reason `config::save` does: a plain write
/// truncates before re-filling, and a reader landing in that window would see an empty file.
pub fn save(path: &Path, user: &[Preset]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = File {
        presets: user.to_vec(),
    };
    let text = toml::to_string_pretty(&file)?;

    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("replacing {} with {}", path.display(), tmp.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Line, Style};
    use std::fs;

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("dc-presets-test-{name}"));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p.push("presets.toml");
        p
    }

    fn sample() -> Preset {
        Preset {
            name: "My look".to_string(),
            style: Style {
                size_px: 96.0,
                opacity: 0.5,
                ..Style::default()
            },
            lines: vec![
                Line {
                    text: "Deadline".to_string(),
                    size_ratio: 0.25,
                    ..Line::default()
                },
                Line {
                    text: "{hh}:{mm}:{ss}".to_string(),
                    ..Line::default()
                },
            ],
        }
    }

    #[test]
    fn a_missing_file_loads_as_an_empty_library() {
        let p = tmp("missing");
        assert!(!p.exists());
        assert!(load(&p).is_empty());
        assert!(!p.exists(), "loading must not create the file");
    }

    #[test]
    fn presets_round_trip_through_the_file() {
        let p = tmp("round-trip");
        save(&p, &[sample()]).unwrap();
        assert_eq!(load(&p), vec![sample()]);
    }

    #[test]
    fn saving_an_empty_library_leaves_a_file_that_loads_as_empty() {
        let p = tmp("empty");
        save(&p, &[]).unwrap();
        assert!(load(&p).is_empty());
    }

    #[test]
    fn a_malformed_file_loads_as_an_empty_library_rather_than_failing() {
        let p = tmp("malformed");
        fs::write(&p, "[[preset\nname = \"broken\"\n").unwrap();
        assert!(load(&p).is_empty());
    }

    /// A hand-edited preset with a value the renderer would refuse is dropped, not applied:
    /// applying it would leave the settings window stuck on "Invalid config" with no way back.
    #[test]
    fn a_preset_that_would_not_validate_is_dropped() {
        let p = tmp("invalid");
        let mut bad = sample();
        bad.name = "Bad".to_string();
        bad.style.opacity = 3.0;
        save(&p, &[bad, sample()]).unwrap();

        let loaded = load(&p);
        assert_eq!(loaded, vec![sample()], "the valid one survives");
    }
}
