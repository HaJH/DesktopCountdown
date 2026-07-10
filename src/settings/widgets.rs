//! Pure conversion helpers for the settings widgets. No egui, no Win32.

use crate::color::parse_hex;
use crate::config::Anchor;

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
        TopLeft => (0, 0), TopCenter => (0, 1), TopRight => (0, 2),
        MiddleLeft => (1, 0), Center => (1, 1), MiddleRight => (1, 2),
        BottomLeft => (2, 0), BottomCenter => (2, 1), BottomRight => (2, 2),
    }
}

pub fn cell_to_anchor(row: usize, col: usize) -> Anchor {
    use Anchor::*;
    match (row, col) {
        (0, 0) => TopLeft, (0, 1) => TopCenter, (0, 2) => TopRight,
        (1, 0) => MiddleLeft, (1, 2) => MiddleRight,
        (2, 0) => BottomLeft, (2, 1) => BottomCenter, (2, 2) => BottomRight,
        _ => Center, // (1,1) and any out-of-range
    }
}

/// The settings window saves 500 ms after the last edit, not on every frame.
pub fn should_save(dirty: bool, ms_since_last_change: u64, debounce_ms: u64) -> bool {
    dirty && ms_since_last_change >= debounce_ms
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
        for a in [TopLeft, TopCenter, TopRight, MiddleLeft, Center, MiddleRight,
                  BottomLeft, BottomCenter, BottomRight] {
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
    fn should_save_only_after_debounce_and_when_dirty() {
        assert!(!should_save(false, 9999, 500)); // not dirty
        assert!(!should_save(true, 100, 500));    // dirty but too soon
        assert!(should_save(true, 500, 500));     // dirty and settled (boundary)
        assert!(should_save(true, 700, 500));     // dirty and settled
    }
}
