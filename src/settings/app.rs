//! Settings window state, save logic, and the eframe UI itself.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use eframe::egui;

use crate::config::{self, Align, Anchor, Config, DrawMode, Line, Style};
use crate::platform;
use crate::settings::{lines, overrides, presets, presets_io, widgets};

/// Minimum gap between two writes of `config.toml`.
///
/// The renderer redraws from that file, so this interval is what the wallpaper's
/// responsiveness is made of. The first edit after a pause writes immediately (the gap
/// since the last write is already larger than this), and an edit that keeps going --
/// dragging a size or opacity slider -- writes again every `SAVE_INTERVAL_MS`, so the
/// wallpaper follows the drag live at ~10 Hz instead of waiting for it to end. The
/// interval is what keeps a drag from writing the file on every UI frame.
const SAVE_INTERVAL_MS: u64 = 100;

/// The preview's font size for a line whose `size_ratio` is 1.0. The preview shows the lines'
/// relative sizes, not their real pixel sizes -- the panel is far smaller than a monitor.
const PREVIEW_BASE_PX: f32 = 28.0;

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
    /// When `config.toml` was last written, for the `SAVE_INTERVAL_MS` throttle.
    pub(crate) last_write_ms: u64,
    pub(crate) cfg_path: PathBuf,
    pub(crate) presets_path: PathBuf,
    /// The built-ins plus the user's own, in picker order.
    pub(crate) library: presets::Library,
    /// A preset the user picked while the current one had unsaved edits on it. Held back
    /// until the discard prompt is answered; applying it straight away is exactly what would
    /// throw those edits out without asking.
    pub(crate) pending_preset: Option<usize>,
    /// The open Save-as box, if any.
    pub(crate) save_as: Option<SaveAs>,
    pub(crate) error: Option<String>,
    /// Tracks which font families are safe to render via `FontFamily::Name` (see
    /// `FontRegistry`).
    pub(crate) font_registry: FontRegistry,
    /// Filter text for the font picker's searchable list.
    pub(crate) font_search: String,
}

/// The Save-as box's state. `then_apply` is set when the box was opened from the discard
/// prompt: once the look is safely named, the preset the user had picked is applied.
#[derive(Debug, Default)]
pub(crate) struct SaveAs {
    pub name: String,
    pub then_apply: Option<usize>,
}

/// Tracks, across frames, which font families are registered with egui for the font
/// picker's per-name rendering (each family name is drawn in its own font — see
/// `style_fields`).
///
/// This needs three buckets, not just one, because of two egui/epaint constraints
/// discovered by running the settings window and reading its panic:
///
/// - `Context::add_font` only takes effect "at the start of the next pass" (its own
///   doc comment). Using `FontFamily::Name(family)` for a family added THIS frame
///   panics epaint (`Fonts::font`: "is not bound to any fonts") because
///   `font_definitions.families` is not updated until the next pass begins. So a
///   freshly queued family sits in `pending` and is only promoted into `active` —
///   meaning "safe to use `FontFamily::Name` now" — by `promote_pending`, which
///   `SettingsApp::ui` calls once at the very top of every frame (i.e. after at least
///   one full pass has elapsed since the `add_font` call).
/// - A family with no local file, or a corrupt one (`fonts::font_file` already
///   filters corrupt files out via its skrifa check), is never registered with egui
///   at all, so using `FontFamily::Name` for it would ALSO panic — permanently, not
///   just for one frame. Those are cached in `failed` so we neither retry the load
///   every frame nor ever try to render them in their own (nonexistent) family.
#[derive(Default)]
pub(crate) struct FontRegistry {
    active: HashSet<String>,
    pending: HashSet<String>,
    failed: HashSet<String>,
}

impl FontRegistry {
    /// Moves families queued last frame into `active`. Must be called once per frame,
    /// before any rendering that might request `FontFamily::Name` for a family queued
    /// during the previous frame — otherwise that rendering can panic (see struct docs).
    fn promote_pending(&mut self) {
        self.active.extend(self.pending.drain());
    }

    /// Registers `family` with `ctx` if it hasn't been tried yet, and reports whether
    /// it is safe to render with `FontFamily::Name(family)` *this* frame. Never panics
    /// and never retries a family already known to have failed.
    fn ensure(&mut self, ctx: &egui::Context, family: &str) -> bool {
        if self.active.contains(family) {
            return true;
        }
        if self.pending.contains(family) || self.failed.contains(family) {
            return false;
        }
        match crate::platform::fonts::font_file(family) {
            Some(file) => {
                let mut data = egui::FontData::from_owned(file.bytes);
                data.index = file.index;
                ctx.add_font(egui::epaint::text::FontInsert::new(
                    family,
                    data,
                    vec![egui::epaint::text::InsertFontFamily {
                        family: egui::FontFamily::Name(family.into()),
                        priority: egui::epaint::text::FontPriority::Highest,
                    }],
                ));
                self.pending.insert(family.to_string());
            }
            None => {
                self.failed.insert(family.to_string());
            }
        }
        false
    }
}

