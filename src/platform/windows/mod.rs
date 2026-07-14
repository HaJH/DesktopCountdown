//! The Windows backend: a wallpaper-layer child window per monitor, painted with
//! Direct2D/DirectWrite and composited with DirectComposition.

mod backoff;
mod dcomp;
mod monitors;
mod panels;
mod render;
mod workerw;

pub mod autostart;
pub mod fonts;
mod single_instance;
mod tray;
mod watch;

use std::any::Any;
use std::panic::AssertUnwindSafe;

use anyhow::Result;
use jiff::Zoned;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::app::{ms_to_next_second, AppCore};

pub use monitors::enumerate as enumerate_monitors;
pub use panels::Panels;
pub use render::{Composed, Painter};
pub use single_instance::SingleInstance;
pub use tray::{Tray, TrayCommand};
pub use watch::ConfigWatcher;

const CTRL_CLASS: PCWSTR = w!("DesktopCountdownController");
const TIMER_ID: usize = 1;
/// One-shot timer that debounces `WM_CONFIG_DIRTY` bursts into a single `reload`.
const RELOAD_TIMER_ID: usize = 2;
/// Posted by `ConfigWatcher` on every filesystem event touching `config.toml`.
pub(crate) const WM_CONFIG_DIRTY: u32 = WM_APP + 1;
/// How long the config file must stay quiet before we re-read it. One save produces a
/// short burst of events (we write a temp file and rename it over the target; an outside
/// editor may write in several steps), so let the burst settle -- but only just: this
/// delay is the whole latency between a settings-window edit and the wallpaper changing.
const RELOAD_DEBOUNCE_MS: u32 = 80;

/// Per-monitor DPI awareness. Must happen before any window or monitor query.
pub fn init() -> Result<()> {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)? };
    Ok(())
}

/// Creates the hidden controller window that owns the timer, and blocks in the Win32
/// message loop until `WM_QUIT`.
pub fn run(core: AppCore) -> Result<()> {
    // `core` must not move for as long as `hwnd` exists: `create_controller_window`
    // stores its address in GWLP_USERDATA, and `wndproc` dereferences it on every
    // message. It stays on this stack frame for the rest of `run`, and nothing below
    // moves or drops it before the message loop (and thus the window) is torn down,
    // so the pointer stays valid for its whole lifetime.
    let mut core = core;

    let hwnd = create_controller_window(&mut core)?;
    // Only now does a window exist for the watcher's thread to post to.
    core.watcher().notify_window(hwnd);
    unsafe { SetTimer(Some(hwnd), TIMER_ID, 100, None) };

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

fn create_controller_window(core: &mut AppCore) -> Result<HWND> {
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: CTRL_CLASS,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CTRL_CLASS,
            w!("DesktopCountdown"),
            WS_POPUP, // never shown; exists only to own the timer and receive messages
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst.into()),
            None,
        )?;

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, core as *mut AppCore as isize);
        Ok(hwnd)
    }
}

/// Renders a caught panic payload for logging. `catch_unwind`'s `Err` payload is
/// `Box<dyn Any + Send>`; the standard library's own panics carry either a `&str`
/// (a string-literal message) or a `String` (a formatted one), so those two cover
/// every panic this process produces. Anything else logs a fixed fallback string
/// rather than failing to log at all.
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.as_str()
    } else {
        "<non-string panic payload>"
    }
}

