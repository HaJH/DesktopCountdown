//! The preset model: a named snapshot of the whole look (lines + style), the built-in list,
//! and the library the settings window picks from. Pure logic, no egui, no I/O.

use crate::config::{
    Config, Line, Style, DEFAULT_PRESET, MAIN_TEMPLATE, SUMMARY_SIZE_RATIO, SUMMARY_TEMPLATE,
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

pub const BUILTIN_COUNT: usize = 6;

/// The presets that ship with the app. `Clock only` is first: it is what a fresh config holds
/// (`config::DEFAULT_PRESET`), and the picker shows the list in this order.
///
/// All six carry `Style::default()`. That makes picking one a way back to the stock look as
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
        preset(
            "Daily countdown",
            &[("{dailySign}{dailyHh}:{dailyMm}:{dailySs}", 1.0)],
        ),
    ]
}

/// Which preset the current look sits on, and whether anything has been changed on top of it.
/// Computed every frame from the config -- never stored, so it cannot go stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Active {
    /// Exactly the preset at this index.
    Clean(usize),
    /// Started from the preset at this index; edits are layered on top.
    Modified(usize),
    /// Matches no preset, and no label points anywhere useful.
    Custom,
}

/// What `Library::check_name` makes of a name typed into the Save-as box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameStatus {
    /// Nothing, or only whitespace.
    Empty,
    /// A built-in's name. Built-ins cannot be overwritten.
    Builtin,
    /// An existing user preset. Saving replaces it.
    Overwrite,
    /// Free.
    New,
}

/// `Style` derives `PartialEq` over every field, including the legacy `show_summary_line`,
/// which only ever holds a value on a `Style` parsed from an old config.toml -- no preset
/// carries it. Comparing it would leave such a config `Modified` against every preset,
/// forever, over a field nothing reads.
pub fn style_eq(a: &Style, b: &Style) -> bool {
    let strip = |s: &Style| Style {
        show_summary_line: None,
        ..s.clone()
    };
    strip(a) == strip(b)
}

/// The presets the picker offers: the built-ins, then the user's own. Index into `all()` is
/// the picker's currency -- `Active` carries one, and so does `apply`.
pub struct Library {
    all: Vec<Preset>,
    n_builtin: usize,
    /// User presets the library could not use: rejected by `config::validate` on load (see
    /// `presets_io::Loaded::dropped`, folded in via `add_dropped`), or dropped right here in
    /// `new` for colliding with an existing name. Never part of `all` -- not pickable, not
    /// resolvable by `find`/`resolve`, not deletable -- but kept around rather than discarded:
    /// the settings window must not delete data it did not understand, and this is what lets
    /// `SettingsApp::persist_presets` write these back out instead of erasing them on the next
    /// save.
    dropped: Vec<Preset>,
}

impl Library {
    /// The user's own list may name-collide with a built-in, or with another user preset --
    /// both possible only via a hand-edited `presets.toml`, since `check_name` blocks either
    /// from the settings window. Either would leave `find` permanently resolving that name to
    /// the earlier entry, so `apply` on the later, shadowed one and `resolve` afterwards would
    /// disagree about which preset is active. The colliding entry is dropped from `all` rather
    /// than kept under a renamed or duplicate label -- there is no rule for what a
    /// machine-picked new name should be -- but it is not thrown away: it lands in `dropped`,
    /// so it survives a rewrite of `presets.toml` even though the picker can never reach it.
    pub fn new(user: Vec<Preset>) -> Self {
        let mut all = builtin();
        let n_builtin = all.len();
        let mut dropped = Vec::new();
        for preset in user {
            if all[..n_builtin].iter().any(|p| p.name == preset.name) {
                tracing::warn!(
                    "dropping user preset '{}': name collides with a built-in",
                    preset.name
                );
                dropped.push(preset);
                continue;
            }
            if all[n_builtin..].iter().any(|p| p.name == preset.name) {
                tracing::warn!("dropping user preset '{}': duplicate name", preset.name);
                dropped.push(preset);
                continue;
            }
            all.push(preset);
        }
        Self {
            all,
            n_builtin,
            dropped,
        }
    }

    pub fn all(&self) -> &[Preset] {
        &self.all
    }

    /// The user's own presets -- what `presets_io::save` writes. The built-ins are not saved.
    pub fn user(&self) -> &[Preset] {
        &self.all[self.n_builtin..]
    }

