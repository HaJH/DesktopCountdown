//! Monitor enumeration with stable per-display identifiers.
//!
//! # Two coordinate systems
//!
//! AppKit's screen space is y-**up**, with the origin at the bottom-left of the primary
//! screen. The shared `layout::place` is y-**down** (its `TopLeft` anchor is `monitor.y`,
//! and a positive y offset moves *down*), because that is what Win32 hands it.
//!
//! So `MonitorInfo.rect` is published y-down, with the origin at the **top**-left of the
//! primary screen -- the same shape Windows produces. `to_appkit` converts back, and
//! `panels` uses it to place the windows. Nothing outside this module sees AppKit's y-up.
//!
//! Units are **points**, not pixels. `scale` carries the Retina factor separately, and the
//! renderer multiplies by it when it sizes the bitmap. Sizes in `config.toml` are therefore
//! points on macOS and physical pixels on Windows: the same number, the same apparent size
//! on a normal display, and crisp instead of doubled on a Retina one.

use anyhow::{anyhow, Result};
use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_core_foundation::{CFRetained, CFString, CFUUID};
use objc2_core_graphics::CGDirectDisplayID;
use objc2_foundation::{ns_string, NSNumber, NSPoint, NSRect, NSSize};

use crate::layout::Rect;
use crate::platform::MonitorInfo;

// `CGDisplayCreateUUIDFromDisplayID` is not in CoreGraphics, despite the name and despite
// every article that mentions it: it is exported by **ColorSync**, declared in
// `ColorSyncDevice.h`. objc2-core-graphics does not bind it, so declare it here.
#[link(name = "ColorSync", kind = "framework")]
extern "C" {
    fn CGDisplayCreateUUIDFromDisplayID(display: CGDirectDisplayID) -> *mut CFUUID;
}

pub fn enumerate() -> Result<Vec<MonitorInfo>> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| anyhow!("monitors can only be enumerated on the main thread"))?;
    Ok(enumerate_on_main(mtm))
}

fn enumerate_on_main(mtm: MainThreadMarker) -> Vec<MonitorInfo> {
    let screens = NSScreen::screens(mtm);
    let flip = flip_reference(mtm);

    screens
        .iter()
        .enumerate()
        .map(|(i, screen)| {
            let frame = screen.frame();
            let id = display_id(&screen)
                .and_then(display_uuid)
                // A screen with no display id or no UUID still has to be drawn on. Fall
                // back to its index, which is at least stable while the app is running --
                // it just will not match a config override across a reconnect.
                .unwrap_or_else(|| {
                    tracing::warn!(index = i, "no display UUID, using the index as the id");
                    format!("screen-index-{i}")
                });

            MonitorInfo {
                name: format!(
                    "{} ({}x{})",
                    screen.localizedName(),
                    frame.size.width as i32,
                    frame.size.height as i32
                ),
                id,
                rect: to_layout(frame, flip),
                scale: screen.backingScaleFactor() as f32,
            }
        })
        .collect()
}

/// The y coordinate, in AppKit's space, of the top edge of the primary screen. Everything
/// y-down is measured down from here.
///
/// `NSScreen::screens()[0]` is the screen with the menu bar, and AppKit's origin is its
/// bottom-left corner -- so this is just its height, but read it off the frame rather than
/// assume the origin is exactly (0, 0).
fn flip_reference(mtm: MainThreadMarker) -> f64 {
    NSScreen::screens(mtm)
        .iter()
        .next()
        .map(|primary| {
            let f = primary.frame();
            f.origin.y + f.size.height
        })
        .unwrap_or(0.0)
}

/// AppKit (y-up, origin at the primary's bottom-left) -> layout (y-down, origin at the
/// primary's top-left).
fn to_layout(frame: NSRect, flip: f64) -> Rect {
    Rect {
        x: frame.origin.x.round() as i32,
        y: (flip - (frame.origin.y + frame.size.height)).round() as i32,
        w: frame.size.width.round() as i32,
        h: frame.size.height.round() as i32,
    }
}

