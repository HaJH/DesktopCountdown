//! The platform backends, and the contract they share.
//!
//! There is no trait here on purpose. The two backends are picked at compile time and
//! only one ever exists in a build, so this follows the shape of `std`'s own `sys`
//! module: each backend exposes the same names with the same signatures, and `pub use`
//! below re-exports whichever one is active. A trait would have to make `Painter`,
//! `Composed`, and `Panels` generic over associated types -- `IDWriteTextLayout` on
//! Windows, `CTLine` on macOS -- and thread that genericity through `AppCore` and every
//! caller, for no gain: nothing ever needs two backends at once.
//!
//! # The contract
//!
//! Each backend module must expose exactly these items:
//!
//! ```text
//! fn init() -> Result<()>              // process-wide setup, before any window or monitor query
//! fn run(core: AppCore) -> Result<()>  // blocking event loop; ticks `core` once a second
//! fn enumerate_monitors() -> Result<Vec<MonitorInfo>>
//!
//! struct Painter   { fn new() -> Result<Self>;
//!                    fn compose(&self, &[Line], &Style) -> Result<Composed>; }
//! struct Composed  { fn size(&self) -> (u32, u32); }
//!
//! struct Panels    { fn new(&Painter) -> Result<Self>;
//!                    fn ensure_attached(&mut self) -> Result<Attach>;
//!                    fn rebuild(&mut self, wanted: &[MonitorInfo]) -> Result<()>;
//!                    fn monitors(&self) -> &[MonitorInfo];
//!                    fn draw(&mut self, &Painter, &[Frame]) -> Result<()>;
//!                    fn recover(&mut self) -> Result<()>; }
//!
//! struct SingleInstance { fn acquire(name: &str) -> Result<Self>; }
//! struct Tray           { fn new() -> Result<Self>;
//!                         fn poll(&self) -> Option<TrayCommand>;
//!                         fn set_warning(&self, on: bool) -> Result<()>; }
//! struct ConfigWatcher
//! enum   TrayCommand    { Quit, Reload, OpenConfig }
//!
//! mod autostart { fn is_enabled() -> Result<bool>; fn set_enabled(on: bool) -> Result<()>; }
//! mod fonts     { fn system_families() -> Result<Vec<String>>;
//!                 fn font_file(family: &str) -> Option<FontFile>; }
//! ```
//!
//! `ensure_attached` is what keeps WorkerW out of the shared code. On Windows it checks
//! that the wallpaper window is still alive and re-acquires it with backoff when it is
//! not; on macOS an `NSWindow` at the desktop level has nothing to attach to, so it
//! answers `Fresh` once and `Live` forever after. `AppCore` never learns the word
//! "WorkerW" -- only whether it has a surface, and whether that surface is new.

use crate::config::Style;
use crate::layout::Rect;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
compile_error!("no platform backend for this target yet (Windows only; macOS is in progress)");

/// One monitor, as both backends describe it.
///
/// `PartialEq` but not `Eq`: `scale` is a float. Identity comparisons must use `id`.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    /// Stable across reboots and cable swaps. The key for a config override.
    ///
    /// Windows: the device interface name (`\\?\DISPLAY#DEL41A8#...`).
    /// macOS: the display UUID.
    ///
    /// The two formats differ, and that is intended: a `config.toml` shared between the
    /// two systems still applies its global `[style]` and `[[line]]` settings on both,
    /// and only the per-monitor overrides go unmatched on the other one.
    pub id: String,
    /// Display only, e.g. `\\.\DISPLAY1 (2560x1440)`. Never used for identity.
    pub name: String,
    /// Virtual-desktop coordinates in physical pixels. May be negative.
    pub rect: Rect,
    /// Windows: dpi / 96.0. macOS: `backingScaleFactor`.
    pub scale: f32,
}

/// What `Panels::ensure_attached` found.
///
/// A bool cannot carry this: "still attached" and "attached again, just now" both mean
/// yes, but only the second obliges the caller to rebuild -- the backend dropped the old
/// panels along with the surface they hung from. Telling them apart with an `attached`
/// flag on the caller's side does not work either, because a WorkerW can die and be
/// re-acquired inside a single tick, so the caller never observes the "not attached"
/// state in between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attach {
    /// Attached to the same surface as last tick. The panels are still good.
    Live,
    /// Attached again, just now: the panels went with the old surface and the caller
    /// must rebuild them before anything can be drawn.
    Fresh,
    /// Not attached. The backend is retrying with backoff; nothing can be drawn.
    Pending,
}

/// One panel's worth of work for `Panels::draw`: what to draw, how, and where.
///
/// Built by `AppCore` (which owns the config and the layout maths) and consumed by the
/// backend (which owns the pixels). `Composed` is the backend's own laid-out-text type,
/// so this struct is only nameable once a backend is selected.
pub struct Frame {
    pub composed: Composed,
    pub style: Style,
    pub rect: Rect,
}
