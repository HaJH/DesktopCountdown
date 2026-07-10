//! Merges global defaults with per-monitor overrides. No Win32, no I/O.

use super::{Anchor, Config, Style};

/// The resolved settings for one monitor.
#[derive(Debug, Clone, PartialEq)]
pub struct Effective {
    pub enabled: bool,
    pub anchor: Anchor,
    pub offset_px: [i32; 2],
    pub style: Style,
}

/// Only fields present in the matching `[[display]]` entry override the globals.
/// A monitor with no entry gets the globals and is enabled.
pub fn effective_for(cfg: &Config, monitor_id: &str) -> Effective {
    let mut e = Effective {
        enabled: true,
        anchor: cfg.layout.anchor,
        offset_px: cfg.layout.offset_px,
        style: cfg.style.clone(),
    };

    let Some(o) = cfg.displays.iter().find(|d| d.id == monitor_id) else {
        return e;
    };

    if let Some(v) = o.enabled { e.enabled = v; }
    if let Some(v) = o.anchor { e.anchor = v; }
    if let Some(v) = o.offset_px { e.offset_px = v; }

    if let Some(v) = &o.font_family { e.style.font_family = v.clone(); }
    if let Some(v) = o.font_weight { e.style.font_weight = v; }
    if let Some(v) = o.size_px { e.style.size_px = v; }
    if let Some(v) = o.mode { e.style.mode = v; }
    if let Some(v) = &o.color { e.style.color = v.clone(); }
    if let Some(v) = &o.outline_color { e.style.outline_color = v.clone(); }
    if let Some(v) = o.outline_width_px { e.style.outline_width_px = v; }
    if let Some(v) = o.opacity { e.style.opacity = v; }
    if let Some(v) = o.letter_spacing_em { e.style.letter_spacing_em = v; }
    if let Some(v) = o.shadow { e.style.shadow = v; }
    if let Some(v) = o.tabular_figures { e.style.tabular_figures = v; }
    if let Some(v) = o.show_summary_line { e.style.show_summary_line = v; }

    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Anchor, Config, DisplayOverride, DrawMode};

    fn cfg_with(over: Vec<DisplayOverride>) -> Config {
        Config { displays: over, ..Config::default() }
    }

    #[test]
    fn no_override_yields_global_defaults() {
        let cfg = cfg_with(vec![]);
        let e = effective_for(&cfg, "MON-A");
        assert!(e.enabled);
        assert_eq!(e.anchor, Anchor::Center);
        assert_eq!(e.offset_px, [0, 0]);
        assert_eq!(e.style.size_px, 64.0);
        assert_eq!(e.style.font_family, "Consolas");
    }

    #[test]
    fn unrelated_override_is_ignored() {
        let cfg = cfg_with(vec![DisplayOverride {
            id: "MON-B".into(),
            size_px: Some(120.0),
            ..DisplayOverride::default()
        }]);
        assert_eq!(effective_for(&cfg, "MON-A").style.size_px, 64.0);
    }

    #[test]
    fn partial_override_replaces_only_present_fields() {
        let cfg = cfg_with(vec![DisplayOverride {
            id: "MON-A".into(),
            anchor: Some(Anchor::TopCenter),
            size_px: Some(48.0),
            ..DisplayOverride::default()
        }]);
        let e = effective_for(&cfg, "MON-A");
        assert_eq!(e.anchor, Anchor::TopCenter);
        assert_eq!(e.style.size_px, 48.0);
        // untouched fields keep global values
        assert_eq!(e.style.font_family, "Consolas");
        assert_eq!(e.style.opacity, 0.85);
        assert_eq!(e.offset_px, [0, 0]);
        assert!(e.enabled);
    }

    #[test]
    fn enabled_false_is_respected() {
        let cfg = cfg_with(vec![DisplayOverride {
            id: "MON-A".into(),
            enabled: Some(false),
            ..DisplayOverride::default()
        }]);
        assert!(!effective_for(&cfg, "MON-A").enabled);
    }

    #[test]
    fn every_style_field_can_be_overridden() {
        let cfg = cfg_with(vec![DisplayOverride {
            id: "MON-A".into(),
            font_family: Some("Impact".into()),
            font_weight: Some(800),
            size_px: Some(10.0),
            mode: Some(DrawMode::Both),
            color: Some("#112233".into()),
            outline_color: Some("#445566".into()),
            outline_width_px: Some(3.0),
            opacity: Some(0.1),
            letter_spacing_em: Some(0.5),
            shadow: Some(false),
            tabular_figures: Some(false),
            show_summary_line: Some(false),
            ..DisplayOverride::default()
        }]);
        let s = effective_for(&cfg, "MON-A").style;
        assert_eq!(s.font_family, "Impact");
        assert_eq!(s.font_weight, 800);
        assert_eq!(s.size_px, 10.0);
        assert_eq!(s.mode, DrawMode::Both);
        assert_eq!(s.color, "#112233");
        assert_eq!(s.outline_color, "#445566");
        assert_eq!(s.outline_width_px, 3.0);
        assert_eq!(s.opacity, 0.1);
        assert_eq!(s.letter_spacing_em, 0.5);
        assert!(!s.shadow);
        assert!(!s.tabular_figures);
        assert!(!s.show_summary_line);
    }

    #[test]
    fn first_matching_override_wins() {
        let cfg = cfg_with(vec![
            DisplayOverride { id: "MON-A".into(), size_px: Some(10.0), ..Default::default() },
            DisplayOverride { id: "MON-A".into(), size_px: Some(20.0), ..Default::default() },
        ]);
        assert_eq!(effective_for(&cfg, "MON-A").style.size_px, 10.0);
    }
}
