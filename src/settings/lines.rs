//! Pure line-list editing for the settings window: presets and reordering. No egui.

use crate::config::{Align, Line, MAIN_TEMPLATE, SUMMARY_SIZE_RATIO, SUMMARY_TEMPLATE};

/// A named line list the user can drop in with one click. Only text and size differ between
/// presets; alignment and colour stay at their defaults, which the user then tweaks.
pub struct Preset {
    pub name: &'static str,
    /// `(text, size_ratio)` per line, top to bottom.
    pub lines: &'static [(&'static str, f32)],
}

impl Preset {
    pub fn build(&self) -> Vec<Line> {
        self.lines
            .iter()
            .map(|(text, size_ratio)| Line {
                text: (*text).to_string(),
                size_ratio: *size_ratio,
                align: Align::Center,
                color: None,
            })
            .collect()
    }
}

pub const PRESETS: [Preset; 5] = [
    Preset {
        name: "Classic",
        lines: &[(SUMMARY_TEMPLATE, SUMMARY_SIZE_RATIO), (MAIN_TEMPLATE, 1.0)],
    },
    Preset {
        name: "Clock only",
        lines: &[(MAIN_TEMPLATE, 1.0)],
    },
    Preset {
        name: "D-Day",
        lines: &[("D-{daysTotal}", 1.0), (MAIN_TEMPLATE, 0.3)],
    },
    Preset {
        name: "Days left",
        lines: &[("{daysTotal} days left", 0.35), (MAIN_TEMPLATE, 1.0)],
    },
    Preset {
        name: "Caption + Clock",
        lines: &[
            ("Deadline", 0.25),
            (MAIN_TEMPLATE, 1.0),
            ("{daysTotal} days left", 0.25),
        ],
    },
];

pub fn move_up(lines: &mut [Line], i: usize) {
    if i > 0 && i < lines.len() {
        lines.swap(i - 1, i);
    }
}

pub fn move_down(lines: &mut [Line], i: usize) {
    if i + 1 < lines.len() {
        lines.swap(i, i + 1);
    }
}

/// Drops line `i`, unless it is the only one: an empty list reads as "not configured", and
/// `config::migrate` would refill it with the classic pair on the next load. A monitor is
/// silenced with `enabled = false`, not by emptying its line list.
pub fn remove(lines: &mut Vec<Line>, i: usize) {
    if lines.len() > 1 && i < lines.len() {
        lines.remove(i);
    }
}

pub fn add(lines: &mut Vec<Line>) {
    lines.push(Line::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(text: &str) -> Line {
        Line {
            text: text.into(),
            ..Line::default()
        }
    }

    #[test]
    fn the_classic_preset_is_the_default_list() {
        let classic = PRESETS
            .iter()
            .find(|p| p.name == "Classic")
            .expect("Classic preset");
        assert_eq!(classic.build(), crate::config::default_lines(true));
    }

    #[test]
    fn every_preset_builds_at_least_one_line_and_leaves_the_rest_at_the_defaults() {
        for p in &PRESETS {
            let lines = p.build();
            assert!(!lines.is_empty(), "{} built nothing", p.name);
            for line in &lines {
                assert!(line.size_ratio > 0.0);
                assert_eq!(line.align, Align::Center);
                assert_eq!(line.color, None);
            }
        }
    }

    #[test]
    fn move_up_swaps_with_the_line_above() {
        let mut v = vec![l("a"), l("b")];
        move_up(&mut v, 1);
        assert_eq!(v[0].text, "b");
        assert_eq!(v[1].text, "a");
    }

    #[test]
    fn move_up_on_the_first_line_does_nothing() {
        let mut v = vec![l("a"), l("b")];
        move_up(&mut v, 0);
        assert_eq!(v[0].text, "a");
    }

    #[test]
    fn move_down_swaps_with_the_line_below() {
        let mut v = vec![l("a"), l("b")];
        move_down(&mut v, 0);
        assert_eq!(v[0].text, "b");
    }

    #[test]
    fn move_down_on_the_last_line_does_nothing() {
        let mut v = vec![l("a"), l("b")];
        move_down(&mut v, 1);
        assert_eq!(v[1].text, "b");
    }

    #[test]
    fn remove_drops_the_line() {
        let mut v = vec![l("a"), l("b")];
        remove(&mut v, 0);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].text, "b");
    }

    #[test]
    fn remove_refuses_to_empty_the_list() {
        let mut v = vec![l("only")];
        remove(&mut v, 0);
        assert_eq!(v.len(), 1, "the last line must survive");
    }

    #[test]
    fn add_appends_a_blank_line_at_the_base_size() {
        let mut v = vec![l("a")];
        add(&mut v);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1], Line::default());
    }

    #[test]
    fn out_of_range_indices_are_ignored() {
        let mut v = vec![l("a"), l("b")];
        move_up(&mut v, 9);
        move_down(&mut v, 9);
        remove(&mut v, 9);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].text, "a");
    }
}
