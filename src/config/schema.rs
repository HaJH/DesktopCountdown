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
    #[serde(default = "d_true")]
    pub show_summary_line: bool,
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
            show_summary_line: true,
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
        Self { anchor: d_anchor(), offset_px: d_offset() }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_summary_line: Option<bool>,
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
            displays: Vec::new(),
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
}

fn check_color(s: &str) -> Result<(), ConfigError> {
    parse_hex(s).map(|_| ()).ok_or_else(|| ConfigError::Color(s.to_string()))
}

fn check_opacity(v: f32) -> Result<(), ConfigError> {
    if (0.0..=1.0).contains(&v) {
        Ok(())
    } else {
        Err(ConfigError::Opacity(v))
    }
}

fn check_size(v: f32) -> Result<(), ConfigError> {
    if v > 0.0 {
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
    if v >= 0.0 {
        Ok(())
    } else {
        Err(ConfigError::OutlineWidth(v))
    }
}

pub fn validate(cfg: &Config) -> Result<(), ConfigError> {
    let s = &cfg.style;
    check_color(&s.color)?;
    check_color(&s.outline_color)?;
    check_opacity(s.opacity)?;
    check_size(s.size_px)?;
    check_weight(s.font_weight)?;
    check_outline_width(s.outline_width_px)?;

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
        assert!(cfg.style.show_summary_line);
        assert_eq!(cfg.layout.anchor, Anchor::Center);
        assert_eq!(cfg.layout.offset_px, [0, 0]);
        assert!(!cfg.general.autostart);
        assert!(cfg.displays.is_empty());
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
            toml::from_str("target = \"2026-10-24T09:00:00\"\n[style]\nmode = \"outline\"").unwrap();
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
}
