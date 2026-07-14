//! The wallpaper-layer surfaces: one desktop-level `NSWindow` per enabled monitor.
//!
//! The Windows counterpart of this file spends most of itself on WorkerW -- finding it,
//! noticing it died, re-acquiring it with backoff. None of that exists here: a window at
//! the desktop level is the wallpaper layer, and nothing takes it away. `ensure_attached`
//! is correspondingly boring.

use anyhow::{anyhow, Result};
use objc2::MainThreadMarker;

use super::desktop_window::DesktopWindow;
use super::monitors;
use super::render::Painter;
use crate::platform::{Attach, Frame, MonitorInfo};

pub struct Panels {
    mtm: MainThreadMarker,
    /// AppKit's y-up origin, in the y-down space `MonitorInfo.rect` uses. Cached per
    /// rebuild: it changes only when the screen layout does, and a rebuild always follows.
    flip: f64,
    /// Lockstep with `windows`, and with the `frames` slice `draw` is handed.
    monitors: Vec<MonitorInfo>,
    windows: Vec<DesktopWindow>,
    attached: bool,
}

impl Panels {
    pub fn new(_painter: &Painter) -> Result<Self> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| anyhow!("the desktop windows must be built on the main thread"))?;
        Ok(Self {
            mtm,
            flip: monitors::appkit_flip_reference(mtm),
            monitors: Vec::new(),
            windows: Vec::new(),
            attached: false,
        })
    }

    /// `Fresh` once, then `Live` forever.
    ///
    /// There is nothing to attach to and nothing that can come loose, so the only thing
    /// this reports is the very first tick -- which is the one that has to build the
    /// windows. `Pending` never happens, and neither does the tray warning that goes with
    /// it.
    pub fn ensure_attached(&mut self) -> Result<Attach> {
        if self.attached {
            return Ok(Attach::Live);
        }
        self.attached = true;
        Ok(Attach::Fresh)
    }

    pub fn rebuild(&mut self, wanted: &[MonitorInfo]) -> Result<()> {
        self.windows.clear();
        self.monitors.clear();
        // The screen layout may have just moved, which moves AppKit's origin with it.
        self.flip = monitors::appkit_flip_reference(self.mtm);

        for m in wanted {
            let frame = monitors::to_appkit(m.rect, self.flip);
            self.windows.push(DesktopWindow::new(self.mtm, frame)?);
            self.monitors.push(m.clone());
        }
        tracing::info!(count = self.windows.len(), "panels built");
        Ok(())
    }

    pub fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    /// Nothing to recover: there is no composition device here that can be lost, only
    /// CoreAnimation, which does not hand out devices in the first place.
    pub fn recover(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn draw(&mut self, painter: &Painter, frames: &[Frame]) -> Result<()> {
        for ((window, m), frame) in self.windows.iter().zip(&self.monitors).zip(frames) {
            let scale = f64::from(m.scale);
            let canvas = painter.render(&frame.composed, &frame.style, m.scale)?;
            let image = canvas.image()?;
            window.draw(&image, monitors::to_appkit(frame.rect, self.flip), scale);
        }
        Ok(())
    }
}
