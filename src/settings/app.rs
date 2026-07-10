//! Settings window state, save logic, and the eframe UI itself.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use eframe::egui;

use crate::config::{self, Anchor, Config, DrawMode, Style};
use crate::monitors;
use crate::settings::{overrides, widgets};

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

/// Key for the scratch copy of the six target-date fields kept in egui's per-`Context`
/// temp storage (not in `SettingsApp` itself). `self.cfg.target` is a `jiff::civil::DateTime`,
/// which cannot represent an invalid combination (e.g. day=31 while month is still February),
/// so a combination that is briefly invalid mid-edit has nowhere else to live; without this
/// scratch copy, re-deriving the fields from `self.cfg.target` every frame would snap the
/// in-progress edit back to the last valid value the instant it turned invalid.
const DATE_FIELDS_MEMORY_ID: &str = "dc_settings_target_date_fields";

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(err) = self.error.clone() {
            egui::Panel::top("dc_error_banner").show(ui, |ui| {
                ui.colored_label(egui::Color32::from_rgb(220, 50, 50), format!("오류: {err}"));
            });
        }

        egui::Panel::top("dc_target_selector").show(ui, |ui| {
            self.ui_target_selector(ui);
        });

        egui::Panel::right("dc_preview")
            .min_size(200.0)
            .show(ui, |ui| {
                self.ui_preview(ui);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.target {
                Target::Global => self.ui_global(ui),
                Target::Monitor(i) => self.ui_monitor(ui, i),
            });
        });

        let now = self.now_ms();
        self.save_if_due(now);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(200));
    }

    /// Flushes any debounced-but-not-yet-written edit when the window closes, so an
    /// edit made just before closing (within the 500ms debounce window) is not silently
    /// lost (design §9). `eframe`'s default (non-`glow`) features are used here, so this
    /// hook takes no `glow::Context` argument — see `eframe::App::on_exit`.
    fn on_exit(&mut self) {
        self.flush();
    }
}

