//! Pure conversion helpers for the settings widgets. No egui, no Win32.

use crate::color::parse_hex;
use crate::config::Anchor;
use jiff::civil::DateTime;

/// Config stores colours as `#RRGGBB`; egui's picker wants `[u8; 3]`.
/// Invalid stored colours fall back to white rather than panicking.
pub fn hex_to_rgb(hex: &str) -> [u8; 3] {
    match parse_hex(hex) {
        Some(c) => [c.r, c.g, c.b],
        None => [255, 255, 255],
    }
}

pub fn rgb_to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// The 3x3 anchor grid: row 0 = top, col 0 = left.
pub fn anchor_to_cell(a: Anchor) -> (usize, usize) {
    use Anchor::*;
    match a {
        TopLeft => (0, 0),
        TopCenter => (0, 1),
        TopRight => (0, 2),
        MiddleLeft => (1, 0),
        Center => (1, 1),
        MiddleRight => (1, 2),
        BottomLeft => (2, 0),
        BottomCenter => (2, 1),
        BottomRight => (2, 2),
    }
}

pub fn cell_to_anchor(row: usize, col: usize) -> Anchor {
    use Anchor::*;
    match (row, col) {
        (0, 0) => TopLeft,
        (0, 1) => TopCenter,
        (0, 2) => TopRight,
        (1, 0) => MiddleLeft,
        (1, 2) => MiddleRight,
        (2, 0) => BottomLeft,
        (2, 1) => BottomCenter,
        (2, 2) => BottomRight,
        _ => Center, // (1,1) and any out-of-range
    }
}

/// The countdown target as six editable integers. egui has no date picker,
/// so each field is a DragValue and this validates the combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateFields {
    pub year: i16,
    pub month: i8,
    pub day: i8,
    pub hour: i8,
    pub minute: i8,
    pub second: i8,
}

pub fn fields_from_datetime(dt: DateTime) -> DateFields {
    DateFields {
        year: dt.year(),
        month: dt.month(),
        day: dt.day(),
        hour: dt.hour(),
        minute: dt.minute(),
        second: dt.second(),
    }
}

/// Returns `None` for an impossible date (Feb 30, month 13, hour 24, ...).
/// `jiff::civil::DateTime::new` validates the whole combination including leap years.
pub fn datetime_from_fields(f: &DateFields) -> Option<DateTime> {
    DateTime::new(f.year, f.month, f.day, f.hour, f.minute, f.second, 0).ok()
}

/// The settings window writes an edit as soon as it happens, but no more often than once
/// per `min_interval_ms` -- so an ongoing drag does not write the file on every UI frame.
pub fn should_save(dirty: bool, ms_since_last_write: u64, min_interval_ms: u64) -> bool {
    dirty && ms_since_last_write >= min_interval_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Anchor;

    #[test]
    fn hex_rgb_round_trips() {
        assert_eq!(hex_to_rgb("#FF8800"), [255, 136, 0]);
        assert_eq!(rgb_to_hex([255, 136, 0]), "#FF8800");
        // Round-trip through both directions.
        for c in [[0, 0, 0], [255, 255, 255], [18, 52, 86]] {
            assert_eq!(hex_to_rgb(&rgb_to_hex(c)), c);
        }
    }

    #[test]
    fn hex_to_rgb_falls_back_on_garbage() {
        // An invalid stored colour must not panic; fall back to white.
        assert_eq!(hex_to_rgb("not-a-colour"), [255, 255, 255]);
        assert_eq!(hex_to_rgb(""), [255, 255, 255]);
    }

    #[test]
    fn rgb_to_hex_is_uppercase_six_digits() {
        assert_eq!(rgb_to_hex([0, 0, 0]), "#000000");
        assert_eq!(rgb_to_hex([171, 205, 239]), "#ABCDEF");
    }

    #[test]
    fn anchor_grid_round_trips_all_nine() {
        use Anchor::*;
        for a in [
            TopLeft,
            TopCenter,
            TopRight,
            MiddleLeft,
            Center,
            MiddleRight,
            BottomLeft,
            BottomCenter,
            BottomRight,
        ] {
            let (r, c) = anchor_to_cell(a);
            assert!(r < 3 && c < 3);
            assert_eq!(cell_to_anchor(r, c), a);
        }
    }

    #[test]
    fn anchor_cells_are_positioned_correctly() {
        assert_eq!(anchor_to_cell(Anchor::TopLeft), (0, 0));
        assert_eq!(anchor_to_cell(Anchor::Center), (1, 1));
        assert_eq!(anchor_to_cell(Anchor::BottomRight), (2, 2));
        assert_eq!(cell_to_anchor(0, 2), Anchor::TopRight);
        assert_eq!(cell_to_anchor(2, 0), Anchor::BottomLeft);
    }

    #[test]
    fn should_save_when_dirty_and_the_interval_since_the_last_write_has_passed() {
        assert!(!should_save(false, 9999, 100)); // nothing to save
        assert!(!should_save(true, 50, 100)); // dirty, but we wrote 50ms ago
        assert!(should_save(true, 100, 100)); // interval elapsed (boundary)
        assert!(should_save(true, 9999, 100)); // first edit after a long pause
    }

    use jiff::civil::datetime;

    #[test]
    fn datetime_fields_round_trip() {
        let dt = datetime(2026, 10, 24, 9, 30, 15, 0);
        let f = fields_from_datetime(dt);
        assert_eq!(
            (f.year, f.month, f.day, f.hour, f.minute, f.second),
            (2026, 10, 24, 9, 30, 15)
        );
        assert_eq!(datetime_from_fields(&f), Some(dt));
    }

    #[test]
    fn invalid_dates_return_none() {
        let feb30 = DateFields {
            year: 2026,
            month: 2,
            day: 30,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert_eq!(datetime_from_fields(&feb30), None);
        let month13 = DateFields {
            year: 2026,
            month: 13,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert_eq!(datetime_from_fields(&month13), None);
        let hour24 = DateFields {
            year: 2026,
            month: 1,
            day: 1,
            hour: 24,
            minute: 0,
            second: 0,
        };
        assert_eq!(datetime_from_fields(&hour24), None);
    }

    #[test]
    fn leap_day_is_valid_in_a_leap_year() {
        let f = DateFields {
            year: 2028,
            month: 2,
            day: 29,
            hour: 12,
            minute: 0,
            second: 0,
        };
        assert!(datetime_from_fields(&f).is_some());
        let f = DateFields {
            year: 2026,
            month: 2,
            day: 29,
            hour: 12,
            minute: 0,
            second: 0,
        };
        assert_eq!(datetime_from_fields(&f), None); // 2026 is not a leap year
    }
}
