//! System tray icon and menu. The only way to quit a wallpaper-layer app.

use anyhow::Result;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub const ICON_SIZE: u32 = 32;

/// Resource id of `assets/icon.ico`, embedded into the exe by `build.rs`. Explorer shows the
/// same resource as the exe's icon, so the tray and the executable never drift apart.
const ICON_RESOURCE_ID: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    OpenConfig,
    Reload,
    Quit,
}

/// The embedded icon, or a plain disc if the resource is missing — a build whose resource
/// compiler was unavailable (see `build.rs`) still has to reach the tray, because the tray is
/// the only way to quit a wallpaper-layer app.
fn icon() -> Result<Icon> {
    match Icon::from_resource(ICON_RESOURCE_ID, Some((ICON_SIZE, ICON_SIZE))) {
        Ok(icon) => Ok(icon),
        Err(e) => {
            tracing::warn!("no embedded icon ({e}), falling back to a plain disc");
            Ok(Icon::from_rgba(disc_rgba(ICON_SIZE), ICON_SIZE, ICON_SIZE)?)
        }
    }
}

fn disc_rgba(size: u32) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let c = (size as f32 - 1.0) / 2.0;
    let r = c - 1.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            if (dx * dx + dy * dy).sqrt() <= r {
                let i = ((y * size + x) * 4) as usize;
                px[i] = 0xE8;
                px[i + 1] = 0xEE;
                px[i + 2] = 0xF7;
                px[i + 3] = 0xFF;
            }
        }
    }
    px
}

pub struct Tray {
    icon: TrayIcon,
    open_id: MenuId,
    reload_id: MenuId,
    quit_id: MenuId,
}

impl Tray {
    pub fn new() -> Result<Self> {
        let open = MenuItem::new("Open settings", true, None);
        let reload = MenuItem::new("Reload config", true, None);
        let quit = MenuItem::new("Quit", true, None);

        let menu = Menu::new();
        menu.append_items(&[&open, &PredefinedMenuItem::separator(), &reload, &quit])?;

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("DesktopCountdown")
            .with_icon(icon()?)
            .build()?;

        Ok(Self {
            icon,
            open_id: open.id().clone(),
            reload_id: reload.id().clone(),
            quit_id: quit.id().clone(),
        })
    }

    /// Non-blocking: must never stall the Win32 message loop that calls this
    /// once per `tick()`. Menu click events arrive via a global receiver that
    /// `tray-icon` populates from the same thread's message pump.
    pub fn poll(&self) -> Option<TrayCommand> {
        let ev = MenuEvent::receiver().try_recv().ok()?;
        if ev.id == self.open_id {
            Some(TrayCommand::OpenConfig)
        } else if ev.id == self.reload_id {
            Some(TrayCommand::Reload)
        } else if ev.id == self.quit_id {
            Some(TrayCommand::Quit)
        } else {
            None
        }
    }

    /// Toggles a warning marker in the tooltip. Wired into both branches of
    /// `App::reload()`: on for a rejected config, off once a reload succeeds.
    pub fn set_warning(&self, on: bool) -> Result<()> {
        tracing::debug!(on, "tray: set_warning");
        let tip = if on {
            "DesktopCountdown — 설정 오류 (log.txt 확인)"
        } else {
            "DesktopCountdown"
        };
        self.icon.set_tooltip(Some(tip))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_disc_pixels_have_the_expected_length() {
        let px = disc_rgba(ICON_SIZE);
        assert_eq!(px.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
    }

    #[test]
    fn fallback_disc_centre_is_opaque_and_corners_are_transparent() {
        let s = ICON_SIZE;
        let px = disc_rgba(s);
        let at = |x: u32, y: u32| px[((y * s + x) * 4 + 3) as usize];
        assert_eq!(at(0, 0), 0, "corner should be transparent");
        assert_eq!(at(s - 1, s - 1), 0, "corner should be transparent");
        assert!(at(s / 2, s / 2) > 0, "centre should be drawn");
    }
}