    /// Presets the library rejected: name collisions dropped by `new`, plus whatever
    /// `add_dropped` folded in from `presets_io::Loaded::dropped`. Not part of `all()` -- not
    /// pickable, not resolvable, not deletable -- so `persist_presets` is what makes this
    /// accessor useful: it writes `user()` followed by `dropped()`, so a rewrite of
    /// `presets.toml` preserves entries the library could not use instead of dropping them a
    /// second time, permanently.
    pub fn dropped(&self) -> &[Preset] {
        &self.dropped
    }

    /// Folds in presets rejected before the library was even built. `presets_io::load`'s
    /// validation gate runs on the raw file contents, before any `Preset` reaches `new`, so its
    /// rejects have to be carried in from outside rather than discovered here. Does not touch
    /// `all`.
    pub fn add_dropped(&mut self, more: Vec<Preset>) {
        self.dropped.extend(more);
    }

    pub fn is_builtin(&self, i: usize) -> bool {
        i < self.n_builtin
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.all.iter().position(|p| p.name == name)
    }

    /// The label is a hint, not the truth. When it names a preset, the current look is compared
    /// against that one and the answer is `Clean` or `Modified`. When it does not (missing from
    /// an old file, or naming a preset since deleted), the look itself is matched against the
    /// whole list -- so a config that happens to be exactly a preset gets its name back rather
    /// than reading `Custom`.
    pub fn resolve(&self, label: Option<&str>, lines: &[Line], style: &Style) -> Active {
        if let Some(i) = label.and_then(|n| self.find(n)) {
            return if self.matches(i, lines, style) {
                Active::Clean(i)
            } else {
                Active::Modified(i)
            };
        }
        match (0..self.all.len()).find(|&i| self.matches(i, lines, style)) {
            Some(i) => Active::Clean(i),
            None => Active::Custom,
        }
    }

    fn matches(&self, i: usize, lines: &[Line], style: &Style) -> bool {
        let p = &self.all[i];
        p.lines == lines && style_eq(&p.style, style)
    }

    /// Drops the preset's whole look onto the config and moves the label to it. Everything the
    /// user had layered on top is gone -- the caller is what guards that (see the settings
    /// window's discard prompt).
    ///
    /// Bounds-checked: `i` is an index handed out by `Active`/`resolve` or held in
    /// `pending_preset`/`SaveAs::then_apply`, and those can outlive a mutation of the library
    /// (e.g. `delete`, which shifts every later index down). An out-of-range index is ignored
    /// rather than indexed unchecked -- this is a GUI process, and a stale index should never
    /// be able to panic it.
    pub fn apply(&self, i: usize, cfg: &mut Config) {
        let Some(p) = self.all.get(i) else {
            tracing::warn!("ignoring apply of stale preset index {i}: out of range");
            return;
        };
        cfg.lines = p.lines.clone();
        cfg.style = p.style.clone();
        cfg.preset = Some(p.name.clone());
    }

    /// What saving under `name` would do. The settings window uses this to label its Save
    /// button and to block the two names it must not take.
    pub fn check_name(&self, name: &str) -> NameStatus {
        let name = name.trim();
        if name.is_empty() {
            return NameStatus::Empty;
        }
        match self.find(name) {
            Some(i) if self.is_builtin(i) => NameStatus::Builtin,
            Some(_) => NameStatus::Overwrite,
            None => NameStatus::New,
        }
    }

    /// Stores the current look under `name` and returns its index in `all()`. An existing user
    /// preset of that name is replaced in place, keeping its slot -- the caller has already
    /// confirmed the overwrite (`NameStatus::Overwrite`).
    ///
    /// Callers must not pass a built-in's name; `check_name` is what rejects it. Doing so
    /// anyway appends a second preset with a duplicate name rather than corrupting a built-in.
    pub fn save_as(&mut self, name: &str, lines: &[Line], style: &Style) -> usize {
        let name = name.trim().to_string();
        let preset = Preset {
            name: name.clone(),
            style: style.clone(),
            lines: lines.to_vec(),
        };
        match self.find(&name) {
            Some(i) if !self.is_builtin(i) => {
                self.all[i] = preset;
                i
            }
            _ => {
                self.all.push(preset);
                self.all.len() - 1
            }
        }
    }

    /// Removes a user preset. Built-ins and out-of-range indices are refused (`false`), so a
    /// stale index from a previous frame cannot delete the wrong thing.
    ///
    /// The caller keeps the config's lines and style as they are and only drops the label --
    /// deleting a preset must not change what is on the wallpaper.
    pub fn delete(&mut self, i: usize) -> bool {
        if self.is_builtin(i) || i >= self.all.len() {
            return false;
        }
        self.all.remove(i);
        true
    }
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

