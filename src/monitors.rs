//! Monitor enumeration with stable per-device identifiers.

use std::cell::RefCell;

use anyhow::Result;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW, HDC, HMONITOR,
    MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME;

use crate::layout::Rect;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorInfo {
    /// Stable across reboots and cable swaps, e.g. `\\?\DISPLAY#DEL41A8#...`.
    pub id: String,
    /// Display only, e.g. `\\.\DISPLAY1 (2560x1440)`. Never used for identity.
    pub name: String,
    /// Virtual-desktop coordinates in physical pixels. May be negative.
    pub rect: Rect,
    pub dpi: u32,
}

thread_local! {
    static SINK: RefCell<Vec<MonitorInfo>> = const { RefCell::new(Vec::new()) };
}

pub fn enumerate() -> Result<Vec<MonitorInfo>> {
    SINK.with(|s| s.borrow_mut().clear());
    unsafe {
        EnumDisplayMonitors(None, None, Some(monitor_cb), LPARAM(0)).ok()?;
    }
    Ok(SINK.with(|s| s.borrow().clone()))
}

unsafe extern "system" fn monitor_cb(hmon: HMONITOR, _hdc: HDC, _rc: *mut RECT, _lp: LPARAM) -> BOOL {
    if let Some(info) = describe(hmon) {
        SINK.with(|s| s.borrow_mut().push(info));
    }
    TRUE
}

unsafe fn describe(hmon: HMONITOR) -> Option<MonitorInfo> {
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    GetMonitorInfoW(hmon, &mut mi.monitorInfo as *mut _).ok().ok()?;

    let device = wide_to_string(&mi.szDevice);
    let r = mi.monitorInfo.rcMonitor;
    let rect = Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top };

    let mut dpi_x = 96u32;
    let mut dpi_y = 96u32;
    let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);

    // The device interface name survives reboots and port changes; szDevice does not.
    let mut dd = DISPLAY_DEVICEW { cb: size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
    let device_wide: Vec<u16> = mi.szDevice.to_vec();
    let ok = EnumDisplayDevicesW(
        windows::core::PCWSTR(device_wide.as_ptr()),
        0,
        &mut dd,
        EDD_GET_DEVICE_INTERFACE_NAME,
    )
    .as_bool();

    let id = if ok && dd.DeviceID[0] != 0 {
        wide_to_string(&dd.DeviceID)
    } else {
        // Fall back to the unstable name rather than dropping the monitor entirely.
        device.clone()
    };

    Some(MonitorInfo {
        id,
        name: format!("{} ({}x{})", device, rect.w, rect.h),
        rect,
        dpi: dpi_x,
    })
}

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn enumerates_at_least_one_monitor() {
        let ms = enumerate().unwrap();
        assert!(!ms.is_empty());
    }

    #[test]
    fn every_monitor_has_a_positive_size_and_dpi() {
        for m in enumerate().unwrap() {
            assert!(m.rect.w > 0, "{m:?}");
            assert!(m.rect.h > 0, "{m:?}");
            assert!(m.dpi >= 96, "{m:?}");
        }
    }

    #[test]
    fn ids_are_unique_and_nonempty() {
        let ms = enumerate().unwrap();
        let ids: HashSet<_> = ms.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids.len(), ms.len(), "duplicate monitor ids: {ms:#?}");
        assert!(ms.iter().all(|m| !m.id.is_empty()));
    }

    #[test]
    fn names_are_nonempty() {
        assert!(enumerate().unwrap().iter().all(|m| !m.name.is_empty()));
    }

    /// Not run by default: prints the real monitor layout for manual inspection.
    /// Run with `cargo test monitors -- --ignored --nocapture`.
    ///
    /// `cargo test` never calls `SetProcessDpiAwarenessContext` (only `main.rs` does), so
    /// without setting it here the test process is DPI-unaware and Windows silently
    /// virtualizes/scales `GetMonitorInfoW`'s rect and `GetDpiForMonitor`'s result for any
    /// monitor whose scaling isn't 100%. Set it explicitly so this test observes the same
    /// physical-pixel values the real app would.
    #[test]
    #[ignore]
    fn prints_the_real_monitor_layout() {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        for m in enumerate().unwrap() {
            println!("{m:#?}");
        }
    }
}