thread_local! {
    // Blocks wndproc from forming a second &mut AppCore if a SENT message (e.g.
    // WM_DPICHANGED during a cross-process SendMessage/SetParent inside tick) re-enters
    // us mid-tick.
    static IN_WNDPROC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The window procedure for the hidden controller window.
///
/// # Safety (panics)
///
/// This function is `extern "system"`: it is called directly by the Win32 message
/// dispatcher, which is C code with no concept of Rust unwinding. Letting a panic
/// unwind out of an `extern "system"` function is undefined behaviour (Rust aborts
/// the process in the best case; in the worst case it corrupts the dispatcher's own
/// stack). `tick`/`on_display_change` call into Direct2D, DirectWrite, and
/// DirectComposition through fallible APIs, but a panic (an out-of-bounds slice
/// index, an `.expect()`, an arithmetic overflow in a debug build, ...) is still
/// possible anywhere below. The whole body below is therefore wrapped in
/// `catch_unwind` so a panic never crosses back into `DispatchMessageW`.
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppCore;
    if ptr.is_null() {
        // No `AppCore` attached yet (messages before `SetWindowLongPtrW`, if any) --
        // nothing to panic on, so this path does not need catch_unwind.
        return DefWindowProcW(hwnd, msg, wp, lp);
    }

    if IN_WNDPROC.with(|f| f.replace(true)) {
        // Re-entrant call: `tick` can be blocked inside a cross-process
        // SendMessage/SetParent (`workerw::acquire`, `ChildWindow::create`) when
        // Windows delivers a SENT message -- e.g. WM_DPICHANGED -- to this wndproc
        // synchronously (documented anti-deadlock behaviour). Forming a second
        // `&mut AppCore` here would alias the outer call's still-live `&mut AppCore`
        // (UB) and could re-enter the panel rebuild mid-rebuild, corrupting the panel
        // list. Drop the message instead: the outer WM_DISPLAYCHANGE/WM_DPICHANGED
        // handler already rebuilds, and the next tick's `ensure_attached` reconciles
        // whatever this drop missed, so no state is permanently lost.
        return DefWindowProcW(hwnd, msg, wp, lp);
    }

    // SAFETY: `ptr` was stored by `create_controller_window` from `&mut AppCore` on
    // `run`'s stack frame. That frame outlives this window (see the comment in `run`
    // next to `create_controller_window`), and the `IN_WNDPROC` guard above ensures no
    // other live call on this thread already holds a `&mut AppCore` to the same value
    // (it is single-threaded, driven only by this thread's message loop), so a unique
    // `&mut AppCore` here is sound.
    let core = &mut *ptr;

    // `&mut AppCore` is not `UnwindSafe`: it lets code past a caught panic observe
    // whatever partially-mutated state the panicking call left behind. That is
    // precisely what we want to guard against here, not paper over -- if
    // `tick`/`on_display_change` panics partway through, `AppCore` may end up
    // inconsistent (e.g. `last_lines` updated for some panels but not others).
    // `dcomp`'s `SurfaceLock`/`DeviceContextDraw` RAII guards already make sure the
    // lower-level Direct2D/DirectComposition locks are released correctly on
    // unwind, so nothing there is left corrupted; the only cost of proceeding is a
    // possibly-stale next frame, and we log loudly so it is never silent.
    let outcome =
        std::panic::catch_unwind(AssertUnwindSafe(|| handle_message(core, hwnd, msg, wp, lp)));

    // Clear the guard before returning on every path below (normal result or caught
    // panic) so the next, non-reentrant call to wndproc is free to form its own
    // &mut AppCore again.
    IN_WNDPROC.with(|f| f.set(false));

    match outcome {
        Ok(lresult) => lresult,
        Err(payload) => {
            tracing::error!(
                message = panic_message(&*payload),
                "panic in wndproc, message dropped"
            );
            // Do not call DefWindowProcW here: `AppCore` may be half-updated, and
            // DefWindowProcW's default handling is not meaningful for our custom
            // messages (WM_TIMER/WM_DISPLAYCHANGE/WM_DPICHANGED) anyway. Swallowing
            // the message and returning keeps the message loop itself alive.
            LRESULT(0)
        }
    }
}

/// The actual message handling, split out of `wndproc` so `catch_unwind` can wrap
/// it as a single closure without repeating the `unsafe extern "system"` signature.
fn handle_message(core: &mut AppCore, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wp.0 == RELOAD_TIMER_ID => {
            // One-shot: the config settled RELOAD_DEBOUNCE_MS ago.
            unsafe {
                let _ = KillTimer(Some(hwnd), RELOAD_TIMER_ID);
            };
            core.on_config_dirty();
            LRESULT(0)
        }
        WM_TIMER => {
            if let Err(e) = core.tick() {
                tracing::error!("tick failed: {e:#}");
            }
            if core.wants_quit() {
                unsafe { PostQuitMessage(0) };
                return LRESULT(0);
            }
            let next = ms_to_next_second(Zoned::now().subsec_nanosecond() as u32);
            unsafe { SetTimer(Some(hwnd), TIMER_ID, next, None) };
            LRESULT(0)
        }
        // A save is a burst of filesystem events (temp file, rename), and an outside editor
        // may write in several steps. Restarting the timer on each one collapses the burst
        // into a single reload once the writes stop.
        WM_CONFIG_DIRTY => {
            unsafe { SetTimer(Some(hwnd), RELOAD_TIMER_ID, RELOAD_DEBOUNCE_MS, None) };
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            core.on_display_change();
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

#[cfg(test)]
mod tests {
    /// `wndproc` must survive a panic thrown while it holds `&mut AppCore` without
    /// re-panicking or leaving the caller (the Win32 message dispatcher, stood in
    /// for here by this closure) in a bad state. Building a real `HWND`/`AppCore` needs
    /// a live Explorer desktop (WorkerW, monitors, a D3D/D2D device), which is out
    /// of reach for a unit test, so this test instead proves the exact mechanism
    /// `wndproc` relies on -- `catch_unwind(AssertUnwindSafe(...))` over a closure
    /// that mutates a value through `&mut` and then panics -- behaves the same way
    /// `wndproc`'s wrapping does: the panic is caught, does not propagate, and the
    /// mutated value remains usable afterwards.
    #[test]
    fn catch_unwind_keeps_the_message_loop_alive() {
        struct Stub {
            ticks: u32,
        }

        let mut stub = Stub { ticks: 0 };
        let ptr: *mut Stub = &mut stub;

        // Mirrors `wndproc`'s three steps: dereference the raw pointer into `&mut`,
        // wrap the call in `catch_unwind(AssertUnwindSafe(...))`, and never let the
        // panic escape.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let app = unsafe { &mut *ptr };
            app.ticks += 1;
            panic!("simulated panic inside a message handler");
        }));

        assert!(
            outcome.is_err(),
            "the simulated panic should have been caught, not propagated"
        );
        // The message loop (this test function) is still running -- proof that a
        // caught panic does not unwind past the `catch_unwind` boundary. `AppCore`
        // itself may be left mid-update (here, `ticks` was bumped before the
        // panic), which is exactly the "app may be inconsistent, log loudly"
        // scenario `wndproc`'s doc comment describes.
        assert_eq!(stub.ticks, 1);
    }
}
