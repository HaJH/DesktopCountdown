//! Ties everything together: a hidden top-level controller window owns the timer,
//! receives system messages, and drives the layered child windows.

use std::any::Any;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;

use anyhow::Result;
use jiff::Zoned;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::backoff::Backoff;
use crate::config::{effective_for, Config};
use crate::countdown::{breakdown, format_main, format_summary};
use crate::dcomp::{Compositor, Surface};
use crate::layout::place;
use crate::monitors::{self, MonitorInfo};
use crate::render::{Lines, Painter};
use crate::tray::{Tray, TrayCommand};
use crate::watch::ConfigWatcher;
use crate::workerw::{self, ChildWindow};

const CTRL_CLASS: PCWSTR = w!("DesktopCountdownController");
const TIMER_ID: usize = 1;
const MIN_TIMER_MS: u32 = 20;
/// Slow fallback poll cadence once the WorkerW retry budget is exhausted. Keeps
/// `tick()` from calling `workerw::acquire()` (and re-logging the give-up error)
/// on every ~1 Hz tick forever; a WorkerW can still reappear later (e.g. Explorer
/// restarts), so we keep checking, just rarely.
const GIVE_UP_POLL_MS: u64 = 30_000;

/// Milliseconds until the next whole second, clamped so the timer never fires
/// immediately (which would spin) nor sleeps past a tick.
fn ms_to_next_second(subsec_nanos: u32) -> u32 {
    let remaining_ms = 1000u32.saturating_sub(subsec_nanos / 1_000_000);
    remaining_ms.clamp(MIN_TIMER_MS, 1000)
}

/// One monitor's window plus the composition surface that supplies its pixels.
struct Panel {
    monitor: MonitorInfo,
    window: ChildWindow,
    surface: Surface,
}

pub struct App {
    cfg_path: PathBuf,
    cfg: Config,
    target: Zoned,
    watcher: ConfigWatcher,
    painter: Painter,
    compositor: Compositor,
    workerw: HWND,
    panels: Vec<Panel>,
    last_lines: Option<Lines>,
    workerw_backoff: Backoff,
    /// `None` means "retry now"; `Some(at)` means "wait out the backoff until `at`".
    retry_at: Option<std::time::Instant>,
    /// Set once the retry budget is spent and we drop to the slow `GIVE_UP_POLL_MS`
    /// cadence; cleared on a successful re-acquire. Gates the give-up `error!` so it
    /// fires once per outage instead of once per slow poll.
    workerw_gave_up: bool,
    tray: Tray,
}

