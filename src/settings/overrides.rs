//! Pure logic for per-monitor overrides in the settings window. No egui, no Win32.

use crate::config::{Config, DisplayOverride};

pub fn find_override<'a>(cfg: &'a Config, id: &str) -> Option<&'a DisplayOverride> {
    cfg.displays.iter().find(|d| d.id == id)
}

fn find_or_create<'a>(cfg: &'a mut Config, id: &str, name: &str) -> &'a mut DisplayOverride {
    if let Some(idx) = cfg.displays.iter().position(|d| d.id == id) {
        &mut cfg.displays[idx]
    } else {
        cfg.displays.push(DisplayOverride {
            id: id.to_string(),
            name: Some(name.to_string()),
            ..Default::default()
        });
        cfg.displays.last_mut().expect("just pushed")
    }
}

/// True if the override carries any style/anchor/offset field (i.e. not just id/name/enabled).
pub fn has_style_override(o: &DisplayOverride) -> bool {
    o.anchor.is_some()
        || o.offset_px.is_some()
        || o.font_family.is_some()
        || o.font_weight.is_some()
        || o.size_px.is_some()
        || o.mode.is_some()
        || o.color.is_some()
        || o.outline_color.is_some()
        || o.outline_width_px.is_some()
        || o.opacity.is_some()
        || o.letter_spacing_em.is_some()
        || o.shadow.is_some()
        || o.tabular_figures.is_some()
        || o.lines.is_some()
}

pub fn set_enabled(cfg: &mut Config, id: &str, name: &str, enabled: bool) {
    find_or_create(cfg, id, name).enabled = Some(enabled);
}

/// Copies the global style + layout + line list into the monitor's override so the user can
/// tweak from the current appearance rather than from blank defaults.
pub fn enable_style_override(cfg: &mut Config, id: &str, name: &str) {
    let g_style = cfg.style.clone();
    let g_layout = cfg.layout.clone();
    let g_lines = cfg.lines.clone();
    let o = find_or_create(cfg, id, name);
    o.anchor = Some(g_layout.anchor);
    o.offset_px = Some(g_layout.offset_px);
    o.font_family = Some(g_style.font_family);
    o.font_weight = Some(g_style.font_weight);
    o.size_px = Some(g_style.size_px);
    o.mode = Some(g_style.mode);
    o.color = Some(g_style.color);
    o.outline_color = Some(g_style.outline_color);
    o.outline_width_px = Some(g_style.outline_width_px);
    o.opacity = Some(g_style.opacity);
    o.letter_spacing_em = Some(g_style.letter_spacing_em);
    o.shadow = Some(g_style.shadow);
    o.tabular_figures = Some(g_style.tabular_figures);
    o.lines = Some(g_lines);
}

