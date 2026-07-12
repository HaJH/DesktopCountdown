//! Config schema, defaults, and validation. No Win32, no I/O.

use jiff::civil::DateTime;
use serde::{Deserialize, Serialize};

use crate::color::parse_hex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DrawMode {
    Fill,
    Outline,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    Center,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

fn d_font_family() -> String {
    "Consolas".to_string()
}
fn d_font_weight() -> u16 {
    400
}
fn d_size_px() -> f32 {
    64.0
}
fn d_mode() -> DrawMode {
    DrawMode::Fill
}
fn d_color() -> String {
    "#FFFFFF".to_string()
}
fn d_outline_color() -> String {
    "#000000".to_string()
}
fn d_outline_width() -> f32 {
    1.5
}
fn d_opacity() -> f32 {
    0.85
}
fn d_letter_spacing() -> f32 {
    0.02
}
fn d_true() -> bool {
    true
}
fn d_anchor() -> Anchor {
    Anchor::Center
}
fn d_offset() -> [i32; 2] {
    [0, 0]
}
fn d_size_ratio() -> f32 {
    1.0
}
fn d_align() -> Align {
    Align::Center
}

/// The summary line's em size as a fraction of `size_px`. Was hard-coded in the renderer as
/// `SUMMARY_RATIO` before lines became configurable; kept as the classic preset's value.
pub const SUMMARY_SIZE_RATIO: f32 = 0.28;
pub const SUMMARY_TEMPLATE: &str = "{months}m {weeks}w {days}d";
pub const MAIN_TEMPLATE: &str = "{hh}:{mm}:{ss}";
/// Longest `Line::text` accepted, in characters.
pub const MAX_TEXT_LEN: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Align {
    Left,
    Center,
    Right,
}

/// One line of the countdown. `text` is a token template (see `crate::tokens`); everything not
/// named here (font, weight, draw mode, outline, shadow, letter spacing, opacity) comes from
/// `[style]` and is shared by every line.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    #[serde(default)]
    pub text: String,
    /// Em size as a fraction of `Style::size_px`.
    #[serde(default = "d_size_ratio")]
    pub size_ratio: f32,
    #[serde(default = "d_align")]
    pub align: Align,
    /// `None` inherits `Style::color`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl Default for Line {
    fn default() -> Self {
        Self {
            text: String::new(),
            size_ratio: d_size_ratio(),
            align: d_align(),
            color: None,
        }
    }
}