impl SettingsApp {
    pub fn new() -> Result<Self> {
        let cfg_path = crate::paths::config_path()?;
        let cfg = config::load_or_create(&cfg_path)?;
        let presets_path = crate::paths::presets_path()?;
        let loaded = presets_io::load(&presets_path);
        let mut library = presets::Library::new(loaded.presets);
        library.add_dropped(loaded.dropped);
        let monitors = platform::enumerate_monitors()
            .unwrap_or_default()
            .into_iter()
            .map(|m| MonitorRef {
                id: m.id,
                name: m.name,
            })
            .collect();
        let fonts = crate::platform::fonts::system_families().unwrap_or_default();
        Ok(Self {
            cfg,
            target: Target::Global,
            monitors,
            fonts,
            dirty: false,
            last_write_ms: 0,
            cfg_path,
            presets_path,
            library,
            pending_preset: None,
            save_as: None,
            error: None,
            font_registry: FontRegistry::default(),
            font_search: String::new(),
        })
    }

    /// Milliseconds since the Unix epoch, used as the save-throttle clock. Wall-clock based
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
    }

    /// Saves if there is an edit to save and the last write is at least `SAVE_INTERVAL_MS`
    /// old. Invalid configs are not written; the error is surfaced instead. Never blanks
    /// the file.
    pub fn save_if_due(&mut self, now_ms: u64) {
        if !crate::settings::widgets::should_save(
            self.dirty,
            now_ms.saturating_sub(self.last_write_ms),
            SAVE_INTERVAL_MS,
        ) {
            return;
        }
        self.write(now_ms);
    }

    /// Forces a save of any pending change, ignoring the throttle (used on window close).
    pub fn flush(&mut self) {
        if self.dirty {
            let now = self.now_ms();
            self.write(now);
        }
    }

    fn write(&mut self, now_ms: u64) {
        // Counts as an attempt even when it fails below: a config that does not validate
        // stays dirty, and without this it would be re-validated on every single frame.
        self.last_write_ms = now_ms;
        match config::validate(&self.cfg) {
            Ok(()) => match config::save(&self.cfg_path, &self.cfg) {
                Ok(()) => {
                    self.dirty = false;
                    self.error = None;
                }
                Err(e) => self.error = Some(format!("Save failed: {e}")),
            },
            Err(e) => self.error = Some(format!("Invalid config: {e}")),
        }
    }

    /// Which preset the current look sits on. Recomputed every frame rather than stored:
    /// every widget in this window can change what it depends on.
    pub fn active(&self) -> presets::Active {
        self.library
            .resolve(self.cfg.preset.as_deref(), &self.cfg.lines, &self.cfg.style)
    }

    /// Drops any preset index held across a library mutation that reorders or removes entries.
    ///
    /// `Library::delete` shifts every later index down by one, so an index captured before the
    /// delete (a pending combo pick, or a Save-as box's `then_apply`) points at the wrong
    /// preset afterwards -- or, if the deleted preset was the last one, past the end of the
    /// list. `Library::apply` is defensively bounds-checked against that (see its doc comment),
    /// but the right fix is to never carry a stale index into the next frame at all. The
    /// Save-as box itself, and the name typed into it, are left alone: only the "apply this
    /// after saving" pick is cleared.
    pub(crate) fn forget_pending(&mut self) {
        self.pending_preset = None;
        if let Some(state) = &mut self.save_as {
            state.then_apply = None;
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
        // Fonts queued last frame are now bound at the epaint level (egui applies
        // `ctx.add_font` "at the start of the next pass"), so this must run before any
        // widget in this frame might render with `FontFamily::Name` for one of them.
        self.font_registry.promote_pending();

        if let Some(err) = self.error.clone() {
            egui::Panel::top("dc_error_banner").show(ui, |ui| {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 50, 50),
                    format!("Error: {err}"),
                );
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

        // A throttled-away edit has no other way back: egui only draws a frame when there
        // is input or someone asks for one, and `save_if_due` only runs in a frame. Ask for
        // exactly the frame on which the throttle interval expires -- that one writes the
        // final value of a drag. Nothing pending means nothing to schedule, so an idle
        // settings window sits at 0 fps instead of repainting five times a second.
        if self.dirty {
            let since_write = self.now_ms().saturating_sub(self.last_write_ms);
            let wait = SAVE_INTERVAL_MS.saturating_sub(since_write).max(10);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(wait));
        }
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
    /// The preset picker: what the current look is called, and the four things you can do to
    /// it. Global-only -- a monitor override is a partial change on top of the global look,
    /// which is not a thing a whole-look snapshot can express. (A monitor starts from the
    /// current look anyway: `overrides::enable_style_override` copies it in.)
    fn ui_preset_bar(&mut self, ui: &mut egui::Ui) {
        let active = self.active();
        let label = match active {
            presets::Active::Clean(i) => self.library.all()[i].name.clone(),
            presets::Active::Modified(i) => format!("{} *", self.library.all()[i].name),
            presets::Active::Custom => "Custom".to_string(),
        };
        let base = match active {
            presets::Active::Clean(i) | presets::Active::Modified(i) => Some(i),
            presets::Active::Custom => None,
        };
        let modified = matches!(active, presets::Active::Modified(_));

        let mut picked: Option<usize> = None;
        ui.horizontal(|ui| {
            ui.label("Preset:");
            egui::ComboBox::from_id_salt("dc_preset_combo")
                .width(180.0)
                .selected_text(label.as_str())
                .show_ui(ui, |ui| {
                    ui.label("Built-in");
                    for (i, p) in self.library.all().iter().enumerate() {
                        if i == presets::BUILTIN_COUNT {
                            ui.separator();
                            ui.label("Saved");
                        }
                        if ui
                            .selectable_label(base == Some(i), p.name.as_str())
                            .clicked()
                        {
                            picked = Some(i);
                        }
                    }
                });

            if ui
                .add_enabled(modified, egui::Button::new("Reset"))
                .on_hover_text("Throw away the changes and go back to the preset")
                .clicked()
            {
                // Reset means "cancel the switch, go back to the preset I was on" -- the whole
                // carried-forward intent has to go, not just `pending_preset`. Clearing only
                // that would leave a Save-as box's `then_apply` armed if it was opened from the
                // discard prompt (pick a preset while Modified -> "Save as..." -> Reset): the
                // look is clean again, but saving would still apply the preset Reset just
                // backed away from.
                self.forget_pending();
                if let Some(i) = base {
                    self.library.apply(i, &mut self.cfg);
                    self.mark_dirty();
                }
            }

            if ui
                .button("Save as\u{2026}")
                .on_hover_text("Store the current lines and style as a preset of your own")
                .clicked()
            {
                self.save_as = Some(SaveAs::default());
                self.pending_preset = None;
            }

            let deletable = base.is_some_and(|i| !self.library.is_builtin(i));
            if ui
                .add_enabled(deletable, egui::Button::new("Delete"))
                .on_hover_text("Remove this preset. The lines and style on screen stay as they are")
                .clicked()
            {
                if let Some(i) = base {
                    // The look stays; only the label goes. Deleting a preset must not change
                    // what is on the wallpaper.
                    if self.library.delete(i) {
                        self.cfg.preset = None;
                        self.persist_presets();
                        self.mark_dirty();
                        self.forget_pending();
                    }
                }
            }
        });

        if let Some(i) = picked {
            if modified {
                self.pending_preset = Some(i);
            } else {
                self.library.apply(i, &mut self.cfg);
                self.mark_dirty();
            }
        }

        self.ui_discard_prompt(ui);
        self.ui_save_as(ui);
    }

    /// Shown when a preset was picked while the current one had unsaved edits. Inline, not a
    /// modal: the window saves as you type and there is no undo stack to fall back on, so the
    /// one place a confirmation earns its keep is the one click that throws work away.
    fn ui_discard_prompt(&mut self, ui: &mut egui::Ui) {
        let Some(pending) = self.pending_preset else {
            return;
        };
        let from = match self.active() {
            presets::Active::Modified(i) => self.library.all()[i].name.clone(),
            // `pending_preset` is only ever set while the look is `Modified(base)` (see the
            // combo handler below), so `cfg.preset` still names a live preset at that point.
            // Reaching `Clean` here means the edits stopped being edits while the prompt was
            // up -- the user undid them by hand, back onto the very label the prompt names --
            // so there is nothing left to discard. `Custom` is unreachable for the same
            // reason `Modified` is guaranteed above: nothing but Delete nulls `cfg.preset`,
            // and Delete clears `pending_preset` in the same step (`forget_pending`). Both
            // cases: apply and move on rather than asking about edits that no longer exist.
            presets::Active::Clean(_) | presets::Active::Custom => {
                self.library.apply(pending, &mut self.cfg);
                self.pending_preset = None;
                self.mark_dirty();
                return;
            }
        };

        ui.horizontal(|ui| {
            ui.colored_label(
                egui::Color32::from_rgb(200, 140, 40),
                format!("\u{26a0} Discard changes to \"{from}\"?"),
            );
            if ui.button("Discard").clicked() {
                self.library.apply(pending, &mut self.cfg);
                self.pending_preset = None;
                self.mark_dirty();
            }
            if ui.button("Save as\u{2026}").clicked() {
                self.save_as = Some(SaveAs {
                    name: String::new(),
                    then_apply: Some(pending),
                });
                self.pending_preset = None;
            }
            if ui.button("Cancel").clicked() {
                self.pending_preset = None;
            }
        });
    }

    /// The name box. Saving stores the current lines and style, moves the label onto the new
    /// preset, and -- when the box was opened from the discard prompt -- applies the preset
    /// the user had picked.
    fn ui_save_as(&mut self, ui: &mut egui::Ui) {
        let Some(mut state) = self.save_as.take() else {
            return;
        };
        let status = self.library.check_name(&state.name);
        let mut keep_open = true;

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.name)
                    .desired_width(180.0)
                    .hint_text("Preset name"),
            );

            let save_label = match status {
                presets::NameStatus::Overwrite => "Overwrite",
                _ => "Save",
            };
            let savable = matches!(
                status,
                presets::NameStatus::New | presets::NameStatus::Overwrite
            );
            if ui
                .add_enabled(savable, egui::Button::new(save_label))
                .clicked()
            {
                // `status` (and the `savable` it gates the button on) was computed before the
                // `TextEdit` above could mutate `state.name` this frame, so it can be one
                // keystroke stale -- a click landing in the same frame as an edit that changes
                // what `check_name` would say. Re-check against the name as typed rather than
                // trust the button having been enabled: saving under a built-in's name would
                // hand `Library::save_as` a name `check_name` was supposed to have blocked,
                // and `Library::new` would only drop it again on the next launch.
                let fresh = self.library.check_name(&state.name);
                if matches!(
                    fresh,
                    presets::NameStatus::New | presets::NameStatus::Overwrite
                ) {
                    let lines = self.cfg.lines.clone();
                    let style = self.cfg.style.clone();
                    let i = self.library.save_as(&state.name, &lines, &style);
                    self.cfg.preset = Some(self.library.all()[i].name.clone());
                    self.persist_presets();
                    if let Some(next) = state.then_apply {
                        self.library.apply(next, &mut self.cfg);
                    }
                    self.mark_dirty();
                    keep_open = false;
                }
            }
            if ui.button("Cancel").clicked() {
                keep_open = false;
            }

            match status {
                presets::NameStatus::Builtin => {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 50, 50),
                        "That is a built-in preset's name",
                    );
                }
                presets::NameStatus::Overwrite => {
                    ui.small("Replaces the preset of that name");
                }
                presets::NameStatus::Empty | presets::NameStatus::New => {}
            }
        });

        if keep_open {
            self.save_as = Some(state);
        }
    }

    /// Writes the user's presets to `presets.toml`: the ones the picker can use, followed by
    /// the ones the library could not (`Library::dropped`) -- a name collision, or a value that
    /// failed `config::validate` on load. Writing only `user()` would let the very next rewrite
    /// erase a hand-edited preset the settings window merely could not make sense of; appending
    /// `dropped()` is what keeps that data alive instead. Failure is shown in the error banner
    /// and otherwise ignored: the library in memory is still right, and nothing on the
    /// wallpaper depends on this file.
    fn persist_presets(&mut self) {
        let mut to_save = self.library.user().to_vec();
        to_save.extend(self.library.dropped().iter().cloned());
        if let Err(e) = presets_io::save(&self.presets_path, &to_save) {
            self.error = Some(format!("Could not save presets: {e}"));
        }
    }

    fn ui_target_selector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Editing:");
            let selected_text = match self.target {
                Target::Global => "Global default".to_string(),
                Target::Monitor(i) => self
                    .monitors
                    .get(i)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| "(Unknown monitor)".to_string()),
            };
            egui::ComboBox::from_id_salt("dc_target_combo")
                .selected_text(selected_text)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.target, Target::Global, "Global default");
                    for (i, m) in self.monitors.iter().enumerate() {
                        ui.selectable_value(&mut self.target, Target::Monitor(i), m.name.clone());
                    }
                });
        });
    }

    /// Approximate desktop preview: an egui-drawn dark panel showing the effective line list
    /// with each line's relative size, colour and alignment. Not a pixel match for the real
    /// DirectWrite renderer (design §4) — just a sense of the composition.
    fn ui_preview(&self, ui: &mut egui::Ui) {
        ui.heading("Preview");

        let eff = self.effective();
        let b = self.preview_breakdown();

        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(24, 24, 24))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_width(180.0);
                for l in &eff.lines {
                    let text = crate::tokens::render(&l.text, &b);
                    if text.is_empty() {
                        continue;
                    }
                    let rgb = widgets::hex_to_rgb(l.color.as_deref().unwrap_or(&eff.style.color));
                    // Sized against a fixed base, not against `size_px`: the preview panel is
                    // a fraction of a monitor's width, so what can be shown faithfully is how
                    // the lines relate to each other, not their real pixel sizes.
                    let px = (l.size_ratio * PREVIEW_BASE_PX).clamp(8.0, 40.0);
                    let align = match l.align {
                        config::Align::Left => egui::Align::LEFT,
                        config::Align::Center => egui::Align::Center,
                        config::Align::Right => egui::Align::RIGHT,
                    };
                    ui.with_layout(egui::Layout::top_down(align), |ui| {
                        ui.label(
                            egui::RichText::new(text)
                                .size(px)
                                .color(egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])),
                        );
                    });
                }
            });

        ui.add_space(6.0);
        ui.small("Preview is approximate; see the desktop for the exact result.");
        if !eff.enabled {
            ui.colored_label(
                egui::Color32::from_rgb(220, 160, 60),
                "This monitor is disabled",
            );
        }
    }

    /// The effective config for whatever the user is currently editing.
    fn effective(&self) -> config::Effective {
        let global = || config::Effective {
            enabled: true,
            anchor: self.cfg.layout.anchor,
            offset_px: self.cfg.layout.offset_px,
            style: self.cfg.style.clone(),
            lines: self.cfg.lines.clone(),
        };
        match self.target {
            Target::Global => global(),
            Target::Monitor(i) => match self.monitors.get(i) {
                Some(m) => config::effective_for(&self.cfg, &m.id),
                None => global(),
            },
        }
    }

    /// The countdown the preview renders, from the real target/now so the preview is a
    /// genuine (if approximately rendered) countdown, not a static mock. Falls back to an
    /// expired countdown if `target` cannot be resolved in the local time zone (e.g. an
    /// out-of-range year) rather than panicking.
    fn preview_breakdown(&self) -> crate::countdown::Breakdown {
        let now = jiff::Zoned::now();
        match self.cfg.target.to_zoned(jiff::tz::TimeZone::system()) {
            Ok(target) => crate::countdown::breakdown(&now, &target),
            Err(_) => crate::countdown::breakdown(&now, &now),
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
                        .prefix("Year "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut fields.month)
                        .range(1..=12)
                        .prefix("Month "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut fields.day)
                        .range(1..=31)
                        .prefix("Day "),
                )
                .changed();
        });
        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::DragValue::new(&mut fields.hour)
                        .range(0..=23)
                        .prefix("Hour "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut fields.minute)
                        .range(0..=59)
                        .prefix("Min "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut fields.second)
                        .range(0..=59)
                        .prefix("Sec "),
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
                ui.colored_label(egui::Color32::from_rgb(220, 50, 50), "Invalid date");
            }
        }

        ui.ctx().data_mut(|d| d.insert_temp(mem_id, fields));
    }

    fn ui_global(&mut self, ui: &mut egui::Ui) {
        ui.heading("Target time");
        self.ui_date_fields(ui);
        ui.separator();

        ui.heading("Text");
        let fonts = self.fonts.clone();
        if style_fields(
            ui,
            &mut self.cfg.style,
            &fonts,
            &mut self.font_registry,
            &mut self.font_search,
        ) {
            self.mark_dirty();
        }
        ui.separator();

        ui.heading("Lines");
        self.ui_preset_bar(ui);
        ui.add_space(4.0);
        // Lent out and put back: `lines_editor` needs `&mut Vec<Line>` while `self` is still
        // borrowed by `ui`'s closure-free call chain here.
        let mut list = std::mem::take(&mut self.cfg.lines);
        let lines_changed = lines_editor(ui, &mut list, "global");
        self.cfg.lines = list;
        if lines_changed {
            self.mark_dirty();
        }
        ui.separator();

        ui.heading("Layout");
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

        ui.heading("General");
        if ui
            .checkbox(&mut self.cfg.general.autostart, "Start with Windows")
            .changed()
        {
            self.mark_dirty();
        }
    }

    fn ui_monitor(&mut self, ui: &mut egui::Ui, idx: usize) {
        let Some(mref) = self.monitors.get(idx).cloned() else {
            ui.label("Monitor not found.");
            return;
        };
        let id = mref.id;
        let name = mref.name;

        ui.heading(&name);

        let mut enabled = overrides::find_override(&self.cfg, &id)
            .and_then(|o| o.enabled)
            .unwrap_or(true);
        if ui.checkbox(&mut enabled, "Show on this monitor").changed() {
            overrides::set_enabled(&mut self.cfg, &id, &name, enabled);
            self.mark_dirty();
        }

        let mut has_override = overrides::find_override(&self.cfg, &id)
            .map(overrides::has_style_override)
            .unwrap_or(false);
        if ui
            .checkbox(&mut has_override, "Override for this monitor")
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
            ui.label("Follows the global settings.");
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

        ui.heading("Layout");
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

        ui.heading("Text");
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
            // Legacy, migrated away on load and never written back; no widget touches it.
            show_summary_line: None,
        };

        let fonts = self.fonts.clone();
        if style_fields(
            ui,
            &mut style,
            &fonts,
            &mut self.font_registry,
            &mut self.font_search,
        ) {
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
            self.mark_dirty();
        }
        ui.separator();

        ui.heading("Lines");
        // `enable_style_override` seeds this with a copy of the global list, so the fallback
        // only matters for a hand-edited config.toml that set some style fields but no lines.
        let mut list = self.cfg.displays[o_idx]
            .lines
            .clone()
            .unwrap_or_else(|| self.cfg.lines.clone());
        if lines_editor(ui, &mut list, "monitor") {
            self.cfg.displays[o_idx].lines = Some(list);
            self.mark_dirty();
        }
    }
}