impl App {
    /// Builds the app, creates the hidden controller window, and blocks in the
    /// Win32 message loop until `WM_QUIT`.
    pub fn run(cfg_path: PathBuf) -> Result<()> {
        let cfg = crate::config::load_or_create(&cfg_path)?;
        if let Err(e) = crate::autostart::set_enabled(cfg.general.autostart) {
            tracing::error!("autostart update failed: {e:#}");
        }
        let target = cfg.target.to_zoned(jiff::tz::TimeZone::system())?;
        // Borrows `cfg_path` to find its parent directory, so build it before
        // `cfg_path` is moved into `App` below.
        let watcher = ConfigWatcher::new(&cfg_path)?;

        let painter = Painter::new()?;
        let compositor = Compositor::new(painter.d2d_factory())?;

        let mut app = App {
            cfg_path,
            cfg,
            target,
            watcher,
            painter,
            compositor,
            // Not yet acquired: a WorkerW may not exist yet (e.g. launched at boot,
            // before Explorer is ready). The first `tick()` attempts acquisition and
            // retries with backoff on failure instead of aborting startup.
            workerw: HWND(std::ptr::null_mut()),
            panels: Vec::new(),
            last_lines: None,
            workerw_backoff: Backoff::new(500, 8_000, 60_000),
            retry_at: None,
            workerw_gave_up: false,
            tray: Tray::new()?,
        };
        tracing::info!("tray icon created");

        // `app` must not move for as long as `hwnd` exists: `create_controller_window`
        // stores `&mut app`'s address in GWLP_USERDATA, and `wndproc` dereferences it
        // on every message. `app` stays on this stack frame for the rest of `run`, and
        // nothing below moves or drops it before the message loop (and thus the window)
        // is torn down, so the pointer stays valid for its whole lifetime.
        let hwnd = create_controller_window(&mut app)?;
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

    /// Destroys and recreates every monitor's child window + surface, e.g. after a
    /// display topology or DPI change. Resets `last_lines` so the very next tick
    /// redraws even if the countdown text happens to be unchanged.
    fn rebuild_panels(&mut self) -> Result<()> {
        self.panels.clear();
        self.last_lines = None;

        for m in monitors::enumerate()? {
            let eff = effective_for(&self.cfg, &m.id);
            if !eff.enabled {
                tracing::info!(monitor = %m.name, "disabled by config");
                continue;
            }
            let window = ChildWindow::create(self.workerw)?;
            let surface = self.compositor.attach(window.hwnd())?;
            self.panels.push(Panel {
                monitor: m,
                window,
                surface,
            });
        }
        tracing::info!(count = self.panels.len(), "panels built");
        Ok(())
    }

    /// Re-reads `config.toml` after the watcher reports a settled change. A malformed
    /// or out-of-range save must never blank the screen (design §6), so on failure the
    /// previous config is kept untouched and only an error is logged.
    fn reload(&mut self) {
        match crate::config::load_or_create(&self.cfg_path) {
            Ok(new_cfg) => {
                let displays_changed =
                    new_cfg.displays != self.cfg.displays || new_cfg.layout != self.cfg.layout;
                let target_changed = new_cfg.target != self.cfg.target;

                self.cfg = new_cfg;
                if target_changed {
                    match self.cfg.target.to_zoned(jiff::tz::TimeZone::system()) {
                        Ok(z) => self.target = z,
                        Err(e) => tracing::error!("bad target: {e:#}"),
                    }
                }
                if displays_changed {
                    if let Err(e) = self.rebuild_panels() {
                        tracing::error!("rebuilding panels failed: {e:#}");
                    }
                }
                self.last_lines = None; // force a redraw with the new style
                let _ = self.tray.set_warning(false);
                if let Err(e) = crate::autostart::set_enabled(self.cfg.general.autostart) {
                    tracing::error!("autostart update failed: {e:#}");
                }
                tracing::info!("config reloaded");
            }
            // Keeping the last valid config beats blanking the screen.
            Err(e) => {
                tracing::error!("config reload rejected, keeping previous: {e:#}");
                let _ = self.tray.set_warning(true);
            }
        }
    }

    /// Runs once per `WM_TIMER`: checks WorkerW health, computes the current
    /// countdown text, and redraws only when it changed since the last tick.
    fn tick(&mut self) -> Result<()> {
        match self.tray.poll() {
            Some(TrayCommand::Quit) => {
                unsafe { PostQuitMessage(0) };
                return Ok(());
            }
            Some(TrayCommand::Reload) => self.reload(),
            Some(TrayCommand::OpenConfig) => {
                if let Err(e) = std::process::Command::new("notepad.exe")
                    .arg(&self.cfg_path)
                    .spawn()
                {
                    tracing::error!("opening the config failed: {e:#}");
                }
            }
            None => {}
        }

        if self.watcher.changed() {
            self.reload();
        }

        if !workerw::is_alive(self.workerw) {
            if let Some(at) = self.retry_at {
                if std::time::Instant::now() < at {
                    return Ok(()); // still waiting out the backoff
                }
            }
            match workerw::acquire() {
                Ok(hwnd) => {
                    tracing::info!("attached to WorkerW");
                    self.workerw = hwnd;
                    self.workerw_backoff.reset();
                    self.retry_at = None;
                    self.workerw_gave_up = false;
                    self.rebuild_panels()?;
                    let _ = self.tray.set_warning(false);
                }
                Err(e) => {
                    let _ = self.tray.set_warning(true);
                    return match self.workerw_backoff.next_delay_ms() {
                        Some(ms) => {
                            tracing::warn!("WorkerW not available ({e:#}), retrying in {ms}ms");
                            self.retry_at = Some(
                                std::time::Instant::now() + std::time::Duration::from_millis(ms),
                            );
                            Ok(())
                        }
                        None => {
                            // Retry budget spent: drop to a slow, bounded poll cadence
                            // instead of hammering `acquire()` every tick forever. Log
                            // the give-up only on the false->true transition so the
                            // (non-rotating) log file does not grow one line per poll.
                            if !self.workerw_gave_up {
                                tracing::error!(
                                    "giving up on WorkerW after the retry budget: {e:#}"
                                );
                                self.workerw_gave_up = true;
                            } else {
                                tracing::debug!("still no WorkerW ({e:#}), polling slowly");
                            }
                            self.retry_at = Some(
                                std::time::Instant::now()
                                    + std::time::Duration::from_millis(GIVE_UP_POLL_MS),
                            );
                            Ok(()) // keep the tray alive so the user can quit
                        }
                    };
                }
            }
        }

        let now = Zoned::now();
        let b = breakdown(&now, &self.target);
        let lines = Lines {
            summary: Some(format_summary(&b)),
            main: format_main(&b),
        };

        if self.last_lines.as_ref() == Some(&lines) {
            return Ok(());
        }

        tracing::debug!(summary = ?lines.summary, main = %lines.main, "tick");
        if let Err(e) = self.draw(&lines) {
            tracing::warn!("draw failed, recreating the compositor: {e:#}");
            self.compositor = Compositor::new(self.painter.d2d_factory())?;
            self.rebuild_panels()?; // targets are bound to HWNDs; rebuild them too
            self.draw(&lines)?; // one retry; a second failure propagates
        }
        self.last_lines = Some(lines);
        Ok(())
    }

    fn draw(&mut self, lines: &Lines) -> Result<()> {
        let workerw = self.workerw;
        let cfg = &self.cfg;
        let painter = &self.painter;

        for p in &mut self.panels {
            let eff = effective_for(cfg, &p.monitor.id);
            let (w, h) = painter.measure(lines, &eff.style)?;
            let rect = place(
                p.monitor.rect,
                w as i32,
                h as i32,
                eff.anchor,
                eff.offset_px,
            );

            p.window.place(workerw, rect)?;
            // Another wallpaper app may have inserted itself above us since last tick.
            p.window.raise_if_covered(workerw);

            self.compositor.draw(&mut p.surface, w, h, |rt, origin| {
                painter.paint(rt, lines, &eff.style, origin)
            })?;
        }
        Ok(())
    }

    fn on_display_change(&mut self) {
        tracing::info!("display configuration changed");
        if let Err(e) = self.rebuild_panels() {
            tracing::error!("rebuilding panels failed: {e:#}");
        }
    }
}

fn create_controller_window(app: &mut App) -> Result<HWND> {
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

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as *mut App as isize);
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

/// The window procedure for the hidden controller window.
///
/// # Safety (panics)
///
/// This function is `extern "system"`: it is called directly by the Win32 message
/// dispatcher, which is C code with no concept of Rust unwinding. Letting a panic
/// unwind out of an `extern "system"` function is undefined behaviour (Rust aborts
/// the process in the best case; in the worst case it corrupts the dispatcher's own
/// stack). `tick`/`draw`/`on_display_change` call into Direct2D, DirectWrite, and
/// DirectComposition through fallible APIs, but a panic (an out-of-bounds slice
/// index, an `.expect()`, an arithmetic overflow in a debug build, ...) is still
/// possible anywhere below. The whole body below is therefore wrapped in
/// `catch_unwind` so a panic never crosses back into `DispatchMessageW`.
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if ptr.is_null() {
        // No `App` attached yet (messages before `SetWindowLongPtrW`, if any) --
        // nothing to panic on, so this path does not need catch_unwind.
        return DefWindowProcW(hwnd, msg, wp, lp);
    }