impl SettingsApp {
    fn ui_target_selector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("편집 대상:");
            let selected_text = match self.target {
                Target::Global => "전역 기본값".to_string(),
                Target::Monitor(i) => self
                    .monitors
                    .get(i)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| "(알 수 없는 모니터)".to_string()),
            };
            egui::ComboBox::from_id_salt("dc_target_combo")
                .selected_text(selected_text)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.target, Target::Global, "전역 기본값");
                    for (i, m) in self.monitors.iter().enumerate() {
                        ui.selectable_value(&mut self.target, Target::Monitor(i), m.name.clone());
                    }
                });
        });
    }

    /// Approximate desktop preview: an egui-drawn dark panel tinted by the effective
    /// colour of the current edit target. Not a pixel match for the real DirectWrite
    /// renderer (design §4) — just a sense of colour/size/summary-line-on-off.
    fn ui_preview(&self, ui: &mut egui::Ui) {
        ui.heading("미리보기");

        let eff = match self.target {
            Target::Global => config::Effective {
                enabled: true,
                anchor: self.cfg.layout.anchor,
                offset_px: self.cfg.layout.offset_px,
                style: self.cfg.style.clone(),
            },
            Target::Monitor(i) => match self.monitors.get(i) {
                Some(m) => config::effective_for(&self.cfg, &m.id),
                None => config::Effective {
                    enabled: true,
                    anchor: self.cfg.layout.anchor,
                    offset_px: self.cfg.layout.offset_px,
                    style: self.cfg.style.clone(),
                },
            },
        };

        let rgb = widgets::hex_to_rgb(&eff.style.color);
        let text_color = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        let (summary, main) = self.preview_lines();

        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(24, 24, 24))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_width(180.0);
                if eff.style.show_summary_line {
                    ui.colored_label(text_color, &summary);
                }
                ui.colored_label(text_color, &main);
            });

        ui.add_space(6.0);
        ui.small("정확한 표시는 바탕화면에서 확인하세요.");
        if !eff.enabled {
            ui.colored_label(
                egui::Color32::from_rgb(220, 160, 60),
                "이 모니터는 비활성화됨",
            );
        }
    }

    /// The countdown text for the preview, computed from the real target/now so the
    /// preview is a genuine (if approximately rendered) countdown, not a static mock.
    /// Falls back to zeroed placeholders if `target` cannot be resolved in the local
    /// time zone (e.g. an out-of-range year) rather than panicking.
    fn preview_lines(&self) -> (String, String) {
        match self.cfg.target.to_zoned(jiff::tz::TimeZone::system()) {
            Ok(target) => {
                let b = crate::countdown::breakdown(&jiff::Zoned::now(), &target);
                (
                    crate::countdown::format_summary(&b),
                    crate::countdown::format_main(&b),
                )
            }
            Err(_) => ("0m 0w 0d".to_string(), "00:00:00".to_string()),
        }
    }

    fn ui_date_fields(&mut self, ui: &mut egui::Ui) {
        let mem_id = egui::Id::new(DATE_FIELDS_MEMORY_ID);
        let mut fields = ui
            .ctx()
            .data(|d| d.get_temp::<widgets::DateFields>(mem_id))
            .unwrap_or_else(|| widgets::fields_from_datetime(self.cfg.target));

        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::DragValue::new(&mut fields.year)
                        .range(2000..=2100)
                        .prefix("년 "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut fields.month)
                        .range(1..=12)
                        .prefix("월 "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut fields.day)
                        .range(1..=31)
                        .prefix("일 "),
                )
                .changed();
        });
        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::DragValue::new(&mut fields.hour)
                        .range(0..=23)
                        .prefix("시 "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut fields.minute)
                        .range(0..=59)
                        .prefix("분 "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut fields.second)
                        .range(0..=59)
                        .prefix("초 "),
                )
                .changed();
        });

        match widgets::datetime_from_fields(&fields) {
            Some(dt) => {
                if changed {
                    self.cfg.target = dt;
                    self.mark_dirty();
                }
            }
            None => {
                ui.colored_label(egui::Color32::from_rgb(220, 50, 50), "잘못된 날짜");
            }
        }

        ui.ctx().data_mut(|d| d.insert_temp(mem_id, fields));
    }

    fn ui_global(&mut self, ui: &mut egui::Ui) {
        ui.heading("목표 시각");
        self.ui_date_fields(ui);
        ui.separator();

        ui.heading("글자");
        let fonts = self.fonts.clone();
        if style_fields(ui, &mut self.cfg.style, &fonts) {
            self.mark_dirty();
        }
        ui.separator();

        ui.heading("레이아웃");
        if anchor_grid(ui, "dc_anchor_global", &mut self.cfg.layout.anchor) {
            self.mark_dirty();
        }
        let mut offset = self.cfg.layout.offset_px;
        let mut off_changed = false;
        ui.horizontal(|ui| {
            off_changed |= ui
                .add(
                    egui::DragValue::new(&mut offset[0])
                        .range(-5000..=5000)
                        .prefix("x: "),
                )
                .changed();
            off_changed |= ui
                .add(
                    egui::DragValue::new(&mut offset[1])
                        .range(-5000..=5000)
                        .prefix("y: "),
                )
                .changed();
        });
        if off_changed {
            self.cfg.layout.offset_px = offset;
            self.mark_dirty();
        }
        ui.separator();

        ui.heading("일반");
        if ui
            .checkbox(&mut self.cfg.general.autostart, "Windows 시작 시 자동 실행")
            .changed()
        {
            self.mark_dirty();
        }
    }

    fn ui_monitor(&mut self, ui: &mut egui::Ui, idx: usize) {
        let Some(mref) = self.monitors.get(idx).cloned() else {
            ui.label("모니터를 찾을 수 없습니다.");
            return;
        };
        let id = mref.id;
        let name = mref.name;

        ui.heading(&name);

        let mut enabled = overrides::find_override(&self.cfg, &id)
            .and_then(|o| o.enabled)
            .unwrap_or(true);
        if ui.checkbox(&mut enabled, "이 모니터에 표시").changed() {
            overrides::set_enabled(&mut self.cfg, &id, &name, enabled);
            self.mark_dirty();
        }

        let mut has_override = overrides::find_override(&self.cfg, &id)
            .map(overrides::has_style_override)
            .unwrap_or(false);
        if ui
            .checkbox(&mut has_override, "전역과 다르게 설정")
            .changed()
        {
            if has_override {
                overrides::enable_style_override(&mut self.cfg, &id, &name);
            } else {
                overrides::disable_style_override(&mut self.cfg, &id);
            }
            self.mark_dirty();
        }

        if !has_override {
            ui.label("전역 설정을 그대로 따릅니다.");
            return;
        }
        let Some(o_idx) = self.cfg.displays.iter().position(|d| d.id == id) else {
            return;
        };
        ui.separator();

        // `enable_style_override` always sets every override field at once, so once
        // `has_style_override` is true every `Option` below is normally `Some`; the
        // global value is only a defensive fallback in case a hand-edited config.toml
        // set some but not all of the override's style fields.
        let globals = self.cfg.style.clone();
        let global_layout = self.cfg.layout.clone();

        ui.heading("레이아웃");
        let mut anchor = self.cfg.displays[o_idx]
            .anchor
            .unwrap_or(global_layout.anchor);
        if anchor_grid(ui, "dc_anchor_monitor", &mut anchor) {
            self.cfg.displays[o_idx].anchor = Some(anchor);
            self.mark_dirty();
        }
        let mut offset = self.cfg.displays[o_idx]
            .offset_px
            .unwrap_or(global_layout.offset_px);
        let mut off_changed = false;
        ui.horizontal(|ui| {
            off_changed |= ui
                .add(
                    egui::DragValue::new(&mut offset[0])
                        .range(-5000..=5000)
                        .prefix("x: "),
                )
                .changed();
            off_changed |= ui
                .add(
                    egui::DragValue::new(&mut offset[1])
                        .range(-5000..=5000)
                        .prefix("y: "),
                )
                .changed();
        });
        if off_changed {
            self.cfg.displays[o_idx].offset_px = Some(offset);
            self.mark_dirty();
        }
        ui.separator();

        ui.heading("글자");
        let o = &self.cfg.displays[o_idx];
        let mut style = Style {
            font_family: o
                .font_family
                .clone()
                .unwrap_or_else(|| globals.font_family.clone()),
            font_weight: o.font_weight.unwrap_or(globals.font_weight),
            size_px: o.size_px.unwrap_or(globals.size_px),
            mode: o.mode.unwrap_or(globals.mode),
            color: o.color.clone().unwrap_or_else(|| globals.color.clone()),
            outline_color: o
                .outline_color
                .clone()
                .unwrap_or_else(|| globals.outline_color.clone()),
            outline_width_px: o.outline_width_px.unwrap_or(globals.outline_width_px),
            opacity: o.opacity.unwrap_or(globals.opacity),
            letter_spacing_em: o.letter_spacing_em.unwrap_or(globals.letter_spacing_em),
            shadow: o.shadow.unwrap_or(globals.shadow),
            tabular_figures: o.tabular_figures.unwrap_or(globals.tabular_figures),
            show_summary_line: o.show_summary_line.unwrap_or(globals.show_summary_line),
        };

        let fonts = self.fonts.clone();
        if style_fields(ui, &mut style, &fonts) {
            let o = &mut self.cfg.displays[o_idx];
            o.font_family = Some(style.font_family);
            o.font_weight = Some(style.font_weight);
            o.size_px = Some(style.size_px);
            o.mode = Some(style.mode);
            o.color = Some(style.color);
            o.outline_color = Some(style.outline_color);
            o.outline_width_px = Some(style.outline_width_px);
            o.opacity = Some(style.opacity);
            o.letter_spacing_em = Some(style.letter_spacing_em);
            o.shadow = Some(style.shadow);
            o.tabular_figures = Some(style.tabular_figures);
            o.show_summary_line = Some(style.show_summary_line);
            self.mark_dirty();
        }
    }
}