fn mode_label(mode: DrawMode) -> &'static str {
    match mode {
        DrawMode::Fill => "Fill",
        DrawMode::Outline => "Outline",
        DrawMode::Both => "Fill + Outline",
    }
}

/// A single font family name, rendered in its own font when `usable` (i.e. the family
/// is actually bound with egui this frame — see `FontRegistry`); otherwise rendered in
/// the default UI font, since using `FontFamily::Name` for an unbound family panics
/// epaint.
fn font_rich_text(family: &str, usable: bool) -> egui::RichText {
    let text = egui::RichText::new(family);
    if usable {
        text.font(egui::FontId::new(
            16.0,
            egui::FontFamily::Name(family.into()),
        ))
    } else {
        text.size(16.0)
    }
}

/// Draws every `[style]` field as an editable widget against a plain `Style`. Shared by
/// the global style (edited in place) and a monitor override's style (a synthesized
/// `Style` merging the override's `Some` fields with the global defaults — see
/// `ui_monitor`), so the widget layout and ranges only exist once. Returns whether any
/// field changed this frame.
///
/// The font picker renders every family name in that family's own font (design goal:
/// non-Latin family names, e.g. Hangul/Han/Kana font names, must not show as tofu),
/// which requires registering each font with egui on demand (`FontRegistry::ensure`).
/// To keep that bounded, only the currently-selected family and the rows visible in
/// the (search-filtered, virtualized) list are ever registered in a given frame —
/// never the whole system font list.
fn style_fields(
    ui: &mut egui::Ui,
    style: &mut Style,
    fonts: &[String],
    font_registry: &mut FontRegistry,
    font_search: &mut String,
) -> bool {
    let mut changed = false;

    let current_usable = font_registry.ensure(ui.ctx(), &style.font_family);
    ui.horizontal(|ui| {
        ui.label("Font:");
        ui.label(font_rich_text(&style.font_family, current_usable));
    });
    egui::CollapsingHeader::new("Change font\u{2026}")
        .id_salt("dc_font_picker")
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(font_search)
                    .hint_text("Search fonts\u{2026}")
                    .desired_width(f32::INFINITY),
            );
            let query = font_search.to_lowercase();
            let filtered: Vec<&String> = fonts
                .iter()
                .filter(|f| query.is_empty() || f.to_lowercase().contains(&query))
                .collect();
            egui::ScrollArea::vertical()
                .id_salt("dc_font_list_scroll")
                .max_height(200.0)
                // Keep the horizontal extent fixed to the parent width. Without this the
                // scroll area shrinks to the widest *currently visible* row, and because
                // show_rows only builds visible rows, that width — and thus the scrollbar —
                // jitters left/right as you scroll through names of different lengths.
                .auto_shrink([false, true])
                .show_rows(ui, 22.0, filtered.len(), |ui, range| {
                    // Make each row span the full width so the selection highlight and click
                    // target don't depend on the name's length.
                    ui.set_min_width(ui.available_width());
                    for i in range {
                        let family = filtered[i];
                        let usable = font_registry.ensure(ui.ctx(), family);
                        let selected = *family == style.font_family;
                        if ui
                            .selectable_label(selected, font_rich_text(family, usable))
                            .clicked()
                            && !selected
                        {
                            style.font_family = family.clone();
                            changed = true;
                        }
                    }
                });
        });

    changed |= ui
        .add(
            egui::Slider::new(&mut style.font_weight, 100..=900)
                .step_by(100.0)
                .text("Weight"),
        )
        .changed();
    // `SliderClamping::Edits`, not egui's default `Always`: these three sliders are narrower
    // than what `config::validate` accepts (any size > 0, any outline width >= 0, any finite
    // letter spacing), and `Always` rewrites the value it is given to fit the slider -- so
    // merely opening this window would silently cut a `size_px = 800` hand-written in
    // config.toml down to the slider's maximum. `Edits` clamps what the user drags or types
    // here, and leaves a value that came from the file alone.
    changed |= ui
        .add(
            egui::Slider::new(&mut style.size_px, 16.0..=512.0)
                .clamping(egui::SliderClamping::Edits)
                .text("Size"),
        )
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
        ui.label("Color:");
        let mut rgb = widgets::hex_to_rgb(&style.color);
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            style.color = widgets::rgb_to_hex(rgb);
            changed = true;
        }
        ui.label("Outline color:");
        let mut outline_rgb = widgets::hex_to_rgb(&style.outline_color);
        if ui.color_edit_button_srgb(&mut outline_rgb).changed() {
            style.outline_color = widgets::rgb_to_hex(outline_rgb);
            changed = true;
        }
    });

    changed |= ui
        .add(
            egui::Slider::new(&mut style.outline_width_px, 0.0..=10.0)
                .clamping(egui::SliderClamping::Edits)
                .text("Outline width"),
        )
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut style.opacity, 0.0..=1.0).text("Opacity"))
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(&mut style.letter_spacing_em, -0.05..=0.4)
                .clamping(egui::SliderClamping::Edits)
                .text("Letter spacing (em)"),
        )
        .changed();
    changed |= ui.checkbox(&mut style.shadow, "Shadow").changed();
    changed |= ui
        .checkbox(&mut style.tabular_figures, "Tabular figures")
        .changed();

    changed
}