/// The inverse of `to_layout`: a y-down rect back into the AppKit frame an `NSWindow` wants.
pub fn to_appkit(rect: Rect, flip: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(
            f64::from(rect.x),
            flip - f64::from(rect.y) - f64::from(rect.h),
        ),
        NSSize::new(f64::from(rect.w), f64::from(rect.h)),
    )
}

/// Exposed so `panels` can convert placements without re-deriving the reference.
pub fn appkit_flip_reference(mtm: MainThreadMarker) -> f64 {
    flip_reference(mtm)
}

fn display_id(screen: &NSScreen) -> Option<CGDirectDisplayID> {
    let desc = screen.deviceDescription();
    let value = desc.objectForKey(ns_string!("NSScreenNumber"))?;
    let number: &NSNumber = value.downcast_ref()?;
    Some(number.unsignedIntValue())
}

/// The display's UUID, as a string.
///
/// **Not** the `CGDirectDisplayID`: that is reassigned when a display is reconnected or the
/// machine reboots, so a config override keyed on it would follow the wrong monitor (or no
/// monitor) after a cable swap. The UUID is stable, which is what makes it the counterpart
/// of the Windows device interface name.
fn display_uuid(id: CGDirectDisplayID) -> Option<String> {
    // SAFETY: `CGDisplayCreateUUIDFromDisplayID` follows the Create rule, returning an
    // owned CFUUID or null.
    let uuid = unsafe {
        let raw = CGDisplayCreateUUIDFromDisplayID(id);
        CFRetained::from_raw(std::ptr::NonNull::new(raw)?)
    };
    let s: CFRetained<CFString> = CFUUID::new_string(None, Some(&uuid))?;
    Some(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not run by default: needs a main thread with AppKit, which `cargo test`'s harness
    /// does not give a test thread. Run with
    /// `cargo test monitors -- --ignored --nocapture` from a real session.
    #[test]
    #[ignore]
    fn prints_the_real_monitor_layout() {
        let mtm = MainThreadMarker::new().expect("main thread");
        for m in enumerate_on_main(mtm) {
            println!("{m:#?}");
        }
    }

    /// The flip has to be its own inverse, or a window would land on the wrong screen --
    /// and it is the only thing standing between AppKit's y-up and the shared layout's
    /// y-down.
    #[test]
    fn the_appkit_flip_round_trips() {
        let flip = 1169.0; // a primary screen 1169pt tall
        for frame in [
            // The primary itself.
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1800.0, 1169.0)),
            // A screen above and to the left of it: negative x, positive AppKit y.
            NSRect::new(NSPoint::new(-2560.0, 1169.0), NSSize::new(2560.0, 1440.0)),
            // A screen below it: negative AppKit y.
            NSRect::new(NSPoint::new(200.0, -1440.0), NSSize::new(2560.0, 1440.0)),
        ] {
            let layout = to_layout(frame, flip);
            assert_eq!(
                to_appkit(layout, flip),
                frame,
                "round trip failed: {frame:?}"
            );
        }
    }

    /// The primary screen's top-left is the origin of the y-down space, exactly as the
    /// primary monitor's top-left is on Windows.
    #[test]
    fn the_primary_screen_starts_at_the_origin() {
        let flip = 1169.0;
        let primary = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1800.0, 1169.0));
        assert_eq!(
            to_layout(primary, flip),
            Rect {
                x: 0,
                y: 0,
                w: 1800,
                h: 1169
            }
        );
    }

    /// A screen sitting above the primary gets a negative y, which is what `layout::place`
    /// expects of a virtual desktop and what its own tests cover.
    #[test]
    fn a_screen_above_the_primary_gets_a_negative_y() {
        let flip = 1169.0;
        let above = NSRect::new(NSPoint::new(0.0, 1169.0), NSSize::new(1800.0, 1000.0));
        assert_eq!(to_layout(above, flip).y, -1000);
    }
}
