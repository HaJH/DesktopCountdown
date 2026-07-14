//! The macOS backend: an `NSWindow` at the desktop level per monitor, painted with
//! CoreText/CoreGraphics and handed to a `CALayer`.
//!
//! # Status
//!
//! The renderer and the font picker's font loading are real (design §9 step 3; `fonts`
//! came forward from step 6 because the shared settings-window tests call it, and a red
//! test suite is worse than a step done early). Everything else -- the desktop window, the
//! monitors, the tray, autostart, the config watcher, the event loop -- is a stub that
//! fails loudly, and lands in steps 4 through 7. `cargo test` exercises what is real; the
//! app itself does not run on macOS yet, and says so instead of misbehaving quietly.
//!
//! The spike (`docs/superpowers/plans/macos-spike-result.md`) has already proved the
//! window settings the stubs will use.

pub mod fonts;
mod render;

use std::path::Path;

use anyhow::{bail, Result};

use crate::app::AppCore;
use crate::platform::{Attach, Frame, MonitorInfo};

pub use render::{Canvas, Composed, Painter};

/// What every unimplemented backend call says, so a stub is never mistaken for a bug.
macro_rules! not_yet {
    ($what:literal, $step:literal) => {
        bail!(concat!(
            "the macOS backend cannot ",
            $what,
            " yet (design section 9, step ",
            $step,
            ")"
        ))
    };
}

/// Windows sets its DPI awareness here. macOS has nothing to set: `backingScaleFactor` is
/// per-screen and read at layout time.
pub fn init() -> Result<()> {
    Ok(())
}

pub fn run(_core: AppCore) -> Result<()> {
    not_yet!("run the event loop", "4")
}

pub fn enumerate_monitors() -> Result<Vec<MonitorInfo>> {
    not_yet!("enumerate monitors", "5")
}

pub struct Panels;

impl Panels {
    pub fn new(_painter: &Painter) -> Result<Self> {
        not_yet!("build the desktop windows", "4")
    }

    /// A desktop-level `NSWindow` has nothing to attach to -- no WorkerW, nothing that can
    /// die under it -- so once the windows exist this answers `Fresh` on the first tick and
    /// `Live` forever after.
    pub fn ensure_attached(&mut self) -> Result<Attach> {
        not_yet!("attach", "4")
    }

    pub fn rebuild(&mut self, _wanted: &[MonitorInfo]) -> Result<()> {
        not_yet!("rebuild the panels", "4")
    }

    pub fn monitors(&self) -> &[MonitorInfo] {
        &[]
    }

    /// Nothing to recover: there is no composition device to lose.
    pub fn recover(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn draw(&mut self, _painter: &Painter, _frames: &[Frame]) -> Result<()> {
        not_yet!("draw", "4")
    }
}

pub struct SingleInstance;

impl SingleInstance {
    pub fn acquire(_name: &str) -> Result<Self> {
        not_yet!("take the single-instance lock", "6")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    OpenConfig,
    Reload,
    Quit,
}

pub struct Tray;

impl Tray {
    pub fn new() -> Result<Self> {
        not_yet!("create the menu bar item", "6")
    }

    pub fn poll(&self) -> Option<TrayCommand> {
        None
    }

    pub fn set_warning(&self, _on: bool) -> Result<()> {
        Ok(())
    }
}

pub struct ConfigWatcher;

impl ConfigWatcher {
    pub fn new(_path: &Path) -> Result<Self> {
        not_yet!("watch the config file", "6")
    }
}

pub mod autostart {
    use anyhow::{bail, Result};

    pub fn is_enabled() -> Result<bool> {
        not_yet!("read the launch agent", "6")
    }

    pub fn set_enabled(_on: bool) -> Result<()> {
        not_yet!("write the launch agent", "6")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stubs must not be mistaken for working code: each one has to fail, and say why.
    #[test]
    fn the_unimplemented_backend_calls_fail_loudly() {
        let e = enumerate_monitors().unwrap_err().to_string();
        assert!(e.contains("macOS backend"), "unhelpful message: {e}");
        assert!(e.contains("step"), "the message should name the step: {e}");

        assert!(Tray::new().is_err());
        assert!(SingleInstance::acquire("x").is_err());
        assert!(autostart::is_enabled().is_err());
    }

    /// The renderer, on the other hand, is real.
    #[test]
    fn the_painter_is_not_a_stub() {
        assert!(Painter::new().is_ok());
    }
}