    // SAFETY: `ptr` was stored by `create_controller_window` from `&mut App` on
    // `App::run`'s stack frame. That frame outlives this window (see the comment
    // in `App::run` next to `create_controller_window`), and no other code holds a
    // reference to the same `App` while `wndproc` runs (it is single-threaded,
    // driven only by this thread's message loop), so a unique `&mut App` here is sound.
    let app = &mut *ptr;

    // `&mut App` is not `UnwindSafe`: it lets code past a caught panic observe
    // whatever partially-mutated state the panicking call left behind. That is
    // precisely what we want to guard against here, not paper over -- if
    // `tick`/`draw`/`on_display_change` panics partway through, `App` may end up
    // inconsistent (e.g. `last_lines` updated for some panels but not others).
    // `dcomp`'s `SurfaceLock`/`DeviceContextDraw` RAII guards already make sure the
    // lower-level Direct2D/DirectComposition locks are released correctly on
    // unwind, so nothing there is left corrupted; the only cost of proceeding is a
    // possibly-stale next frame, and we log loudly so it is never silent.
    let outcome =
        std::panic::catch_unwind(AssertUnwindSafe(|| handle_message(app, hwnd, msg, wp, lp)));

    match outcome {
        Ok(lresult) => lresult,
        Err(payload) => {
            tracing::error!(
                message = panic_message(&*payload),
                "panic in wndproc, message dropped"
            );
            // Do not call DefWindowProcW here: `App` may be half-updated, and
            // DefWindowProcW's default handling is not meaningful for our custom
            // messages (WM_TIMER/WM_DISPLAYCHANGE/WM_DPICHANGED) anyway. Swallowing
            // the message and returning keeps the message loop itself alive.
            LRESULT(0)
        }
    }
}

