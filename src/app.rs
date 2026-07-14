//! The countdown's state and logic, with nothing platform-specific in it.
//!
//! The event loop lives in the platform backend and calls in here: `tick` once a second,
//! `on_config_dirty` when the watcher reports a write, `on_display_change` when the
//! monitor layout moves. Everything those need -- the wallpaper surfaces, the tray, the
//! text compositor -- is reached through `crate::platform`, which resolves to whichever
//! backend this build targets.

use std::path::PathBuf;

use anyhow::Result;
use jiff::Zoned;

use crate::config::{effective_for, Config, Line};
use crate::countdown::{breakdown, daily_breakdown, Breakdown, DailyBreakdown};
use crate::layout::place;
use crate::platform::{
    self, autostart, Attach, ConfigWatcher, Frame, MonitorInfo, Painter, Panels, Tray, TrayCommand,
};
use crate::tokens;

/// Never let the tick timer fire immediately (which would spin) nor sleep past a tick.
const MIN_TIMER_MS: u32 = 20;

/// Milliseconds until the next whole second, clamped so the timer never fires
/// immediately (which would spin) nor sleeps past a tick.
pub fn ms_to_next_second(subsec_nanos: u32) -> u32 {
    let remaining_ms = 1000u32.saturating_sub(subsec_nanos / 1_000_000);
    remaining_ms.clamp(MIN_TIMER_MS, 1000)
}

pub struct AppCore {
    cfg_path: PathBuf,
    cfg: Config,
    target: Zoned,
    watcher: ConfigWatcher,
    painter: Painter,
    panels: Panels,
    tray: Tray,
    /// The lines actually drawn last time, per panel (a monitor override can give a monitor
    /// its own list). Compared against the freshly resolved ones to skip a redraw that would
    /// paint the same pixels.
    last_lines: Option<Vec<Vec<Line>>>,
    /// What the tray tooltip currently says, so `warn` can skip the call when nothing
    /// changed. `tick` asks for the warning on every tick of an outage, and the tray's
    /// setter is a `Shell_NotifyIcon` round trip plus a log line -- once per second, for
    /// as long as the outage lasts, neither of which anyone needs.
    warned: bool,
    quit: bool,
    /// The settings window this process spawned, while it is still running.
    ///
    /// Kept so quitting from the tray takes the settings window with it. Without the handle
    /// the child is simply detached, and "quit" would leave a settings window editing the
    /// config of an app that is no longer drawing it.
    ///
    /// A settings window the user started some other way (a second copy of the exe, a
    /// shortcut with `--settings`) is not ours and is left alone -- it holds its own
    /// single-instance lock, so there is never more than one either way.
    settings: Option<std::process::Child>,
}

impl AppCore {
    pub fn new(cfg_path: PathBuf) -> Result<Self> {
        let cfg = crate::config::load_or_create(&cfg_path)?;
        if let Err(e) = autostart::set_enabled(cfg.general.autostart) {
            tracing::error!("autostart update failed: {e:#}");
        }
        let target = cfg.target.to_zoned(jiff::tz::TimeZone::system())?;
        // Borrows `cfg_path` to find its parent directory, so build it before `cfg_path`
        // is moved into `AppCore` below.
        let watcher = ConfigWatcher::new(&cfg_path)?;

        let painter = Painter::new()?;
        let panels = Panels::new(&painter)?;
        let tray = Tray::new()?;
        tracing::info!("tray icon created");

        Ok(Self {
            cfg_path,
            cfg,
            target,
            watcher,
            painter,
            panels,
            tray,
            last_lines: None,
            // `Tray::new` starts with the plain tooltip, so we start in step with it.
            warned: false,
            quit: false,
            settings: None,
        })
    }

    /// The backend needs this to point the watcher at whatever it uses to wake itself.
    pub fn watcher(&self) -> &ConfigWatcher {
        &self.watcher
    }

    /// Set by a tray "quit"; the event loop checks it after every `tick`.
    pub fn wants_quit(&self) -> bool {
        self.quit
    }

    /// Shows or clears the tray's warning marker, skipping the call when the tooltip
    /// already says what we want it to. A failure here is not worth failing a tick over:
    /// the tooltip is a hint, and the log already carries the real error.
    fn warn(&mut self, on: bool) {
        if self.warned == on {
            return;
        }
        if let Err(e) = self.tray.set_warning(on) {
            tracing::warn!("updating the tray tooltip failed: {e:#}");
        }
        self.warned = on;
    }

