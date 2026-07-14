//! The wallpaper-layer surfaces: one child window plus one composition surface per
//! enabled monitor, all parented to Explorer's WorkerW.
//!
//! Everything WorkerW-shaped lives here -- finding it, noticing it died, re-acquiring it
//! with backoff. `AppCore` only ever sees `ensure_attached`'s "yes" or "not yet".

use anyhow::Result;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::ID2D1Factory1;

use super::backoff::Backoff;
use super::dcomp::{Compositor, Surface};
use super::render::Painter;
use super::workerw::{self, ChildWindow};
use crate::platform::{Attach, Frame, MonitorInfo};

/// Slow fallback poll cadence once the WorkerW retry budget is exhausted. Keeps
/// `ensure_attached` from calling `workerw::acquire()` (and re-logging the give-up
/// error) on every ~1 Hz tick forever; a WorkerW can still reappear later (e.g. Explorer
/// restarts), so we keep checking, just rarely.
const GIVE_UP_POLL_MS: u64 = 30_000;

/// One monitor's window plus the composition surface that supplies its pixels.
struct Panel {
    window: ChildWindow,
    surface: Surface,
}

pub struct Panels {
    /// Kept so a lost device can be rebuilt without reaching back into `Painter`'s guts.
    d2d: ID2D1Factory1,
    compositor: Compositor,
    workerw: HWND,
    /// Lockstep with `panels`, and with the `frames` slice `draw` is handed.
    monitors: Vec<MonitorInfo>,
    panels: Vec<Panel>,
    backoff: Backoff,
    /// `None` means "retry now"; `Some(at)` means "wait out the backoff until `at`".
    retry_at: Option<std::time::Instant>,
    /// Set once the retry budget is spent and we drop to the slow `GIVE_UP_POLL_MS`
    /// cadence; cleared on a successful re-acquire. Gates the give-up `error!` so it
    /// fires once per outage instead of once per slow poll.
    gave_up: bool,
}

impl Panels {
    /// Takes the `Painter` for its Direct2D factory: the compositor's device and the
    /// painter's device-independent resources have to come from the same factory to be
    /// usable together.
    pub fn new(painter: &Painter) -> Result<Self> {
        let d2d = painter.d2d_factory().clone();
        let compositor = Compositor::new(&d2d)?;
        Ok(Self {
            d2d,
            compositor,
            // Not yet acquired: a WorkerW may not exist yet (e.g. launched at boot,
            // before Explorer is ready). The first `ensure_attached` attempts
            // acquisition and retries with backoff on failure instead of aborting
            // startup.
            workerw: HWND(std::ptr::null_mut()),
            monitors: Vec::new(),
            panels: Vec::new(),
            backoff: Backoff::new(500, 8_000, 60_000),
            retry_at: None,
            gave_up: false,
        })
    }

    /// Whether the wallpaper window is ours to draw on, re-acquiring it (with backoff)
    /// when it is not.
    ///
    /// A WorkerW can die and come back between two ticks -- Explorer restarts, the
    /// wallpaper or theme changes, another wallpaper app pokes Progman -- and the panels
    /// hanging off the old one die with it. `Attach::Fresh` is what tells the caller
    /// that happened; it must rebuild before the next draw, or it will draw nothing at
    /// all, forever.
    ///
    /// `Pending` is "not attached yet", not a failure: the tray shows a warning and the
    /// app keeps running so the user can still quit it.
    pub fn ensure_attached(&mut self) -> Result<Attach> {
        if workerw::is_alive(self.workerw) {
            return Ok(Attach::Live);
        }

        if let Some(at) = self.retry_at {
            if std::time::Instant::now() < at {
                return Ok(Attach::Pending); // still waiting out the backoff
            }
        }

        match workerw::acquire() {
            Ok(hwnd) => {
                tracing::info!("attached to WorkerW");
                self.workerw = hwnd;
                self.backoff.reset();
                self.retry_at = None;
                self.gave_up = false;
                // The old windows were children of a WorkerW that is gone.
                self.monitors.clear();
                self.panels.clear();
                Ok(Attach::Fresh)
            }
            Err(e) => {
                self.retry_at = Some(std::time::Instant::now() + self.retry_delay(&e));
                Ok(Attach::Pending)
            }
        }
    }

    /// How long to wait before the next `acquire` attempt, and the logging that goes
    /// with it. Once the retry budget is spent we drop to a slow, bounded poll cadence
    /// instead of hammering `acquire()` every tick forever, and log the give-up only on
    /// the false->true transition so the (non-rotating) log file does not grow one line
    /// per poll.
    fn retry_delay(&mut self, e: &anyhow::Error) -> std::time::Duration {
        match self.backoff.next_delay_ms() {
            Some(ms) => {
                tracing::warn!("WorkerW not available ({e:#}), retrying in {ms}ms");
                std::time::Duration::from_millis(ms)
            }
            None => {
                if !self.gave_up {
                    tracing::error!("giving up on WorkerW after the retry budget: {e:#}");
                    self.gave_up = true;
                } else {
                    tracing::debug!("still no WorkerW ({e:#}), polling slowly");
                }
                std::time::Duration::from_millis(GIVE_UP_POLL_MS)
            }
        }
    }

    /// Destroys and recreates every window + surface so that they match `wanted`, which
    /// the caller has already filtered down to the monitors the config enables.
    pub fn rebuild(&mut self, wanted: &[MonitorInfo]) -> Result<()> {
        self.panels.clear();
        self.monitors.clear();

        for m in wanted {
            let window = ChildWindow::create(self.workerw)?;
            let surface = self.compositor.attach(window.hwnd())?;
            self.panels.push(Panel { window, surface });
            self.monitors.push(m.clone());
        }
        tracing::info!(count = self.panels.len(), "panels built");
        Ok(())
    }

    pub fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    /// Rebuilds the composition device after a draw failed. The caller must follow this
    /// with a `rebuild`: composition targets are bound to the device that just went away.
    pub fn recover(&mut self) -> Result<()> {
        self.compositor = Compositor::new(&self.d2d)?;
        Ok(())
    }

    /// Paints one pre-composed frame per panel. `frames` is lockstep with `monitors()`.
    pub fn draw(&mut self, painter: &Painter, frames: &[Frame]) -> Result<()> {
        let workerw = self.workerw;
        // Our own windows are not "covering" each other; only a foreign one counts.
        let ours: Vec<HWND> = self.panels.iter().map(|p| p.window.hwnd()).collect();

        for (p, frame) in self.panels.iter_mut().zip(frames) {
            let (w, h) = frame.composed.size();

            p.window.place(workerw, frame.rect)?;
            // Another wallpaper app may have inserted itself above us since last tick.
            p.window.raise_if_covered(workerw, &ours);

            self.compositor.draw(&mut p.surface, w, h, |rt, origin| {
                painter.paint(rt, &frame.composed, &frame.style, origin)
            })?;
        }
        Ok(())
    }
}
