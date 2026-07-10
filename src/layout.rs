//! Anchor + offset arithmetic in virtual-desktop coordinates. No Win32, no I/O.

use crate::config::Anchor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Places a `content_w` x `content_h` box inside `monitor` (whose coordinates may be
/// negative), anchored per `anchor`, then shifts it by `offset` (+x right, +y down).
///
/// The anchor is relative to the monitor's full rectangle, not its work area, so
/// bottom anchors can land under the taskbar. Callers lift them with a negative y offset.
pub fn place(monitor: Rect, content_w: i32, content_h: i32, anchor: Anchor, offset: [i32; 2]) -> Rect {
    use Anchor::*;

    let x = match anchor {
        TopLeft | MiddleLeft | BottomLeft => monitor.x,
        TopCenter | Center | BottomCenter => monitor.x + (monitor.w - content_w) / 2,
        TopRight | MiddleRight | BottomRight => monitor.x + monitor.w - content_w,
    };
    let y = match anchor {
        TopLeft | TopCenter | TopRight => monitor.y,
        MiddleLeft | Center | MiddleRight => monitor.y + (monitor.h - content_h) / 2,
        BottomLeft | BottomCenter | BottomRight => monitor.y + monitor.h - content_h,
    };

    Rect { x: x + offset[0], y: y + offset[1], w: content_w, h: content_h }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Anchor::*;

    /// The user's leftmost monitor sits at negative virtual-desktop coordinates.
    const NEG: Rect = Rect { x: -3840, y: -368, w: 2560, h: 1440 };
    /// The user's portrait monitor.
    const PORTRAIT: Rect = Rect { x: 2560, y: -556, w: 1440, h: 2560 };

    const CW: i32 = 800;
    const CH: i32 = 140;

    #[test]
    fn center_on_negative_origin_monitor() {
        let r = place(NEG, CW, CH, Center, [0, 0]);
        assert_eq!(r, Rect { x: -2960, y: 282, w: CW, h: CH });
    }

    #[test]
    fn top_left_is_the_monitor_origin() {
        assert_eq!(place(NEG, CW, CH, TopLeft, [0, 0]), Rect { x: -3840, y: -368, w: CW, h: CH });
    }

    #[test]
    fn bottom_right_hugs_the_far_corner() {
        assert_eq!(place(NEG, CW, CH, BottomRight, [0, 0]), Rect { x: -2080, y: 932, w: CW, h: CH });
    }

    #[test]
    fn offset_is_applied_after_the_anchor() {
        let r = place(NEG, CW, CH, BottomRight, [-40, -80]);
        assert_eq!(r, Rect { x: -2120, y: 852, w: CW, h: CH });
    }

    #[test]
    fn center_on_portrait_monitor() {
        let r = place(PORTRAIT, CW, CH, Center, [0, 0]);
        assert_eq!(r, Rect { x: 2880, y: 654, w: CW, h: CH });
    }

    #[test]
    fn top_center_and_middle_left_and_bottom_center() {
        assert_eq!(place(NEG, CW, CH, TopCenter, [0, 0]).x, -2960);
        assert_eq!(place(NEG, CW, CH, TopCenter, [0, 0]).y, -368);
        assert_eq!(place(NEG, CW, CH, MiddleLeft, [0, 0]).x, -3840);
        assert_eq!(place(NEG, CW, CH, MiddleLeft, [0, 0]).y, 282);
        assert_eq!(place(NEG, CW, CH, BottomCenter, [0, 0]).x, -2960);
        assert_eq!(place(NEG, CW, CH, BottomCenter, [0, 0]).y, 932);
    }

    #[test]
    fn top_right_and_middle_right_and_bottom_left() {
        assert_eq!(place(NEG, CW, CH, TopRight, [0, 0]), Rect { x: -2080, y: -368, w: CW, h: CH });
        assert_eq!(place(NEG, CW, CH, MiddleRight, [0, 0]), Rect { x: -2080, y: 282, w: CW, h: CH });
        assert_eq!(place(NEG, CW, CH, BottomLeft, [0, 0]), Rect { x: -3840, y: 932, w: CW, h: CH });
    }

    #[test]
    fn content_wider_than_monitor_overhangs_symmetrically() {
        let r = place(NEG, 3000, CH, Center, [0, 0]);
        assert_eq!(r.x, -4060); // -3840 + (2560 - 3000) / 2
    }
}