fn align_label(align: Align) -> &'static str {
    match align {
        Align::Left => "Left",
        Align::Center => "Center",
        Align::Right => "Right",
    }
}

/// What a row's buttons asked for. Applied after the row loop: reordering or removing
/// mid-iteration would invalidate the indices the rest of the loop is walking.
enum LineAction {
    Up,
    Down,
    Remove,
}

/// The line-list editor: the token reference and one row per line. Shared by the global list
/// and a monitor override's list (`salt` keeps their widget ids apart). The preset bar is not
/// here -- it is global-only, and `SettingsApp::ui_preset_bar` draws it.
/// Returns whether anything changed this frame.
fn lines_editor(ui: &mut egui::Ui, list: &mut Vec<Line>, salt: &str) -> bool {
    let mut changed = false;

    egui::CollapsingHeader::new("Available tokens")
        .id_salt(("dc_tokens", salt))
        .show(ui, |ui| {
            egui::Grid::new(("dc_token_grid", salt))
                .num_columns(2)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    for (token, description) in crate::tokens::TOKENS {
                        ui.monospace(token);
                        ui.small(description);
                        ui.end_row();
                    }
                });
        });
    ui.add_space(4.0);

    let last = list.len().saturating_sub(1);
    let mut action: Option<(usize, LineAction)> = None;
    for (i, line) in list.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut line.text)
                        .desired_width(170.0)
                        .hint_text("Text or {token}"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut line.size_ratio)
                        .speed(0.01)
                        .range(0.05..=4.0)
                        .fixed_decimals(2),
                )
                .on_hover_text("Size, relative to the base size above")
                .changed();
            egui::ComboBox::from_id_salt(("dc_align", salt, i))
                .width(72.0)
                .selected_text(align_label(line.align))
                .show_ui(ui, |ui| {
                    for a in [Align::Left, Align::Center, Align::Right] {
                        changed |= ui
                            .selectable_value(&mut line.align, a, align_label(a))
                            .changed();
                    }
                });

            // The colour button needs a concrete colour even when the line is inheriting one,
            // so the checkbox -- not the button -- is what says "own colour" vs. "inherit".
            // Switching inheritance off starts from white rather than from the inherited
            // colour: the editor is not given the `Style` (a monitor override synthesizes its
            // own), and the seed is one click away from any other colour anyway.
            let mut own = line.color.is_some();
            if ui
                .checkbox(&mut own, "")
                .on_hover_text("Use a colour of its own instead of the global one")
                .changed()
            {
                line.color = own.then(|| "#FFFFFF".to_string());
                changed = true;
            }
            if let Some(hex) = &mut line.color {
                let mut rgb = widgets::hex_to_rgb(hex);
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    *hex = widgets::rgb_to_hex(rgb);
                    changed = true;
                }
            }

            if ui
                .add_enabled(i > 0, egui::Button::new("\u{2191}"))
                .clicked()
            {
                action = Some((i, LineAction::Up));
            }
            if ui
                .add_enabled(i < last, egui::Button::new("\u{2193}"))
                .clicked()
            {
                action = Some((i, LineAction::Down));
            }
            if ui
                .add_enabled(last > 0, egui::Button::new("\u{2715}"))
                .on_hover_text("Remove this line (the last one cannot be removed)")
                .clicked()
            {
                action = Some((i, LineAction::Remove));
            }
        });
    }

    if let Some((i, what)) = action {
        match what {
            LineAction::Up => lines::move_up(list, i),
            LineAction::Down => lines::move_down(list, i),
            LineAction::Remove => lines::remove(list, i),
        }
        changed = true;
    }

    if ui.button("+ Add line").clicked() {
        lines::add(list);
        changed = true;
    }

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
            last_write_ms: 0,
            cfg_path: path,
            presets_path: PathBuf::from("presets.toml"),
            library: crate::settings::presets::Library::new(Vec::new()),
            pending_preset: None,
            save_as: None,
            error: None,
            font_registry: FontRegistry::default(),
            font_search: String::new(),
        }
    }

    // These two tests reproduce, at the unit level, the exact defect that caused a
    // real launch of the settings window to panic during development: registering a
    // font with `ctx.add_font` and using `FontFamily::Name` for it in the *same*
    // frame panics epaint, because the registration only takes effect "at the start
    // of the next pass". `FontRegistry` exists specifically to make that impossible.
    #[test]
    fn font_registry_defers_a_newly_queued_family_by_one_frame() {
        let mut reg = FontRegistry::default();
        let ctx = egui::Context::default();
        // Arial ships on every Windows install, so `fonts::font_file` succeeds and
        // `ctx.add_font` is called -- but it must not be reported usable yet.
        assert!(
            !reg.ensure(&ctx, "Arial"),
            "a family just queued this frame must not be usable this frame"
        );
        reg.promote_pending();
        assert!(
            reg.ensure(&ctx, "Arial"),
            "a family queued in a prior frame must be usable after promote_pending"
        );
    }

    #[test]
    fn font_registry_never_reports_a_failed_family_as_usable() {
        let mut reg = FontRegistry::default();
        let ctx = egui::Context::default();
        assert!(!reg.ensure(&ctx, "NoSuchFamily12345XYZ"));
        reg.promote_pending();
        assert!(
            !reg.ensure(&ctx, "NoSuchFamily12345XYZ"),
            "a family with no local file must never become usable, even across frames"
        );
    }

    /// The edit that starts an interaction -- a click, a checkbox, the first frame of a
    /// drag -- must reach the file at once. The old trailing debounce made every edit wait
    /// out a quiet window first, which is what the wallpaper's lag was made of.
    #[test]
    fn the_first_edit_after_a_pause_saves_immediately() {
        let path = tmp_path("leading");
        let mut app = app_with(path.clone());
        app.cfg.style.size_px = 123.0;
        app.mark_dirty();

        app.save_if_due(1_000); // last write was long ago (0)
        assert!(
            path.exists(),
            "the first edit must not wait for a quiet window"
        );
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("123"), "saved file must reflect the edit");
    }

    /// ...but an interaction that keeps producing edits (a slider drag, one edit per UI
    /// frame) must not write the file on every frame.
    #[test]
    fn a_continuing_edit_is_throttled_then_written() {
        let path = tmp_path("throttle");
        let mut app = app_with(path.clone());
        app.mark_dirty();
        app.save_if_due(1_000); // leading write
        assert!(app.last_write_ms == 1_000 && !app.dirty);

        app.cfg.style.size_px = 55.0;
        app.mark_dirty();
        app.save_if_due(1_050); // 50ms < 100ms interval
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("55"),
            "should not write again within the interval"
        );
        assert!(app.dirty, "the pending edit must stay pending");

        app.save_if_due(1_100); // interval elapsed
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("55"),
            "the pending edit must be written once the interval passes"
        );
    }

    #[test]
    fn save_clears_dirty() {
        let path = tmp_path("clears");
        let mut app = app_with(path);
        app.mark_dirty();
        app.save_if_due(1_000);
        assert!(!app.dirty, "dirty must clear after a successful save");
    }

    #[test]
    fn invalid_config_is_not_saved_and_sets_error() {
        let path = tmp_path("invalid");
        let mut app = app_with(path.clone());
        app.cfg.style.opacity = 5.0; // out of range → validate rejects
        app.mark_dirty();
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
        app.last_write_ms = app.now_ms(); // throttle interval has not elapsed
        app.flush();
        assert!(
            path.exists(),
            "flush must write even if the throttle interval has not elapsed"
        );
    }

    /// The regression this guards: a settings window that loads `presets.toml`, drops
    /// entries it cannot use, and then rewrites the file from the survivors alone would
    /// silently destroy those entries on the very first Save-as or Delete. `persist_presets`
    /// is supposed to write `library.user()` followed by `library.dropped()` -- this drives
    /// the *real* method (not a hand-rolled stand-in for it) end to end: file on disk ->
    /// `presets_io::load` -> `Library::new` + `add_dropped` (exactly as `SettingsApp::new`
    /// builds it) -> `persist_presets` -> file on disk again.
    #[test]
    fn persist_presets_keeps_dropped_presets_in_the_file() {
        let cfg_path = tmp_path("persist-dropped");
        let presets_path = cfg_path.with_file_name("presets.toml");

        // Fails `config::validate`: dropped by `presets_io::load` into `Loaded::dropped`.
        let invalid = presets::Preset {
            name: "Bad".to_string(),
            style: Style {
                opacity: 3.0,
                ..Style::default()
            },
            lines: vec![Line::default()],
        };
        // Name collides with the "D-Day" built-in: dropped by `Library::new` into its own
        // `dropped` list. Marked with a distinctive line so it can be told apart from the
        // built-in of the same name below.
        let colliding = presets::Preset {
            name: "D-Day".to_string(),
            style: Style::default(),
            lines: vec![Line {
                text: "shadowed".to_string(),
                ..Line::default()
            }],
        };
        // An ordinary preset the library can use.
        let good = presets::Preset {
            name: "Good".to_string(),
            style: Style::default(),
            lines: vec![Line::default()],
        };
        presets_io::save(
            &presets_path,
            &[invalid.clone(), colliding.clone(), good.clone()],
        )
        .unwrap();

        // Build the library exactly the way `SettingsApp::new` does.
        let loaded = presets_io::load(&presets_path);
        let mut library = presets::Library::new(loaded.presets);
        library.add_dropped(loaded.dropped);

        let mut app = SettingsApp {
            presets_path: presets_path.clone(),
            library,
            ..app_with(cfg_path)
        };

        app.persist_presets();

        let reloaded = presets_io::load(&presets_path);
        let mut names: Vec<&str> = reloaded
            .presets
            .iter()
            .chain(reloaded.dropped.iter())
            .map(|p| p.name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["Bad", "D-Day", "Good"],
            "all three original entries -- the usable one and the two dropped ones -- must \
             still be present in the file after persist_presets rewrites it"
        );

        // Neither dropped preset became reachable through the picker.
        assert_eq!(
            app.library.all().len(),
            presets::BUILTIN_COUNT + 1,
            "only the usable preset was added on top of the built-ins"
        );
        assert!(
            app.library.all().iter().all(|p| p.name != "Bad"),
            "the invalid preset must not be pickable"
        );
        assert!(
            app.library
                .all()
                .iter()
                .all(|p| p.lines.first().map(|l| l.text.as_str()) != Some("shadowed")),
            "the name-colliding preset must not be pickable, even under the built-in's name"
        );
    }
}