/// Clears the style/anchor/offset fields (monitor follows global again), keeps `enabled`,
/// and prunes the whole entry if nothing meaningful remains.
pub fn disable_style_override(cfg: &mut Config, id: &str) {
    if let Some(o) = cfg.displays.iter_mut().find(|d| d.id == id) {
        o.anchor = None;
        o.offset_px = None;
        o.font_family = None;
        o.font_weight = None;
        o.size_px = None;
        o.mode = None;
        o.color = None;
        o.outline_color = None;
        o.outline_width_px = None;
        o.opacity = None;
        o.letter_spacing_em = None;
        o.shadow = None;
        o.tabular_figures = None;
        o.lines = None;
    }
    // Prune entries that now hold only id (+name): no enabled, no style.
    cfg.displays
        .retain(|d| d.enabled.is_some() || has_style_override(d));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Anchor, Config, DrawMode};

    const ID: &str = "MON-A";
    const NAME: &str = "DISPLAY1";

    #[test]
    fn set_enabled_creates_then_updates_override() {
        let mut cfg = Config::default();
        assert!(find_override(&cfg, ID).is_none());

        set_enabled(&mut cfg, ID, NAME, false);
        let o = find_override(&cfg, ID).unwrap();
        assert_eq!(o.enabled, Some(false));
        assert_eq!(o.name.as_deref(), Some(NAME));
        assert!(
            !has_style_override(o),
            "enabling toggle must not add style fields"
        );

        set_enabled(&mut cfg, ID, NAME, true);
        assert_eq!(find_override(&cfg, ID).unwrap().enabled, Some(true));
        assert_eq!(cfg.displays.len(), 1, "must not duplicate the entry");
    }

    #[test]
    fn enable_style_override_copies_global_values() {
        let mut cfg = Config::default();
        cfg.style.size_px = 80.0;
        cfg.layout.anchor = Anchor::TopCenter;

        enable_style_override(&mut cfg, ID, NAME);
        let o = find_override(&cfg, ID).unwrap();
        assert!(has_style_override(o));
        assert_eq!(o.size_px, Some(80.0));
        assert_eq!(o.anchor, Some(Anchor::TopCenter));
        // A field the user has not changed still mirrors the global default.
        assert_eq!(o.mode, Some(DrawMode::Fill));
    }

    #[test]
    fn disable_style_override_clears_style_but_keeps_enabled() {
        let mut cfg = Config::default();
        set_enabled(&mut cfg, ID, NAME, false); // enabled = Some(false)
        enable_style_override(&mut cfg, ID, NAME); // adds style fields

        disable_style_override(&mut cfg, ID);
        let o = find_override(&cfg, ID).unwrap();
        assert!(!has_style_override(o), "style fields must be cleared");
        assert_eq!(o.enabled, Some(false), "enabled must survive");
    }

    #[test]
    fn disable_removes_entry_when_nothing_left() {
        let mut cfg = Config::default();
        enable_style_override(&mut cfg, ID, NAME); // only style fields, enabled is None
        assert_eq!(cfg.displays.len(), 1);

        disable_style_override(&mut cfg, ID);
        // enabled is None and style is cleared → the entry holds only id+name → remove it.
        assert!(cfg.displays.is_empty(), "empty override should be pruned");
    }

    #[test]
    fn disable_keeps_entry_when_enabled_is_set() {
        let mut cfg = Config::default();
        set_enabled(&mut cfg, ID, NAME, false);
        enable_style_override(&mut cfg, ID, NAME);
        disable_style_override(&mut cfg, ID);
        assert_eq!(cfg.displays.len(), 1, "enabled=Some keeps the entry alive");
    }

    /// Safety net for `has_style_override`'s field list: setting any single style/anchor/offset
    /// field must make it report true. Without this, a field dropped from the OR-chain goes
    /// unnoticed (the enable/disable tests only ever set or clear all 14 at once). This guards the
    /// gap once per-field editing (Task 7 UI) can leave a lone field set.
    #[test]
    fn has_style_override_detects_each_field_individually() {
        use crate::config::{Anchor, DrawMode};

        let base = DisplayOverride {
            id: ID.into(),
            ..Default::default()
        };
        assert!(
            !has_style_override(&base),
            "an override with only id must report no style"
        );

        type Setter = fn(&mut DisplayOverride);
        let setters: [Setter; 14] = [
            |o| o.anchor = Some(Anchor::Center),
            |o| o.offset_px = Some([1, 2]),
            |o| o.font_family = Some("X".into()),
            |o| o.font_weight = Some(400),
            |o| o.size_px = Some(1.0),
            |o| o.mode = Some(DrawMode::Fill),
            |o| o.color = Some("#000000".into()),
            |o| o.outline_color = Some("#000000".into()),
            |o| o.outline_width_px = Some(1.0),
            |o| o.opacity = Some(0.5),
            |o| o.letter_spacing_em = Some(0.0),
            |o| o.shadow = Some(true),
            |o| o.tabular_figures = Some(true),
            |o| o.lines = Some(crate::config::default_lines(true)),
        ];
        for (i, set) in setters.iter().enumerate() {
            let mut o = base.clone();
            set(&mut o);
            assert!(
                has_style_override(&o),
                "has_style_override missed field index {i}"
            );
        }
    }
}