/// The classic two-line countdown: an optional summary above the clock.
pub fn default_lines(summary: bool) -> Vec<Line> {
    let mut v = Vec::with_capacity(2);
    if summary {
        v.push(Line {
            text: SUMMARY_TEMPLATE.to_string(),
            size_ratio: SUMMARY_SIZE_RATIO,
            ..Line::default()
        });
    }
    v.push(Line {
        text: MAIN_TEMPLATE.to_string(),
        ..Line::default()
    });
    v
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Style {
    #[serde(default = "d_font_family")]
    pub font_family: String,
    #[serde(default = "d_font_weight")]
    pub font_weight: u16,
    #[serde(default = "d_size_px")]
    pub size_px: f32,
    #[serde(default = "d_mode")]
    pub mode: DrawMode,
    #[serde(default = "d_color")]
    pub color: String,
    #[serde(default = "d_outline_color")]
    pub outline_color: String,
    #[serde(default = "d_outline_width")]
    pub outline_width_px: f32,
    #[serde(default = "d_opacity")]
    pub opacity: f32,
    #[serde(default = "d_letter_spacing")]
    pub letter_spacing_em: f32,
    #[serde(default = "d_true")]
    pub shadow: bool,
    #[serde(default = "d_true")]
    pub tabular_figures: bool,
    /// Legacy. Older config files carried this flag instead of a `[[line]]` list; `migrate`
    /// turns it into one and nothing else reads it. Never written back
    /// (`skip_serializing`), but still accepted on read — `deny_unknown_fields` would reject
    /// an existing file otherwise.
    #[serde(default, skip_serializing)]
    pub show_summary_line: Option<bool>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            font_family: d_font_family(),
            font_weight: d_font_weight(),
            size_px: d_size_px(),
            mode: d_mode(),
            color: d_color(),
            outline_color: d_outline_color(),
            outline_width_px: d_outline_width(),
            opacity: d_opacity(),
            letter_spacing_em: d_letter_spacing(),
            shadow: true,
            tabular_figures: true,
            show_summary_line: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    #[serde(default = "d_anchor")]
    pub anchor: Anchor,
    #[serde(default = "d_offset")]
    pub offset_px: [i32; 2],
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            anchor: d_anchor(),
            offset_px: d_offset(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct General {
    #[serde(default)]
    pub autostart: bool,
}

/// Per-monitor overrides. Style fields sit at the same level as `enabled`,
/// not nested under a `[style]` table — see spec section 4.2.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayOverride {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_px: Option<[i32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_px: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DrawMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_width_px: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter_spacing_em: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabular_figures: Option<bool>,
    /// Legacy, as in `Style`. Read to migrate an older per-monitor override into a line list;
    /// never written back.
    #[serde(default, skip_serializing)]
    pub show_summary_line: Option<bool>,
    /// Replaces the global line list wholesale (no per-line merge). TOML: `[[display.line]]`.
    /// Must stay the last field: TOML rejects a scalar written after a table.
    #[serde(default, rename = "line", skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<Line>>,
}

fn d_target() -> DateTime {
    jiff::civil::datetime(2026, 12, 31, 23, 59, 59, 0)
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "d_target")]
    pub target: DateTime,
    #[serde(default)]
    pub style: Style,
    #[serde(default)]
    pub layout: Layout,
    #[serde(default)]
    pub general: General,
    /// The lines drawn, top to bottom. An empty list means "not configured" — `migrate` fills
    /// it in; it never means "draw nothing".
    #[serde(default, rename = "line", skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<Line>,
    #[serde(default, rename = "display", skip_serializing_if = "Vec::is_empty")]
    pub displays: Vec<DisplayOverride>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target: d_target(),
            style: Style::default(),
            layout: Layout::default(),
            general: General::default(),
            lines: default_lines(true),
            displays: Vec::new(),
        }
    }
}

/// Fills in the `[[line]]` list for a config written before lines existed (or hand-edited to
/// have none). The legacy `show_summary_line` flag is the only thing that decides the
/// synthesized list. It is left in place afterwards — it is never serialized, so it cannot
/// leak back into the file — but nothing else reads it.
pub fn migrate(cfg: &mut Config) {
    if cfg.lines.is_empty() {
        cfg.lines = default_lines(cfg.style.show_summary_line.unwrap_or(true));
    }
    for d in &mut cfg.displays {
        if d.lines.is_none() {
            if let Some(summary) = d.show_summary_line {
                d.lines = Some(default_lines(summary));
            }
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    #[error("invalid colour `{0}`, expected #RRGGBB")]
    Color(String),
    #[error("opacity must be within 0.0..=1.0, got {0}")]
    Opacity(f32),
    #[error("size_px must be greater than 0, got {0}")]
    Size(f32),
    #[error("font_weight must be within 100..=900, got {0}")]
    Weight(u16),
    #[error("outline_width_px must not be negative, got {0}")]
    OutlineWidth(f32),
    #[error("letter_spacing_em must be finite, got {0}")]
    LetterSpacing(f32),
    #[error("size_ratio must be greater than 0, got {0}")]
    SizeRatio(f32),
    #[error("line text must be at most 200 characters, got {0}")]
    TextTooLong(usize),
}

fn check_color(s: &str) -> Result<(), ConfigError> {
    parse_hex(s)
        .map(|_| ())
        .ok_or_else(|| ConfigError::Color(s.to_string()))
}

fn check_opacity(v: f32) -> Result<(), ConfigError> {
    if (0.0..=1.0).contains(&v) {
        Ok(())
    } else {
        Err(ConfigError::Opacity(v))
    }
}

fn check_size(v: f32) -> Result<(), ConfigError> {
    if v.is_finite() && v > 0.0 {
        Ok(())
    } else {
        Err(ConfigError::Size(v))
    }
}

fn check_weight(v: u16) -> Result<(), ConfigError> {
    if (100..=900).contains(&v) {
        Ok(())
    } else {
        Err(ConfigError::Weight(v))
    }
}

fn check_outline_width(v: f32) -> Result<(), ConfigError> {
    if v.is_finite() && v >= 0.0 {
        Ok(())
    } else {
        Err(ConfigError::OutlineWidth(v))
    }
}

fn check_letter_spacing(v: f32) -> Result<(), ConfigError> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(ConfigError::LetterSpacing(v))
    }
}

