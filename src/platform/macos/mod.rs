//! The macOS backend: an `NSWindow` at the desktop level per monitor, painted with
//! CoreText/CoreGraphics and handed to a `CALayer`.
//!
//! # Status
//!
//! Complete through design §9 step 6. What is left is the settings window's activation
//! policy (step 7) and packaging (step 8).
//!
//! The window settings were verified on a real machine before any of this was written; see
//! `docs/superpowers/plans/macos-spike-result.md`.

pub mod autostart;
mod desktop_window;
pub mod fonts;
mod monitors;
mod panels;
mod render;
mod single_instance;
mod tray;
mod watch;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;
use std::rc::Rc;

use anyhow::{anyhow, Result};
use block2::RcBlock;
use jiff::Zoned;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationDidChangeScreenParametersNotification, NSEvent, NSEventModifierFlags, NSEventType,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSTimer};

use crate::app::{ms_to_next_second, AppCore};

pub use monitors::enumerate as enumerate_monitors;
pub use panels::Panels;
pub use render::{Canvas, Composed, Painter};
pub use single_instance::SingleInstance;
pub use tray::{Tray, TrayCommand};
pub use watch::ConfigWatcher;

/// How long the config file must stay quiet before we re-read it. One save produces a short
/// burst of events (we write a temp file and rename it over the target; an outside editor
/// may write in several steps), so let the burst settle -- but only just: this delay is the
/// whole latency between a settings-window edit and the wallpaper changing.
const RELOAD_DEBOUNCE_SECS: f64 = 0.080;

/// Sets the activation policy, which has to happen before anything makes a window or a menu
/// bar item -- and `AppCore::new` does both, through `Tray::new`.
///
/// `Accessory` keeps the app out of the Dock and out of Cmd-Tab while still letting it own a
/// menu bar item. It is the runtime half of `LSUIElement` in the bundle's plist; setting it
/// here means an unbundled `cargo run` behaves the same way the shipped `.app` will.
pub fn init() -> Result<()> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| anyhow!("DesktopCountdown must start on the main thread"))?;
    NSApplication::sharedApplication(mtm)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    Ok(())
}

/// Everything the timer blocks need, on the main thread and nowhere else.
///
/// The blocks reach it through a `Weak`; the config watcher's dispatched wake-up reaches it
/// through `LOOP`. The Windows backend does the same job with `GWLP_USERDATA` and a raw
/// pointer -- this is the same idea with the lifetime actually checked.
struct EventLoop {
    mtm: MainThreadMarker,
    core: RefCell<AppCore>,
    /// The one-shot reload timer, restarted on every filesystem wake-up so that a burst of
    /// events collapses into a single reload. The Windows backend restarts a Win32 timer on
    /// `WM_CONFIG_DIRTY` for exactly the same reason.
    reload_timer: RefCell<Option<Retained<NSTimer>>>,
    /// Set by the screen-parameters observer, drained by the next tick.
    ///
    /// AppKit fires that notification several times in a row for a single change, so acting
    /// on each one would tear every window down and back up three or four times. A flag the
    /// tick drains coalesces the burst into one rebuild, at the cost of up to a second's
    /// delay -- which nobody sees, because the countdown is redrawn on that same tick.
    displays_changed: Cell<bool>,
}

thread_local! {
    /// The live `EventLoop`, so the config watcher's dispatched block can find it.
    ///
    /// Only ever touched on the main thread: `dispatch_async` to the main queue runs its
    /// block there, and `run` sets and clears this from there too.
    static LOOP: RefCell<Option<Rc<EventLoop>>> = const { RefCell::new(None) };
}

/// Called on the main thread by the config watcher, once per filesystem event.
///
/// Events arriving before `run` has installed the loop are dropped, exactly as the Windows
/// watcher drops the ones that arrive before it has a window to post to: the config was just
/// read at startup, so there is nothing to miss.
pub(super) fn wake_for_config_change() {
    let Some(lp) = LOOP.with(|l| l.borrow().clone()) else {
        return;
    };
    lp.arm_reload();
}

impl EventLoop {
    /// Starts, or restarts, the debounce timer. Restarting is the whole point: one save is a
    /// burst of filesystem events, and only the last of them should lead to a reload.
    fn arm_reload(self: &Rc<Self>) {
        if let Some(old) = self.reload_timer.borrow_mut().take() {
            old.invalidate();
        }

        let weak = Rc::downgrade(self);
        let block = RcBlock::new(move |_: NonNull<NSTimer>| {
            let Some(lp) = weak.upgrade() else { return };
            lp.reload_timer.replace(None);
            lp.guarded("config reload", |core| {
                core.on_config_dirty();
                false
            });
        });

        // SAFETY: the block only touches main-thread state, and it is attached to a timer
        // scheduled on -- and therefore only ever fired by -- this thread's run loop.
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_repeats_block(
                RELOAD_DEBOUNCE_SECS,
                false,
                &block,
            )
        };
        *self.reload_timer.borrow_mut() = Some(timer);
    }

    /// Runs `f` against the core, swallowing a panic rather than letting it unwind into
    /// Objective-C.
    ///
    /// A panic crossing back into a block AppKit called is undefined behaviour, just as one
    /// crossing back into `DispatchMessageW` is on Windows. A panic partway through a tick
    /// can leave `AppCore` half-updated -- a stale frame, at worst -- so log loudly and carry
    /// on rather than take the process down.
    fn guarded(&self, what: &str, f: impl FnOnce(&mut AppCore) -> bool) -> bool {
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut core = self.core.borrow_mut();
            f(&mut core)
        }));
        match outcome {
            Ok(quit) => quit,
            Err(payload) => {
                tracing::error!(
                    message = panic_message(&*payload),
                    what,
                    "panic inside a run-loop callback, dropped"
                );
                false
            }
        }
    }

    /// Returns whether the app should quit.
    fn tick(&self) -> bool {
        let displays_changed = self.displays_changed.replace(false);
        self.guarded("tick", |core| {
            if displays_changed {
                core.on_display_change();
            }
            if let Err(e) = core.tick() {
                tracing::error!("tick failed: {e:#}");
            }
            core.wants_quit()
        })
    }
}

