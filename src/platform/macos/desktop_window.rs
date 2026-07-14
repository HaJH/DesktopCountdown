//! A borderless `NSWindow` that sits below the Finder's desktop icons, with one `CALayer`
//! carrying the countdown bitmap.
//!
//! The Windows counterpart is `workerw.rs`, and it is most of that file's length: there is
//! no wallpaper window to hunt for here, nothing to re-parent, nothing that can die under
//! us and need re-acquiring with backoff. A window at the right level simply *is* the
//! wallpaper layer.
//!
//! Every setting below was verified on a real machine before any of this was written; see
//! `docs/superpowers/plans/macos-spike-result.md`.

use anyhow::{anyhow, Result};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
    NSWindowLevel, NSWindowStyleMask,
};
use objc2_core_graphics::{kCGDesktopIconWindowLevel, CGImage};
use objc2_foundation::{NSPoint, NSRect};
use objc2_quartz_core::{CALayer, CATransaction};

/// One step below the level the Finder puts its desktop icons at, which is what puts us
/// under them. Anything below `kCGDesktopIconWindowLevel` would do; this is the top of that
/// range, so nothing else that means to be wallpaper ends up above us by accident.
const DESKTOP_LEVEL: NSWindowLevel = (kCGDesktopIconWindowLevel - 1) as NSWindowLevel;

pub struct DesktopWindow {
    window: Retained<NSWindow>,
    /// Carries the countdown bitmap. A sublayer, not the root: the root's frame is the
    /// whole screen and never moves, while this one moves to wherever the anchor puts it.
    content: Retained<CALayer>,
    /// The window's own origin in AppKit coordinates, which every layer frame is relative to.
    origin: NSPoint,
}

impl DesktopWindow {
    /// Covers `frame` (one screen, in AppKit coordinates) entirely. The window never moves
    /// again; only the layer inside it does.
    pub fn new(mtm: MainThreadMarker, frame: NSRect) -> Result<Self> {
        // SAFETY: NSWindow's designated initializer, on the main thread.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };

        window.setLevel(DESKTOP_LEVEL);
        window.setCollectionBehavior(
            // One window, shared by every Space, that does not move when they switch --
            // which is why a Space change needs no rebuild here, unlike a WorkerW outage
            // on Windows.
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::FullScreenNone,
        );
        window.setIgnoresMouseEvents(true);
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(false);
        // SAFETY: `false` is the direction that keeps the window alive; the `Retained`
        // below owns it. (`true` is what would risk a use-after-free.)
        unsafe { window.setReleasedWhenClosed(false) };
        window.setCanHide(false);
        window.setMovable(false);
        window.setExcludedFromWindowsMenu(true);
        window.setAnimationBehavior(NSWindowAnimationBehavior::None);
        window.setRestorable(false);
        window.disableSnapshotRestoration();

        let view = window
            .contentView()
            .ok_or_else(|| anyhow!("the desktop window has no content view"))?;
        view.setWantsLayer(true);
        let root = view
            .layer()
            .ok_or_else(|| anyhow!("wantsLayer(true) produced no layer"))?;

        let content = CALayer::layer();
        root.addSublayer(&content);

        // Never makeKeyAndOrderFront: this window must never take focus.
        window.orderFrontRegardless();

        Ok(Self {
            window,
            content,
            origin: frame.origin,
        })
    }

    /// Puts `image` on screen at `frame` (AppKit coordinates, points).
    ///
    /// `scale` is the screen's `backingScaleFactor`; `image` is `frame.size * scale` pixels,
    /// and `contentsScale` is what tells CoreAnimation to draw it one-for-one instead of
    /// stretching it.
    pub fn draw(&self, image: &CGImage, frame: NSRect, scale: f64) {
        // Layer property changes animate by default, over a quarter of a second. Left
        // alone, every tick of the clock would cross-fade into the next -- a countdown
        // that is never quite legible.
        CATransaction::begin();
        CATransaction::setDisableActions(true);

        self.content.setContentsScale(scale);
        self.content.setFrame(NSRect::new(
            NSPoint::new(
                frame.origin.x - self.origin.x,
                frame.origin.y - self.origin.y,
            ),
            frame.size,
        ));
        // SAFETY: `CALayer.contents` accepts a `CGImageRef`; the layer retains it.
        unsafe {
            let object: &AnyObject = (**image).as_ref();
            self.content.setContents(Some(object));
        }

        CATransaction::commit();
    }
}

impl Drop for DesktopWindow {
    fn drop(&mut self) {
        // `releasedWhenClosed` is false, so this only unmaps the window; the `Retained`
        // still owns it and frees it as this struct goes away.
        self.window.close();
    }
}