/// The actual message handling, split out of `wndproc` so `catch_unwind` can wrap
/// it as a single closure without repeating the `unsafe extern "system"` signature.
fn handle_message(app: &mut App, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            if let Err(e) = app.tick() {
                tracing::error!("tick failed: {e:#}");
            }
            let next = ms_to_next_second(Zoned::now().subsec_nanosecond() as u32);
            unsafe { SetTimer(Some(hwnd), TIMER_ID, next, None) };
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            app.on_display_change();
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
    use super::ms_to_next_second;

    #[test]
    fn just_after_a_boundary_waits_almost_a_full_second() {
        assert_eq!(ms_to_next_second(1_000_000), 999); // 1ms past the boundary
    }

    #[test]
    fn just_before_a_boundary_waits_the_minimum() {
        // 999.9ms past the boundary would round to 0; we clamp to MIN_TIMER_MS.
        assert_eq!(ms_to_next_second(999_900_000), 20);
    }

    #[test]
    fn exactly_on_a_boundary_waits_a_full_second() {
        assert_eq!(ms_to_next_second(0), 1000);
    }

    #[test]
    fn never_exceeds_one_second() {
        for ns in [0, 1, 500_000_000, 999_999_999] {
            assert!(ms_to_next_second(ns) <= 1000);
            assert!(ms_to_next_second(ns) >= 20);
        }
    }

    /// `wndproc` must survive a panic thrown while it holds `&mut App` without
    /// re-panicking or leaving the caller (the Win32 message dispatcher, stood in
    /// for here by this closure) in a bad state. Building a real `HWND`/`App` needs
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
        // caught panic does not unwind past the `catch_unwind` boundary. `App`
        // itself may be left mid-update (here, `ticks` was bumped before the
        // panic), which is exactly the "app may be inconsistent, log loudly"
        // scenario `wndproc`'s doc comment describes.
        assert_eq!(stub.ticks, 1);
    }
}