fn mode_label(mode: DrawMode) -> &'static str {
    match mode {
        DrawMode::Fill => "채우기",
        DrawMode::Outline => "외곽선",
        DrawMode::Both => "채우기+외곽선",
    }
}

/// Draws every `[style]` field as an editable widget against a plain `Style`. Shared by
/// the global style (edited in place) and a monitor override's style (a synthesized
/// `Style` merging the override's `Some` fields with the global defaults — see
/// `ui_monitor`), so the widget layout and ranges only exist once. Returns whether any
/// field changed this frame.
fn style_fields(ui: &mut egui::Ui, style: &mut Style, fonts: &[String]) -> bool {
    let mut changed = false;

    egui::ComboBox::from_label("폰트")
        .selected_text(style.font_family.clone())
        .show_ui(ui, |ui| {
            for f in fonts {
                changed |= ui
                    .selectable_value(&mut style.font_family, f.clone(), f)
                    .changed();
            }
        });

    changed |= ui
        .add(
            egui::Slider::new(&mut style.font_weight, 100..=900)
                .step_by(100.0)
                .text("굵기"),
        )
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut style.size_px, 16.0..=240.0).text("크기"))
        .changed();

    egui::ComboBox::from_id_salt("dc_draw_mode")
        .selected_text(mode_label(style.mode))
        .show_ui(ui, |ui| {
            for m in [DrawMode::Fill, DrawMode::Outline, DrawMode::Both] {
                changed |= ui
                    .selectable_value(&mut style.mode, m, mode_label(m))
                    .changed();
            }
        });

    ui.horizontal(|ui| {
        ui.label("색:");
        let mut rgb = widgets::hex_to_rgb(&style.color);
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            style.color = widgets::rgb_to_hex(rgb);
            changed = true;
        }
        ui.label("외곽선 색:");
        let mut outline_rgb = widgets::hex_to_rgb(&style.outline_color);
        if ui.color_edit_button_srgb(&mut outline_rgb).changed() {
            style.outline_color = widgets::rgb_to_hex(outline_rgb);
            changed = true;
        }
    });

    changed |= ui
        .add(egui::Slider::new(&mut style.outline_width_px, 0.0..=10.0).text("외곽선 두께"))
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut style.opacity, 0.0..=1.0).text("불투명도"))
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut style.letter_spacing_em, -0.05..=0.4).text("자간(em)"))
        .changed();
    changed |= ui.checkbox(&mut style.shadow, "그림자").changed();
    changed |= ui
        .checkbox(&mut style.tabular_figures, "고정폭 숫자")
        .changed();
    changed |= ui
        .checkbox(&mut style.show_summary_line, "요약줄 표시")
        .changed();

    changed
}