    #[test]
    fn builtin_list_includes_daily_countdown() {
        assert!(builtin().iter().any(|p| p.name == "Daily countdown"));
    }

    /// Every builtin template must render with no `{` left behind -- an
    /// unresolvable token in a shipped preset would print as a typo.
    #[test]
    fn builtin_templates_use_only_known_tokens() {
        let now = jiff::civil::datetime(2026, 7, 15, 12, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(9)))
            .unwrap();
        let target = jiff::civil::datetime(2026, 10, 24, 9, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(9)))
            .unwrap();
        let b = crate::countdown::breakdown(&now, &target);
        let d = crate::countdown::daily_breakdown(&now, jiff::civil::time(18, 0, 0, 0));
        for p in builtin() {
            for l in &p.lines {
                let rendered = crate::tokens::render(&l.text, &b, &d);
                assert!(
                    !rendered.contains('{'),
                    "unresolved token in preset '{}': {rendered}",
                    p.name
                );
            }
        }
    }

    fn lib() -> Library {
        Library::new(vec![Preset {
            name: "Mine".to_string(),
            style: Style {
                size_px: 99.0,
                ..Style::default()
            },
            lines: vec![Line {
                text: "hi".to_string(),
                ..Line::default()
            }],
        }])
    }

    #[test]
    fn user_presets_come_after_the_builtins() {
        let l = lib();
        assert_eq!(l.all().len(), BUILTIN_COUNT + 1);
        assert!(l.is_builtin(0));
        assert!(!l.is_builtin(BUILTIN_COUNT));
        assert_eq!(l.user().len(), 1);
        assert_eq!(l.user()[0].name, "Mine");
    }

    #[test]
    fn a_label_whose_look_matches_resolves_clean() {
        let l = lib();
        let p = &l.all()[0];
        assert_eq!(
            l.resolve(Some(&p.name), &p.lines, &p.style),
            Active::Clean(0)
        );
    }

    #[test]
    fn a_label_whose_lines_differ_resolves_modified() {
        let l = lib();
        let p = l.all()[0].clone();
        let edited = vec![Line {
            text: "edited".to_string(),
            ..Line::default()
        }];
        assert_eq!(
            l.resolve(Some(&p.name), &edited, &p.style),
            Active::Modified(0)
        );
    }

    /// The preset carries the style too, so a style-only edit is just as much a modification
    /// as a line edit. This is the case a lines-only preset model would have missed.
    #[test]
    fn a_label_whose_style_differs_resolves_modified() {
        let l = lib();
        let p = l.all()[0].clone();
        let restyled = Style {
            size_px: 12.0,
            ..p.style.clone()
        };
        assert_eq!(
            l.resolve(Some(&p.name), &p.lines, &restyled),
            Active::Modified(0)
        );
    }

    /// No label (an old config.toml, or a hand-edited one) is not `Custom` on its own: the
    /// look is matched against the list and gets its name back. This is what lets the
    /// migration carry no code at all.
    #[test]
    fn a_missing_label_recovers_the_name_from_the_look() {
        let l = lib();
        let p = l.all()[1].clone();
        assert_eq!(l.resolve(None, &p.lines, &p.style), Active::Clean(1));
    }

    #[test]
    fn a_missing_label_with_no_matching_look_is_custom() {
        let l = lib();
        let odd = vec![Line {
            text: "nothing like a preset".to_string(),
            ..Line::default()
        }];
        assert_eq!(l.resolve(None, &odd, &Style::default()), Active::Custom);
    }

    /// A deleted preset leaves its name behind in config.toml. That is not an error.
    #[test]
    fn a_label_naming_no_preset_falls_back_to_matching_the_look() {
        let l = lib();
        let p = l.all()[2].clone();
        assert_eq!(
            l.resolve(Some("gone"), &p.lines, &p.style),
            Active::Clean(2)
        );

        let odd = vec![Line {
            text: "nothing like a preset".to_string(),
            ..Line::default()
        }];
        assert_eq!(
            l.resolve(Some("gone"), &odd, &Style::default()),
            Active::Custom
        );
    }

    /// The legacy flag only ever exists on a config loaded from an old file; no preset carries
    /// it. Comparing it would make such a config permanently `Modified` against every preset.
    #[test]
    fn the_legacy_summary_flag_is_ignored_when_comparing_styles() {
        let legacy = Style {
            show_summary_line: Some(false),
            ..Style::default()
        };
        assert!(style_eq(&legacy, &Style::default()));
        assert_ne!(
            legacy,
            Style::default(),
            "the derived PartialEq still sees it"
        );
    }

    #[test]
    fn apply_replaces_lines_and_style_and_moves_the_label() {
        let l = lib();
        let mut cfg = crate::config::Config::default();
        cfg.style.size_px = 11.0;
        let i = BUILTIN_COUNT; // "Mine"
        l.apply(i, &mut cfg);
        assert_eq!(cfg.lines, l.all()[i].lines);
        assert_eq!(cfg.style.size_px, 99.0);
        assert_eq!(cfg.preset, Some("Mine".to_string()));
        assert_eq!(
            l.resolve(cfg.preset.as_deref(), &cfg.lines, &cfg.style),
            Active::Clean(i)
        );
    }

    /// The whole no-panic guarantee for a stale `pending_preset`/`Active` index rests on this:
    /// `apply` must leave the config untouched rather than index unchecked.
    #[test]
    fn apply_with_an_out_of_range_index_leaves_the_config_untouched() {
        let l = lib();
        let mut cfg = crate::config::Config::default();
        let before = cfg.clone();
        l.apply(999, &mut cfg);
        assert_eq!(cfg, before, "an out-of-range index must not touch cfg");
    }

    #[test]
    fn check_name_rejects_the_empty_string_and_builtin_names() {
        let l = lib();
        assert_eq!(l.check_name(""), NameStatus::Empty);
        assert_eq!(l.check_name("   "), NameStatus::Empty);
        assert_eq!(l.check_name("Clock only"), NameStatus::Builtin);
        assert_eq!(l.check_name("Mine"), NameStatus::Overwrite);
        assert_eq!(l.check_name("Fresh"), NameStatus::New);
    }

    #[test]
    fn save_as_appends_a_user_preset_and_returns_its_index() {
        let mut l = lib();
        let lines = vec![Line {
            text: "saved".to_string(),
            ..Line::default()
        }];
        let style = Style {
            opacity: 0.5,
            ..Style::default()
        };
        let i = l.save_as("Fresh", &lines, &style);
        assert_eq!(i, BUILTIN_COUNT + 1);
        assert_eq!(l.user().len(), 2);
        assert_eq!(l.all()[i].name, "Fresh");
        assert_eq!(l.resolve(Some("Fresh"), &lines, &style), Active::Clean(i));
    }

    #[test]
    fn save_as_over_an_existing_user_preset_replaces_it_in_place() {
        let mut l = lib();
        let lines = vec![Line {
            text: "replaced".to_string(),
            ..Line::default()
        }];
        let i = l.save_as("Mine", &lines, &Style::default());
        assert_eq!(i, BUILTIN_COUNT, "kept its slot");
        assert_eq!(l.user().len(), 1, "no duplicate");
        assert_eq!(l.all()[i].lines, lines);
    }

    #[test]
    fn delete_drops_a_user_preset_and_refuses_a_builtin() {
        let mut l = lib();
        assert!(!l.delete(0), "built-ins cannot be deleted");
        assert_eq!(l.all().len(), BUILTIN_COUNT + 1);

        assert!(l.delete(BUILTIN_COUNT));
        assert_eq!(l.all().len(), BUILTIN_COUNT);
        assert!(l.user().is_empty());
    }

    #[test]
    fn delete_out_of_range_is_ignored() {
        let mut l = lib();
        assert!(!l.delete(999));
        assert_eq!(l.all().len(), BUILTIN_COUNT + 1);
    }

    /// A hand-edited `presets.toml` naming a preset after a built-in cannot get past
    /// `check_name` (that only guards the settings window's own Save-as box), so it has to be
    /// rejected here instead -- otherwise `find` would resolve that name to the built-in
    /// forever, and the user's entry would sit in the list unreachable by name. It still must
    /// not be erased outright, though -- see `dropped()`.
    #[test]
    fn new_drops_a_user_preset_whose_name_collides_with_a_builtin() {
        let l = Library::new(vec![Preset {
            name: "D-Day".to_string(),
            style: Style::default(),
            lines: vec![Line {
                text: "shadowed".to_string(),
                ..Line::default()
            }],
        }]);
        assert_eq!(l.all().len(), BUILTIN_COUNT, "the colliding entry is gone");
        assert!(l.user().is_empty());
        assert_eq!(l.dropped().len(), 1, "it is kept, just not in all()");
        assert_eq!(l.dropped()[0].name, "D-Day");
        assert_eq!(l.dropped()[0].lines[0].text, "shadowed");
    }

    /// Two user presets sharing a name are just as unreachable by `find` as a built-in
    /// collision -- only the first is ever resolved. The later one is dropped rather than
    /// silently shadowing the first, but (as above) not erased.
    #[test]
    fn new_drops_a_later_user_preset_that_duplicates_an_earlier_ones_name() {
        let l = Library::new(vec![
            Preset {
                name: "Mine".to_string(),
                style: Style::default(),
                lines: vec![Line {
                    text: "first".to_string(),
                    ..Line::default()
                }],
            },
            Preset {
                name: "Mine".to_string(),
                style: Style::default(),
                lines: vec![Line {
                    text: "second".to_string(),
                    ..Line::default()
                }],
            },
        ]);
        assert_eq!(l.user().len(), 1);
        assert_eq!(l.user()[0].lines[0].text, "first");
        assert_eq!(l.dropped().len(), 1);
        assert_eq!(l.dropped()[0].lines[0].text, "second");
    }

    /// Finding 1: a name-colliding preset must survive being written back out. This is what
    /// `SettingsApp::persist_presets` relies on -- it writes `user()` followed by `dropped()`,
    /// so feeding that combined list back into `Library::new` (simulating the next launch,
    /// after a `presets_io::save` + `presets_io::load` round trip) must still contain the
    /// colliding preset, not silently lose it on a second pass.
    #[test]
    fn a_name_colliding_preset_survives_a_persist_and_reload_round_trip() {
        let l = Library::new(vec![Preset {
            name: "D-Day".to_string(),
            style: Style::default(),
            lines: vec![Line {
                text: "shadowed".to_string(),
                ..Line::default()
            }],
        }]);

        // What `persist_presets` writes to `presets.toml`.
        let mut to_save = l.user().to_vec();
        to_save.extend(l.dropped().iter().cloned());
        assert_eq!(to_save.len(), 1, "the colliding preset is in the write-out");

        // What the next launch builds from that file.
        let reloaded = Library::new(to_save);
        assert_eq!(
            reloaded.dropped().len(),
            1,
            "still dropped, but still present, after a full round trip"
        );
        assert_eq!(reloaded.dropped()[0].name, "D-Day");
    }

    /// Finding 1: whichever way a preset is dropped -- a name collision in `new`, or a failed
    /// `config::validate` folded in via `add_dropped` -- it must be a true dead end: absent
    /// from `all()`, unreachable by `find`, unmatched by `resolve`, and outside every index
    /// `delete` could ever be called with.
    #[test]
    fn dropped_presets_are_unreachable_by_all_find_resolve_and_delete() {
        let mut l = Library::new(vec![Preset {
            name: "D-Day".to_string(), // collides with a builtin
            style: Style::default(),
            lines: vec![Line {
                text: "shadowed".to_string(),
                ..Line::default()
            }],
        }]);
        let invalid = Preset {
            name: "Bad".to_string(), // unique name; would-be validation failure
            style: Style {
                opacity: 3.0,
                ..Style::default()
            },
            lines: vec![Line {
                text: "invalid".to_string(),
                ..Line::default()
            }],
        };
        l.add_dropped(vec![invalid.clone()]);

        assert_eq!(l.dropped().len(), 2, "both dropped presets are kept");

        // Not in all(): every slot in all() is a builtin or a well-formed user preset.
        assert_eq!(l.all().len(), BUILTIN_COUNT, "neither entered all()");
        assert!(l.all().iter().all(|p| p.name != "Bad"));

        // Not reachable by find(): "D-Day" resolves to the *builtin* of that name, not the
        // dropped, shadowed one; "Bad" resolves to nothing at all.
        let day = l.find("D-Day").expect("the builtin still resolves");
        assert!(l.is_builtin(day), "resolves to the builtin, not the drop");
        assert_eq!(l.find("Bad"), None);

        // Not reachable by resolve(): neither dropped preset's look matches anything in
        // all(), so labelling the config with its name and asking `resolve` for it lands on
        // Custom rather than picking the dropped entry up.
        assert_eq!(
            l.resolve(Some("Bad"), &invalid.lines, &invalid.style),
            Active::Custom
        );

        // Not reachable by delete(): every valid index into all() names a builtin or a
        // surviving user preset, never a dropped one, so no index deletes it.
        for i in 0..l.all().len() {
            let before = l.dropped().len();
            l.delete(i);
            assert_eq!(
                l.dropped().len(),
                before,
                "deleting a real slot must not touch dropped()"
            );
        }
    }
}