/// Blocks in the run loop until the tray's "quit".
pub fn run(core: AppCore) -> Result<()> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| anyhow!("the run loop must be driven from the main thread"))?;

    let lp = Rc::new(EventLoop {
        mtm,
        core: RefCell::new(core),
        reload_timer: RefCell::new(None),
        displays_changed: Cell::new(false),
    });
    LOOP.with(|l| *l.borrow_mut() = Some(Rc::clone(&lp)));

    // Held until `run` returns: dropping the token unregisters the observer.
    let _observer = observe_screen_changes(&lp);

    let app = NSApplication::sharedApplication(mtm);
    // `delegate` is a weak property -- AppKit does not retain it -- so this must outlive
    // the run loop that calls into it.
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    schedule_tick(&lp);
    app.run();

    // AppKit must not be left with a dangling delegate pointer once `delegate` drops.
    app.setDelegate(None);

    LOOP.with(|l| *l.borrow_mut() = None);
    Ok(())
}

define_class!(
    /// Exists for one method: the reopen event.
    ///
    /// SAFETY:
    /// - `NSObject` has no subclassing requirements.
    /// - `AppDelegate` holds no ivars and does not implement `Drop`.
    /// - `MainThreadOnly`, as an app delegate must be: AppKit only ever calls it on the
    ///   main thread, which is where `LOOP` and everything under it lives.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "DCAppDelegate"]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        /// Launching an app that is already running does not start a second process on
        /// macOS: LaunchServices activates the running one and sends it this. So the
        /// "launch it again to get the settings window" path in `main` -- which is a
        /// second process finding the single-instance lock taken -- is never reached from
        /// Finder or the Dock, and before this existed the app simply did nothing.
        ///
        /// `false` tells AppKit not to run its own default handling (unhiding windows,
        /// or making a new one). We have no windows to unhide: the countdown lives on the
        /// wallpaper layer and the settings window is a separate process.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(
            &self,
            _sender: &NSApplication,
            _has_visible_windows: bool,
        ) -> bool {
            tracing::info!("reopened; opening the settings window");
            if let Some(lp) = LOOP.with(|l| l.borrow().clone()) {
                // `guarded` for the same reason the timer blocks use it: a panic unwinding
                // from here would cross back into Objective-C, which is undefined behaviour.
                lp.guarded("reopen", |core| {
                    core.open_settings();
                    false
                });
            }
            false
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: `NSObject`'s designated initialiser, and the class has no ivars to set up.
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

fn observe_screen_changes(lp: &Rc<EventLoop>) -> Retained<ProtocolObject<dyn NSObjectProtocol>> {
    let weak = Rc::downgrade(lp);
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        if let Some(lp) = weak.upgrade() {
            lp.displays_changed.set(true);
        }
    });

    // SAFETY: a `None` queue means the block runs synchronously on the thread that posted
    // the notification, and AppKit posts this one on the main thread -- which is where the
    // state the block touches lives.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSApplicationDidChangeScreenParametersNotification),
            None,
            None,
            &block,
        )
    }
}

/// One-shot, re-armed to the next whole second on every fire -- the same cadence the Windows
/// backend gets from `SetTimer` plus `ms_to_next_second`, so the clock never drifts into
/// updating just before or just after the second it is showing.
fn schedule_tick(lp: &Rc<EventLoop>) {
    let ms = ms_to_next_second(Zoned::now().subsec_nanosecond() as u32);
    let weak = Rc::downgrade(lp);

    let block = RcBlock::new(move |_: NonNull<NSTimer>| {
        let Some(lp) = weak.upgrade() else { return };
        if lp.tick() {
            stop(lp.mtm);
            return;
        }
        schedule_tick(&lp);
    });

    // SAFETY: scheduled on this thread's run loop, so the block only ever runs here, which
    // is where every piece of state it reaches lives.
    unsafe {
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(f64::from(ms) / 1000.0, false, &block)
    };
}

/// Ends the run loop, so that `run` returns and `main`'s guards -- the log flusher above all
/// -- actually get to run. `std::process::exit` would skip them.
fn stop(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    app.stop(None);

    // `stop:` is only honoured when the *next* event comes through, and a timer is not an
    // event. Without this the app would sit there until the user happened to click something.
    let event =
        NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::ApplicationDefined,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::empty(),
            0.0,
            0,
            None,
            0,
            0,
            0,
        );
    match event {
        Some(event) => app.postEvent_atStart(&event, true),
        None => tracing::warn!("could not post the wake-up event; quitting may be delayed"),
    }
}

/// Renders a caught panic payload for logging. The standard library's own panics carry
/// either a `&str` or a `String`, which covers every panic this process produces; anything
/// else logs a fixed string rather than failing to log at all.
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.as_str()
    } else {
        "<non-string panic payload>"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_painter_is_real() {
        assert!(Painter::new().is_ok());
    }

    /// Reading the launch agent must never fail just because there is not one.
    #[test]
    fn autostart_reads_as_off_when_no_launch_agent_is_installed() {
        assert!(autostart::is_enabled().is_ok());
    }
}
