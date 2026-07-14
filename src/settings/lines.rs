//! Pure line-list editing for the settings window: reordering, adding, removing. No egui.
//! Presets live in `settings::presets`.

use crate::config::Line;

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
/// `config::migrate` would refill it with the default list on the next load. A monitor is
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
