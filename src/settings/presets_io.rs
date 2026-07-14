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

/// What `load` found: the presets that parsed and validated, and the ones it had to reject.
/// `dropped` is not thrown away here -- the settings window must not delete data it did not
/// understand, so the caller (`Library::add_dropped`) carries it forward into any later
/// rewrite of this file instead of letting it vanish. See `Library::dropped`.
#[derive(Debug, Default, PartialEq)]
pub struct Loaded {
    pub presets: Vec<Preset>,
    pub dropped: Vec<Preset>,
}

/// Reads the user's presets. A file that is missing, unreadable, or malformed is not an error:
/// the settings window opens with the built-ins alone rather than refusing to start over a
/// broken preset library -- there is nothing to preserve when the file could not be understood
/// at all. A single preset that would not validate (e.g. a hand-edited `opacity = 3.0`) is
/// different: it does not load, but it is returned in `dropped` rather than discarded, since it
/// parsed fine and only its values are the problem.
pub fn load(path: &Path) -> Loaded {
    if !path.exists() {
        return Loaded::default();
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("could not read {}: {e}", path.display());
            return Loaded::default();
        }
    };
    let file: File = match toml::from_str(&text) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("could not parse {}: {e}", path.display());
            return Loaded::default();
        }
    };
    let mut presets = Vec::new();
    let mut dropped = Vec::new();
    for p in file.presets {
        if validates(&p) {
            presets.push(p);
        } else {
            tracing::warn!("dropping preset '{}': it does not validate", p.name);
            dropped.push(p);
        }
    }
    Loaded { presets, dropped }
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
        let loaded = load(&p);
        assert!(loaded.presets.is_empty());
        assert!(loaded.dropped.is_empty());
        assert!(!p.exists(), "loading must not create the file");
    }

    #[test]
    fn presets_round_trip_through_the_file() {
        let p = tmp("round-trip");
        save(&p, &[sample()]).unwrap();
        let loaded = load(&p);
        assert_eq!(loaded.presets, vec![sample()]);
        assert!(loaded.dropped.is_empty());
    }

    #[test]
    fn saving_an_empty_library_leaves_a_file_that_loads_as_empty() {
        let p = tmp("empty");
        save(&p, &[]).unwrap();
        assert!(load(&p).presets.is_empty());
    }

    #[test]
    fn a_malformed_file_loads_as_an_empty_library_rather_than_failing() {
        let p = tmp("malformed");
        fs::write(&p, "[[preset\nname = \"broken\"\n").unwrap();
        let loaded = load(&p);
        assert!(loaded.presets.is_empty());
        assert!(loaded.dropped.is_empty());
    }

    /// A hand-edited preset with a value the renderer would refuse is dropped, not applied:
    /// applying it would leave the settings window stuck on "Invalid config" with no way back.
    /// It is not lost, though -- see `Loaded::dropped` and the round-trip test below.
    #[test]
    fn a_preset_that_would_not_validate_is_dropped() {
        let p = tmp("invalid");
        let mut bad = sample();
        bad.name = "Bad".to_string();
        bad.style.opacity = 3.0;
        save(&p, &[bad.clone(), sample()]).unwrap();

        let loaded = load(&p);
        assert_eq!(loaded.presets, vec![sample()], "the valid one survives");
        assert_eq!(
            loaded.dropped,
            vec![bad],
            "the invalid one is not thrown away"
        );
    }

    /// The whole point of `Loaded::dropped`: a preset that fails `config::validate` must
    /// survive being written back out. This is what `Library::dropped` plus
    /// `SettingsApp::persist_presets` (which writes `user()` followed by `dropped()`) rely on
    /// -- a rewrite of `presets.toml` must not erase an entry the settings window could not
    /// use, only entries the user actually removed.
    #[test]
    fn an_invalid_preset_survives_a_load_then_rewrite_then_load_round_trip() {
        let p = tmp("invalid-round-trip");
        let mut bad = sample();
        bad.name = "Bad".to_string();
        bad.style.opacity = 3.0;
        save(&p, &[bad.clone(), sample()]).unwrap();

        let first = load(&p);
        assert_eq!(first.presets, vec![sample()]);
        assert_eq!(first.dropped, vec![bad.clone()]);

        // What `persist_presets` writes: the usable presets followed by the ones the library
        // could not use.
        let mut rewritten = first.presets.clone();
        rewritten.extend(first.dropped.clone());
        save(&p, &rewritten).unwrap();

        let second = load(&p);
        assert_eq!(
            second.presets,
            vec![sample()],
            "still loads after the rewrite"
        );
        assert_eq!(
            second.dropped,
            vec![bad],
            "the invalid preset is still in the file after the rewrite"
        );
    }
}