#[cfg(test)]
mod preset_bar_tests {
    use super::*;
    use crate::settings::presets;

    fn app(cfg: Config) -> SettingsApp {
        SettingsApp {
            cfg,
            target: Target::Global,
            monitors: Vec::new(),
            fonts: Vec::new(),
            dirty: false,
            last_write_ms: 0,
            cfg_path: PathBuf::from("config.toml"),
            presets_path: PathBuf::from("presets.toml"),
            library: presets::Library::new(Vec::new()),
            pending_preset: None,
            save_as: None,
            error: None,
            font_registry: FontRegistry::default(),
            font_search: String::new(),
        }
    }

    #[test]
    fn a_fresh_config_is_clean_on_its_own_preset() {
        let a = app(Config::default());
        let i = a.library.find("Clock only").expect("Clock only");
        assert_eq!(a.active(), presets::Active::Clean(i));
    }

    #[test]
    fn a_style_edit_makes_the_active_preset_modified() {
        let mut cfg = Config::default();
        cfg.style.size_px = 123.0;
        let a = app(cfg);
        let i = a.library.find("Clock only").expect("Clock only");
        assert_eq!(a.active(), presets::Active::Modified(i));
    }

    /// This is the invariant Finding 2's defence-in-depth exists behind: `Library::delete`
    /// shifts every later index down, so a pending combo pick or a Save-as box's `then_apply`
    /// captured before the delete must not survive it. Proven at the unit level here rather
    /// than only inline in the Delete button's closure.
    #[test]
    fn deleting_a_preset_forgets_the_pending_pick_and_the_save_as_carry() {
        let mut a = app(Config::default());
        a.library = presets::Library::new(vec![presets::Preset {
            name: "Mine".to_string(),
            style: Style::default(),
            lines: vec![Line::default()],
        }]);
        let i = presets::BUILTIN_COUNT; // "Mine", the only user preset
        a.pending_preset = Some(i);
        a.save_as = Some(SaveAs {
            name: "Draft".to_string(),
            then_apply: Some(i),
        });

        assert!(a.library.delete(i), "deleting a user preset must succeed");
        a.forget_pending();

        assert_eq!(
            a.pending_preset, None,
            "a pending pick captured before the delete must not survive it"
        );
        let save_as = a.save_as.as_ref().expect("the box itself stays open");
        assert_eq!(
            save_as.then_apply, None,
            "a then_apply carry captured before the delete must not survive it"
        );
        assert_eq!(
            save_as.name, "Draft",
            "the typed name is untouched by forget_pending"
        );
    }
}
