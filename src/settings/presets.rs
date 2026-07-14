//! The preset model: a named snapshot of the whole look (lines + style), the built-in list,
//! and the library the settings window picks from. Pure logic, no egui, no I/O.

use crate::config::{
    Line, Style, DEFAULT_PRESET, MAIN_TEMPLATE, SUMMARY_SIZE_RATIO, SUMMARY_TEMPLATE,
};

/// A named snapshot of the whole look: the line list *and* the shared style. Picking a preset
/// replaces both -- so a preset is a look, not just a layout, and switching one away discards
/// any unsaved tweaks on top of it.
///
/// Field order here (`name`, `style`, `lines`) matches the order the resulting `presets.toml`
/// reads most naturally in -- the name up front, then what it looks like. `toml_edit`'s pretty
/// printer reorders scalars ahead of tables regardless of declaration order, so nothing here
/// depends on this order for correctness.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub style: Style,
    #[serde(default, rename = "line")]
    pub lines: Vec<Line>,
}

pub const BUILTIN_COUNT: usize = 5;

/// The presets that ship with the app. `Clock only` is first: it is what a fresh config holds
/// (`config::DEFAULT_PRESET`), and the picker shows the list in this order.
///
/// All five carry `Style::default()`. That makes picking one a way back to the stock look as
/// well as to a layout -- a recovery point, not a stray side effect.
pub fn builtin() -> Vec<Preset> {
    fn preset(name: &str, lines: &[(&str, f32)]) -> Preset {
        Preset {
            name: name.to_string(),
            style: Style::default(),
            lines: lines
                .iter()
                .map(|(text, size_ratio)| Line {
                    text: (*text).to_string(),
                    size_ratio: *size_ratio,
                    ..Line::default()
                })
                .collect(),
        }
    }

    vec![
        preset(DEFAULT_PRESET, &[(MAIN_TEMPLATE, 1.0)]),
        preset(
            "Summary + Clock",
            &[(SUMMARY_TEMPLATE, SUMMARY_SIZE_RATIO), (MAIN_TEMPLATE, 1.0)],
        ),
        preset("D-Day", &[("D-{daysTotal}", 1.0), (MAIN_TEMPLATE, 0.3)]),
        preset(
            "Days left",
            &[("{daysTotal} days left", 0.35), (MAIN_TEMPLATE, 1.0)],
        ),
        preset(
            "Caption + Clock",
            &[
                ("Deadline", 0.25),
                (MAIN_TEMPLATE, 1.0),
                ("{daysTotal} days left", 0.25),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_only_comes_first() {
        assert_eq!(builtin()[0].name, "Clock only");
    }

    /// `Config::default` labels itself `DEFAULT_PRESET`, and the settings window resolves
    /// that label against this list. If the two drift apart, a fresh install opens on
    /// `Custom` -- which is exactly the confusing state the label exists to prevent.
    #[test]
    fn the_default_config_matches_its_own_label() {
        let cfg = crate::config::Config::default();
        let labelled = builtin()
            .into_iter()
            .find(|p| p.name == DEFAULT_PRESET)
            .expect("DEFAULT_PRESET names a built-in preset");
        assert_eq!(labelled.lines, cfg.lines);
        assert_eq!(labelled.style, cfg.style);
    }

    #[test]
    fn there_is_no_preset_called_classic() {
        assert!(
            !builtin().iter().any(|p| p.name == "Classic"),
            "Classic was renamed to Summary + Clock"
        );
    }

    #[test]
    fn summary_plus_clock_is_the_old_classic_pair() {
        let p = builtin()
            .into_iter()
            .find(|p| p.name == "Summary + Clock")
            .expect("Summary + Clock preset");
        assert_eq!(p.lines.len(), 2);
        assert_eq!(p.lines[0].text, SUMMARY_TEMPLATE);
        assert_eq!(p.lines[0].size_ratio, SUMMARY_SIZE_RATIO);
        assert_eq!(p.lines[1].text, MAIN_TEMPLATE);
        assert_eq!(p.lines[1].size_ratio, 1.0);
    }

    #[test]
    fn every_builtin_carries_the_default_style_and_at_least_one_line() {
        for p in builtin() {
            assert!(!p.lines.is_empty(), "{} built nothing", p.name);
            assert_eq!(
                p.style,
                Style::default(),
                "{} is not on the default style",
                p.name
            );
            for line in &p.lines {
                assert!(line.size_ratio > 0.0);
            }
        }
    }

    #[test]
    fn builtin_names_are_unique() {
        let mut names: Vec<String> = builtin().into_iter().map(|p| p.name).collect();
        names.sort();
        let n = names.len();
        names.dedup();
        assert_eq!(names.len(), n, "duplicate built-in preset name");
    }

    #[test]
    fn builtin_count_matches_the_list() {
        assert_eq!(builtin().len(), BUILTIN_COUNT);
    }
}