/// A 3x3 anchor picker: clicking a cell sets `*anchor` to that cell's `Anchor` (design
/// §6). `salt` must be unique among grids shown in the same frame (global vs. monitor
/// editing are mutually exclusive per frame, so a fixed per-caller salt is enough).
fn anchor_grid(ui: &mut egui::Ui, salt: &str, anchor: &mut Anchor) -> bool {
    let mut changed = false;
    let (sel_row, sel_col) = widgets::anchor_to_cell(*anchor);
    egui::Grid::new(salt).spacing([4.0, 4.0]).show(ui, |ui| {
        for row in 0..3usize {
            for col in 0..3usize {
                let selected = row == sel_row && col == sel_col;
                let symbol = match (row, col) {
                    (0, 0) => "\u{2196}",
                    (0, 1) => "\u{2191}",
                    (0, 2) => "\u{2197}",
                    (1, 0) => "\u{2190}",
                    (1, 1) => "\u{2022}",
                    (1, 2) => "\u{2192}",
                    (2, 0) => "\u{2199}",
                    (2, 1) => "\u{2193}",
                    (2, 2) => "\u{2198}",
                    _ => unreachable!("row/col are always in 0..3"),
                };
                if ui.selectable_label(selected, symbol).clicked() && !selected {
                    *anchor = widgets::cell_to_anchor(row, col);
                    changed = true;
                }
            }
            ui.end_row();
        }
    });
    changed
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
