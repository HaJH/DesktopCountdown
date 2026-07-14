//! Menu bar item and menu. The only way to quit a wallpaper-layer app.

use anyhow::{anyhow, Result};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// The menu bar wants a small icon; anything larger is downscaled, and badly.
pub const ICON_SIZE: u32 = 22;

/// The same PNG the `.app` bundle's `.icns` is generated from, so the menu bar and the
/// application icon never drift apart. Windows reaches the same asset through the exe's
/// resource table; macOS has no such thing, so it is embedded directly.
const ICON_PNG: &[u8] = include_bytes!("../../../assets/icon.png");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    OpenConfig,
    Reload,
    Quit,
}

/// The embedded icon, or a plain disc if it will not decode -- the menu bar item is the
/// only way to quit a wallpaper-layer app, so it has to appear even if the artwork does not.
fn icon() -> Result<Icon> {
    match decode_png(ICON_PNG) {
        Ok((rgba, w, h)) => Ok(Icon::from_rgba(rgba, w, h)?),
        Err(e) => {
            tracing::warn!("could not decode the menu bar icon ({e:#}), falling back to a disc");
            Ok(Icon::from_rgba(disc_rgba(ICON_SIZE), ICON_SIZE, ICON_SIZE)?)
        }
    }
}

fn decode_png(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32)> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());

    // `tray_icon::Icon::from_rgba` takes RGBA8 and nothing else.
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(anyhow!(
            "the icon is {:?}/{:?}, not 8-bit RGBA",
            info.color_type,
            info.bit_depth
        ));
    }
    Ok((buf, info.width, info.height))
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
        let open = MenuItem::new("설정 열기", true, None);
        let reload = MenuItem::new("다시 불러오기", true, None);
        let quit = MenuItem::new("종료", true, None);

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

    /// Non-blocking: must never stall the run loop that calls this once per `tick()`.
    /// Menu click events arrive via a global receiver that `tray-icon` populates from the
    /// main thread's own run loop.
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

    /// Toggles a warning marker in the tooltip: on for a rejected config, off once a reload
    /// succeeds.
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

    /// The bundled artwork has to be exactly what `Icon::from_rgba` accepts, or every run
    /// silently falls back to the plain disc.
    #[test]
    fn the_embedded_icon_decodes_as_rgba8() {
        let (rgba, w, h) = decode_png(ICON_PNG).expect("the bundled icon must decode");
        assert!(w > 0 && h > 0);
        assert_eq!(rgba.len(), (w * h * 4) as usize);
    }

    #[test]
    fn the_fallback_disc_is_the_size_it_claims() {
        let px = disc_rgba(ICON_SIZE);
        assert_eq!(px.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        // The centre is inside the disc, the corner is outside it.
        let centre = ((ICON_SIZE / 2 * ICON_SIZE + ICON_SIZE / 2) * 4 + 3) as usize;
        assert_eq!(px[centre], 0xFF);
        assert_eq!(px[3], 0x00);
    }
}