    /// Opens the settings window, keeping the handle so `close_settings` can take it down
    /// again. Spawning a second one while the first is up is harmless -- it exits at once on
    /// the settings single-instance lock -- but the handle we would store for it would be a
    /// process that is already gone, so don't.
    fn open_settings(&mut self) {
        if self.settings.is_some() {
            return;
        }
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                tracing::error!("current_exe failed: {e:#}");
                return;
            }
        };
        match std::process::Command::new(exe).arg("--settings").spawn() {
            Ok(child) => self.settings = Some(child),
            Err(e) => tracing::error!("opening the settings window failed: {e:#}"),
        }
    }

    /// Takes our settings window down with us. Quitting an app that leaves a settings window
    /// behind -- still editing the config of something that is no longer drawing it -- is not
    /// what "quit" means.
    ///
    /// This kills rather than asks. There is no portable way to ask another process's GUI to
    /// close itself, and a per-platform one buys little here: the settings window writes
    /// config.toml on a 100 ms throttle *while* you edit (`SAVE_INTERVAL_MS`), so what a kill
    /// can lose is under one throttle interval of a slider drag -- the same window the
    /// throttle already tolerates.
    fn close_settings(&mut self) {
        let Some(mut child) = self.settings.take() else {
            return;
        };
        // It may have exited since the last `try_wait`. Killing a process that is already
        // gone is an error rather than a no-op, and one worth no log line.
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        if let Err(e) = child.kill() {
            tracing::warn!("could not close the settings window: {e:#}");
            return;
        }
        let _ = child.wait();
        tracing::info!("settings window closed with the app");
    }

    /// Runs once a second: drains the tray, makes sure the surfaces are attached, and
    /// redraws only when the countdown text changed.
    pub fn tick(&mut self) -> Result<()> {
        // A settings window the user closed themselves is not ours to kill any more, and on
        // Unix it stays a zombie until someone reaps it.
        if let Some(child) = &mut self.settings {
            if !matches!(child.try_wait(), Ok(None)) {
                self.settings = None;
            }
        }

        match self.tray.poll() {
            Some(TrayCommand::Quit) => {
                self.close_settings();
                self.quit = true;
                return Ok(());
            }
            Some(TrayCommand::Reload) => self.reload(),
            Some(TrayCommand::OpenConfig) => self.open_settings(),
            None => {}
        }

        match self.panels.ensure_attached()? {
            Attach::Pending => {
                self.warn(true);
                return Ok(()); // keep the tray alive so the user can quit
            }
            // The tick we attach on is the tick the panels have to be built on: the
            // backend dropped them along with the surface they hung from.
            Attach::Fresh => {
                self.rebuild_panels()?;
                self.warn(false);
            }
            Attach::Live => {}
        }

        self.render()
    }

    /// Re-reads the config and repaints with it, in one go: the whole point of the
    /// watcher's wake-up is that an edit in the settings window shows up now, not on
    /// the next tick.
    pub fn on_config_dirty(&mut self) {
        self.reload();
        if let Err(e) = self.render() {
            tracing::error!("redraw after a config reload failed: {e:#}");
        }
    }

    pub fn on_display_change(&mut self) {
        tracing::info!("display configuration changed");
        if let Err(e) = self.rebuild_panels() {
            tracing::error!("rebuilding panels failed: {e:#}");
        }
    }

    /// Destroys and recreates every monitor's surface, e.g. after a display topology or
    /// DPI change. Resets `last_lines` so the very next tick redraws even if the
    /// countdown text happens to be unchanged.
    fn rebuild_panels(&mut self) -> Result<()> {
        let mut wanted = Vec::new();
        for m in platform::enumerate_monitors()? {
            if self.enabled_on(&m) {
                wanted.push(m);
            } else {
                tracing::info!(monitor = %m.name, "disabled by config");
            }
        }
        // Before, not after: `rebuild` can fail partway, and every surface it did manage
        // to create is brand new and unpainted. A surviving `last_lines` would let the
        // next `render` decide nothing changed and skip them -- a monitor left blank
        // until the countdown text happens to tick over, or forever if the line has no
        // tokens in it.
        self.last_lines = None;
        self.panels.rebuild(&wanted)?;
        Ok(())
    }

    fn enabled_on(&self, m: &MonitorInfo) -> bool {
        effective_for(&self.cfg, &m.id).enabled
    }

    /// Re-reads `config.toml` after the watcher reports a change. A malformed or
    /// out-of-range save must never blank the screen (design §6), so on failure the
    /// previous config is kept untouched and only an error is logged.
    fn reload(&mut self) {
        match crate::config::load_or_create(&self.cfg_path) {
            Ok(new_cfg) => {
                let target_changed = new_cfg.target != self.cfg.target;

                self.cfg = new_cfg;
                if target_changed {
                    match self.cfg.target.to_zoned(jiff::tz::TimeZone::system()) {
                        Ok(z) => self.target = z,
                        Err(e) => tracing::error!("bad target: {e:#}"),
                    }
                }
                if self.panels_are_stale() {
                    if let Err(e) = self.rebuild_panels() {
                        tracing::error!("rebuilding panels failed: {e:#}");
                    }
                }
                self.last_lines = None; // force a redraw with the new style
                self.warn(false);
                if let Err(e) = autostart::set_enabled(self.cfg.general.autostart) {
                    tracing::error!("autostart update failed: {e:#}");
                }
                tracing::info!("config reloaded");
            }
            // Keeping the last valid config beats blanking the screen.
            Err(e) => {
                tracing::error!("config reload rejected, keeping previous: {e:#}");
                self.warn(true);
            }
        }
    }

    /// Whether `self.cfg` now wants panels on a different set of monitors than we have.
    ///
    /// Only *that* justifies `rebuild_panels`, which destroys and recreates every window
    /// and composition target. Style, layout, and target edits all just change what the
    /// next `render` draws and where it places an existing window -- rebuilding for those
    /// would tear the windows down and back up on every frame of a settings slider drag.
    ///
    /// Silent, unlike `rebuild_panels`: this runs on every reload, and a settings-window
    /// slider drag reloads several times a second.
    fn panels_are_stale(&self) -> bool {
        let monitors = match platform::enumerate_monitors() {
            Ok(ms) => ms,
            // Cannot tell: leave the panels alone rather than rebuild them blindly.
            Err(e) => {
                tracing::error!("enumerating monitors failed: {e:#}");
                return false;
            }
        };
        let wanted = monitors
            .iter()
            .filter(|m| self.enabled_on(m))
            .map(|m| m.id.as_str());
        let have = self.panels.monitors().iter().map(|m| m.id.as_str());
        !wanted.eq(have)
    }

    /// Computes the current countdown text and redraws only when it changed since the
    /// last time. Split out of `tick` so a config reload can repaint straight away
    /// instead of waiting out the rest of the current second.
    fn render(&mut self) -> Result<()> {
        let now = Zoned::now();
        let b = breakdown(&now, &self.target);
        let d = daily_breakdown(&now, self.cfg.daily_target);
        let resolved = self.resolve(&b, &d);

        if self.last_lines.as_ref() == Some(&resolved) {
            return Ok(());
        }

        tracing::debug!(?resolved, "render");
        if let Err(e) = self.draw(&resolved) {
            tracing::warn!("draw failed, recreating the compositor: {e:#}");
            self.panels.recover()?;
            // Composition targets are bound to the device that just went away, so the
            // panels have to be rebuilt too -- and that can change how many there are,
            // so the lines must be resolved against the new set. `draw` walks panels and
            // frames in lockstep.
            self.rebuild_panels()?;
            let resolved = self.resolve(&b, &d);
            self.draw(&resolved)?; // one retry; a second failure propagates
            self.last_lines = Some(resolved);
            return Ok(());
        }
        self.last_lines = Some(resolved);
        Ok(())
    }

    /// One line list per panel, with the templates substituted. Per panel and not once for
    /// all of them because a monitor override can carry its own list.
    fn resolve(&self, b: &Breakdown, d: &DailyBreakdown) -> Vec<Vec<Line>> {
        self.panels
            .monitors()
            .iter()
            .map(|m| {
                effective_for(&self.cfg, &m.id)
                    .lines
                    .into_iter()
                    .map(|l| Line {
                        text: tokens::render(&l.text, b, d),
                        ..l
                    })
                    .collect()
            })
            .collect()
    }

    /// Lays every panel's text out, works out where on its monitor it goes, and hands the
    /// lot to the backend to paint.
    fn draw(&mut self, resolved: &[Vec<Line>]) -> Result<()> {
        let mut frames = Vec::with_capacity(resolved.len());

        for (m, lines) in self.panels.monitors().iter().zip(resolved) {
            let eff = effective_for(&self.cfg, &m.id);
            // Laying the text out is the expensive half of a redraw (glyph outlines, see
            // `render::ink_span`); do it once and let both the sizing and the painting
            // below read from it.
            let composed = self.painter.compose(lines, &eff.style)?;
            let (w, h) = composed.size();
            let rect = place(m.rect, w as i32, h as i32, eff.anchor, eff.offset_px);
            frames.push(Frame {
                composed,
                style: eff.style,
                rect,
            });
        }

        self.panels.draw(&self.painter, &frames)
    }
}

impl Drop for AppCore {
    /// The safety net for `close_settings`: whatever brings the app down -- the tray's quit,
    /// which calls it directly, or an exit that never reaches that arm -- the settings window
    /// this process opened goes with it. `close_settings` takes the handle, so running twice
    /// is not a problem.
    fn drop(&mut self) {
        self.close_settings();
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
}