fn check_lines(lines: &[Line]) -> Result<(), ConfigError> {
    for l in lines {
        if !(l.size_ratio.is_finite() && l.size_ratio > 0.0) {
            return Err(ConfigError::SizeRatio(l.size_ratio));
        }
        let len = l.text.chars().count();
        if len > MAX_TEXT_LEN {
            return Err(ConfigError::TextTooLong(len));
        }
        if let Some(c) = &l.color {
            check_color(c)?;
        }
    }
    Ok(())
}

pub fn validate(cfg: &Config) -> Result<(), ConfigError> {
    let s = &cfg.style;
    check_color(&s.color)?;
    check_color(&s.outline_color)?;
    check_opacity(s.opacity)?;
    check_size(s.size_px)?;
    check_weight(s.font_weight)?;
    check_outline_width(s.outline_width_px)?;
    check_letter_spacing(s.letter_spacing_em)?;
    check_lines(&cfg.lines)?;

    for d in &cfg.displays {
        if let Some(v) = &d.color {
            check_color(v)?;
        }
        if let Some(v) = &d.outline_color {
            check_color(v)?;
        }
        if let Some(v) = d.opacity {
            check_opacity(v)?;
        }
        if let Some(v) = d.size_px {
            check_size(v)?;
        }
        if let Some(v) = d.font_weight {
            check_weight(v)?;
        }
        if let Some(v) = d.outline_width_px {
            check_outline_width(v)?;
        }
        if let Some(v) = d.letter_spacing_em {
            check_letter_spacing(v)?;
        }
        if let Some(lines) = &d.lines {
            check_lines(lines)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"target = "2026-10-24T09:00:00""#;

    #[test]
    fn minimal_config_fills_defaults() {
        let cfg: Config = toml::from_str(MINIMAL).unwrap();
        assert_eq!(cfg.style.font_family, "Consolas");
        assert_eq!(cfg.style.size_px, 64.0);
        assert_eq!(cfg.style.mode, DrawMode::Fill);
        assert_eq!(cfg.style.opacity, 0.85);
        assert!(cfg.style.shadow);
        assert!(cfg.style.tabular_figures);
        assert_eq!(cfg.style.show_summary_line, None);
        assert!(
            cfg.lines.is_empty(),
            "a file without [[line]] parses as empty; migrate fills it"
        );
        assert_eq!(cfg.layout.anchor, Anchor::Center);
        assert_eq!(cfg.layout.offset_px, [0, 0]);
        assert!(!cfg.general.autostart);
        assert!(cfg.displays.is_empty());
    }

    #[test]
    fn default_config_has_the_classic_two_lines() {
        let cfg = Config::default();
        assert_eq!(cfg.lines.len(), 2);
        assert_eq!(cfg.lines[0].text, "{months}m {weeks}w {days}d");
        assert_eq!(cfg.lines[0].size_ratio, 0.28);
        assert_eq!(cfg.lines[0].align, Align::Center);
        assert_eq!(cfg.lines[0].color, None);
        assert_eq!(cfg.lines[1].text, "{hh}:{mm}:{ss}");
        assert_eq!(cfg.lines[1].size_ratio, 1.0);
    }

    #[test]
    fn lines_are_parsed_from_an_array_of_tables() {
        // r##..##: the colour literal contains `"#`, which would end an r#..# string.
        let cfg: Config = toml::from_str(
            r##"
target = "2026-10-24T09:00:00"

[[line]]
text = "수능까지"
size_ratio = 0.25
align = "right"
color = "#AABBCC"

[[line]]
text = "{hh}:{mm}:{ss}"
"##,
        )
        .unwrap();
        assert_eq!(cfg.lines.len(), 2);
        assert_eq!(cfg.lines[0].text, "수능까지");
        assert_eq!(cfg.lines[0].align, Align::Right);
        assert_eq!(cfg.lines[0].color.as_deref(), Some("#AABBCC"));
        // Defaults fill the second line.
        assert_eq!(cfg.lines[1].size_ratio, 1.0);
        assert_eq!(cfg.lines[1].align, Align::Center);
        assert_eq!(cfg.lines[1].color, None);
    }

    #[test]
    fn a_display_override_can_replace_the_whole_line_list() {
        let cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"

[[display]]
id = "MON-A"
size_px = 48.0

[[display.line]]
text = "D-{daysTotal}"
"#,
        )
        .unwrap();
        let lines = cfg.displays[0].lines.as_ref().unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "D-{daysTotal}");
    }

    #[test]
    fn legacy_show_summary_line_still_parses_but_is_not_written_back() {
        let cfg: Config = toml::from_str(
            "target = \"2026-10-24T09:00:00\"\n[style]\nshow_summary_line = false\n",
        )
        .unwrap();
        assert_eq!(cfg.style.show_summary_line, Some(false));

        let text = toml::to_string_pretty(&cfg).unwrap();
        assert!(
            !text.contains("show_summary_line"),
            "the legacy flag must not be serialized: {text}"
        );
    }

    #[test]
    fn validate_rejects_nonpositive_size_ratio() {
        let mut cfg = Config::default();
        cfg.lines[0].size_ratio = 0.0;
        assert!(matches!(validate(&cfg), Err(ConfigError::SizeRatio(_))));
    }

    #[test]
    fn validate_rejects_bad_line_colour() {
        let mut cfg = Config::default();
        cfg.lines[0].color = Some("nope".into());
        assert!(matches!(validate(&cfg), Err(ConfigError::Color(_))));
    }

    #[test]
    fn validate_rejects_overlong_line_text() {
        let mut cfg = Config::default();
        cfg.lines[0].text = "가".repeat(MAX_TEXT_LEN + 1);
        assert!(matches!(validate(&cfg), Err(ConfigError::TextTooLong(_))));
    }

    #[test]
    fn validate_checks_lines_in_display_overrides_too() {
        let mut cfg = Config::default();
        cfg.displays.push(DisplayOverride {
            id: "MON-A".into(),
            lines: Some(vec![Line {
                size_ratio: -1.0,
                ..Line::default()
            }]),
            ..DisplayOverride::default()
        });
        assert!(matches!(validate(&cfg), Err(ConfigError::SizeRatio(_))));
    }

    #[test]
    fn migrate_synthesizes_the_classic_list_for_a_config_without_lines() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        assert!(cfg.lines.is_empty());
        migrate(&mut cfg);
        assert_eq!(cfg.lines, default_lines(true));
    }

    #[test]
    fn migrate_honours_the_legacy_summary_flag() {
        let mut cfg: Config = toml::from_str(
            "target = \"2026-10-24T09:00:00\"\n[style]\nshow_summary_line = false\n",
        )
        .unwrap();
        migrate(&mut cfg);
        assert_eq!(cfg.lines, default_lines(false));
        assert_eq!(cfg.lines.len(), 1);
        assert_eq!(cfg.lines[0].text, "{hh}:{mm}:{ss}");
    }

    #[test]
    fn migrate_leaves_an_existing_list_alone() {
        let mut cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"
[style]
show_summary_line = false

[[line]]
text = "kept"
"#,
        )
        .unwrap();
        migrate(&mut cfg);
        assert_eq!(cfg.lines.len(), 1);
        assert_eq!(cfg.lines[0].text, "kept");
    }

    #[test]
    fn migrate_converts_a_legacy_per_monitor_summary_flag_into_a_line_list() {
        let mut cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"

[[display]]
id = "MON-A"
show_summary_line = false
"#,
        )
        .unwrap();
        migrate(&mut cfg);
        assert_eq!(cfg.displays[0].lines, Some(default_lines(false)));
    }

    #[test]
    fn migrate_leaves_a_monitor_without_the_legacy_flag_following_the_globals() {
        let mut cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"

[[display]]
id = "MON-A"
size_px = 48.0
"#,
        )
        .unwrap();
        migrate(&mut cfg);
        assert_eq!(cfg.displays[0].lines, None);
    }

    #[test]
    fn anchor_uses_kebab_case() {
        let cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"
[layout]
anchor = "bottom-right"
offset_px = [-40, -80]
"#,
        )
        .unwrap();
        assert_eq!(cfg.layout.anchor, Anchor::BottomRight);
        assert_eq!(cfg.layout.offset_px, [-40, -80]);
    }

    #[test]
    fn display_overrides_are_parsed_flat() {
        let cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"

[[display]]
id = "\\\\?\\DISPLAY#DEL41A8#1"
name = "DISPLAY1 (세로)"
enabled = true
anchor = "top-center"
size_px = 48.0
"#,
        )
        .unwrap();
        assert_eq!(cfg.displays.len(), 1);
        let d = &cfg.displays[0];
        assert_eq!(d.enabled, Some(true));
        assert_eq!(d.anchor, Some(Anchor::TopCenter));
        assert_eq!(d.size_px, Some(48.0));
        assert_eq!(d.font_family, None);
    }

    #[test]
    fn draw_mode_is_lowercase() {
        let cfg: Config =
            toml::from_str("target = \"2026-10-24T09:00:00\"\n[style]\nmode = \"outline\"")
                .unwrap();
        assert_eq!(cfg.style.mode, DrawMode::Outline);
    }

    #[test]
    fn validate_accepts_defaults() {
        let cfg: Config = toml::from_str(MINIMAL).unwrap();
        assert!(validate(&cfg).is_ok());
    }

    #[test]
    fn validate_rejects_bad_colour() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        cfg.style.color = "not-a-colour".into();
        assert!(matches!(validate(&cfg), Err(ConfigError::Color(_))));
    }

    #[test]
    fn validate_rejects_out_of_range_opacity() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        cfg.style.opacity = 1.5;
        assert!(matches!(validate(&cfg), Err(ConfigError::Opacity(_))));
    }

    #[test]
    fn validate_rejects_nonpositive_size() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        cfg.style.size_px = 0.0;
        assert!(matches!(validate(&cfg), Err(ConfigError::Size(_))));
    }

    #[test]
    fn validate_rejects_bad_weight() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        cfg.style.font_weight = 50;
        assert!(matches!(validate(&cfg), Err(ConfigError::Weight(_))));
    }

    #[test]
    fn validate_checks_display_overrides_too() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        cfg.displays.push(DisplayOverride {
            id: "x".into(),
            opacity: Some(9.0),
            ..DisplayOverride::default()
        });
        assert!(matches!(validate(&cfg), Err(ConfigError::Opacity(_))));
    }

    #[test]
    fn default_config_round_trips_through_toml() {
        let cfg = Config::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn validate_rejects_infinite_size() {
        let cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"
[style]
size_px = inf
"#,
        )
        .unwrap();
        assert!(matches!(validate(&cfg), Err(ConfigError::Size(_))));
    }

    #[test]
    fn validate_rejects_infinite_outline_width() {
        let cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"
[style]
outline_width_px = inf
"#,
        )
        .unwrap();
        assert!(matches!(validate(&cfg), Err(ConfigError::OutlineWidth(_))));
    }

    #[test]
    fn validate_rejects_nan_opacity() {
        let cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"
[style]
opacity = nan
"#,
        )
        .unwrap();
        assert!(matches!(validate(&cfg), Err(ConfigError::Opacity(_))));
    }

    #[test]
    fn validate_rejects_nan_letter_spacing() {
        let cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"
[style]
letter_spacing_em = nan
"#,
        )
        .unwrap();
        assert!(matches!(validate(&cfg), Err(ConfigError::LetterSpacing(_))));
    }

    #[test]
    fn validate_rejects_infinite_letter_spacing() {
        let cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"
[style]
letter_spacing_em = inf
"#,
        )
        .unwrap();
        assert!(matches!(validate(&cfg), Err(ConfigError::LetterSpacing(_))));
    }

    #[test]
    fn validate_rejects_letter_spacing_in_display_override() {
        let cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"

[[display]]
id = "x"
letter_spacing_em = nan
"#,
        )
        .unwrap();
        assert!(matches!(validate(&cfg), Err(ConfigError::LetterSpacing(_))));
    }
}
