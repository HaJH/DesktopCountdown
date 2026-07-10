//! Settings window state and save logic. The eframe UI lives in `render_ui` (Task 7).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::config::{self, Config};
use crate::monitors;

const DEBOUNCE_MS: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Global,
    Monitor(usize),
}

#[derive(Debug, Clone)]
pub struct MonitorRef {
    pub id: String,
    pub name: String,
}

pub struct SettingsApp {
    pub cfg: Config,
    pub target: Target,
    pub monitors: Vec<MonitorRef>,
    pub fonts: Vec<String>,
    pub(crate) dirty: bool,
    pub(crate) last_change_ms: u64,
    pub(crate) cfg_path: PathBuf,
    pub(crate) error: Option<String>,
}

impl SettingsApp {
    pub fn new() -> Result<Self> {
        let cfg_path = crate::paths::config_path()?;
        let cfg = config::load_or_create(&cfg_path)?;
        let monitors = monitors::enumerate()
            .unwrap_or_default()
            .into_iter()
            .map(|m| MonitorRef {
                id: m.id,
                name: m.name,
            })
            .collect();
        let fonts = crate::fonts::system_families().unwrap_or_default();
        Ok(Self {
            cfg,
            target: Target::Global,
            monitors,
            fonts,
            dirty: false,
            last_change_ms: 0,
            cfg_path,
            error: None,
        })
    }

    /// Milliseconds since the Unix epoch, used as the debounce clock. Wall-clock based
    /// (rather than a stored `Instant` origin) so the struct stays plain-data and matches
    /// the fields the tests construct directly.
    pub fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_change_ms = self.now_ms();
    }

    /// Saves if dirty and the debounce window has elapsed. Invalid configs are not
    /// written; the error is surfaced instead. Never blanks the file.
    pub fn save_if_due(&mut self, now_ms: u64) {
        if !crate::settings::widgets::should_save(
            self.dirty,
            now_ms.saturating_sub(self.last_change_ms),
            DEBOUNCE_MS,
        ) {
            return;
        }
        self.write();
    }

    /// Forces a save of any pending change, ignoring the debounce (used on window close).
    pub fn flush(&mut self) {
        if self.dirty {
            self.write();
        }
    }

    fn write(&mut self) {
        match config::validate(&self.cfg) {
            Ok(()) => match config::save(&self.cfg_path, &self.cfg) {
                Ok(()) => {
                    self.dirty = false;
                    self.error = None;
                }
                Err(e) => self.error = Some(format!("저장 실패: {e}")),
            },
            Err(e) => self.error = Some(format!("잘못된 설정: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::fs;

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("dc-settings-test-{name}"));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p.push("config.toml");
        p
    }

    fn app_with(path: std::path::PathBuf) -> SettingsApp {
        SettingsApp {
            cfg: Config::default(),
            target: Target::Global,
            monitors: vec![],
            fonts: vec!["Consolas".into()],
            dirty: false,
            last_change_ms: 0,
            cfg_path: path,
            error: None,
        }
    }

    #[test]
    fn save_if_due_writes_only_after_debounce() {
        let path = tmp_path("debounce");
        let mut app = app_with(path.clone());
        app.cfg.style.size_px = 123.0;
        app.mark_dirty();
        app.last_change_ms = 1_000;

        app.save_if_due(1_200); // 200ms < 500ms debounce
        assert!(!path.exists(), "should not save before debounce elapses");

        app.save_if_due(1_500); // 500ms elapsed
        assert!(path.exists(), "should save after debounce");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("123"), "saved file must reflect the edit");
    }

    #[test]
    fn save_clears_dirty() {
        let path = tmp_path("clears");
        let mut app = app_with(path);
        app.mark_dirty();
        app.last_change_ms = 0;
        app.save_if_due(1_000);
        assert!(!app.dirty, "dirty must clear after a successful save");
    }

    #[test]
    fn invalid_config_is_not_saved_and_sets_error() {
        let path = tmp_path("invalid");
        let mut app = app_with(path.clone());
        app.cfg.style.opacity = 5.0; // out of range → validate rejects
        app.mark_dirty();
        app.last_change_ms = 0;
        app.save_if_due(1_000);
        assert!(!path.exists(), "invalid config must not be written");
        assert!(app.error.is_some(), "an error message must be surfaced");
    }

    #[test]
    fn flush_forces_a_pending_save() {
        let path = tmp_path("flush");
        let mut app = app_with(path.clone());
        app.cfg.style.size_px = 77.0;
        app.mark_dirty();
        app.last_change_ms = 999_999; // debounce not elapsed by wall clock
        app.flush();
        assert!(
            path.exists(),
            "flush must write even if debounce has not elapsed"
        );
    }
}
