//! Finds the window Explorer paints the wallpaper into, and parents our windows to it.
//!
//! Two arrangements exist in the wild:
//!
//! * "classic": `SHELLDLL_DefView` lives inside a top-level `WorkerW`, and the wallpaper
//!   `WorkerW` is the next top-level sibling after it.
//! * "child of Progman": `SHELLDLL_DefView` stays inside `Progman`, and the wallpaper
//!   `WorkerW` is a *child* of `Progman`, below the icons. Windows 11 build 26200 does this.
//!
//! `0x052C` asks Explorer to create the `WorkerW` if none exists; it is a no-op once one does.

use std::ptr::null_mut;
use std::sync::Once;

use anyhow::{anyhow, Result};
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::layout::Rect;

const CHILD_CLASS: PCWSTR = w!("DesktopCountdownChild");
static REGISTER: Once = Once::new();

/// Which of the two arrangements documented at module level actually matched.
/// `pub(crate)` only: this exists so a test can observe/log which strategy this machine
/// uses, without widening `acquire`'s public `-> Result<HWND>` signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Strategy {
    /// Top-level `WorkerW` sibling of the top-level window that owns `SHELLDLL_DefView`.
    Classic,
    /// `WorkerW` is a direct child of `Progman`.
    ProgmanChild,
}

impl Strategy {
    fn label(self) -> &'static str {
        match self {
            Strategy::Classic => "classic (top-level sibling)",
            Strategy::ProgmanChild => "child of Progman",
        }
    }
}

pub fn acquire() -> Result<HWND> {
    Ok(acquire_with_strategy()?.0)
}

/// Same lookup as `acquire`, but also reports which strategy matched. Used by tests/
/// diagnostics only; kept `pub(crate)` so it never becomes part of the public API surface.
pub(crate) fn acquire_with_strategy() -> Result<(HWND, Strategy)> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), None)?;
        if let Some(found) = lookup(progman) {
            tracing::info!(strategy = found.1.label(), hwnd = ?found.0, "found wallpaper WorkerW");
            return Ok(found);
        }
        for (wp, lp) in [(0usize, 0isize), (0xD, 0x1), (0xD, 0x0)] {
            let mut res = 0usize;
            SendMessageTimeoutW(
                progman,
                0x052C,
                WPARAM(wp),
                LPARAM(lp),
                SMTO_NORMAL,
                1000,
                Some(&mut res),
            );
            if let Some(found) = lookup(progman) {
                tracing::info!(strategy = found.1.label(), hwnd = ?found.0, "found wallpaper WorkerW after 0x052C");
                return Ok(found);
            }
        }
        Err(anyhow!(
            "no WorkerW found by either strategy, even after 0x052C"
        ))
    }
}

unsafe fn lookup(progman: HWND) -> Option<(HWND, Strategy)> {
    if let Some(h) = classic_sibling() {
        return Some((h, Strategy::Classic));
    }
    FindWindowExW(Some(progman), None, w!("WorkerW"), None)
        .ok()
        .map(|h| (h, Strategy::ProgmanChild))
}

pub fn is_alive(hwnd: HWND) -> bool {
    !hwnd.0.is_null() && unsafe { IsWindow(Some(hwnd)) }.as_bool()
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None).is_ok() {
        if let Ok(worker) = FindWindowExW(None, Some(hwnd), w!("WorkerW"), None) {
            *(lparam.0 as *mut HWND) = worker;
            return BOOL(0);
        }
    }
    BOOL(1)
}

unsafe fn classic_sibling() -> Option<HWND> {
    let mut out = HWND(null_mut());
    let _ = EnumWindows(Some(enum_cb), LPARAM(&mut out as *mut HWND as isize));
    (!out.0.is_null()).then_some(out)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wp, lp)
}

/// A window parented into the wallpaper layer. Its pixels come from `dcomp`, not WM_PAINT:
/// a plain child window's pixels are overpainted whenever Explorer redraws the wallpaper.
pub struct ChildWindow {
    hwnd: HWND,
}

impl ChildWindow {
    pub fn create(parent: HWND) -> Result<Self> {
        unsafe {
            let hinst = GetModuleHandleW(None)?;
            REGISTER.call_once(|| {
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(wndproc),
                    hInstance: hinst.into(),
                    lpszClassName: CHILD_CLASS,
                    ..Default::default()
                };
                RegisterClassW(&wc);
            });

            // CreateWindowEx rejects a parent owned by another process (explorer.exe):
            // it returns NULL with GetLastError() == 0. Create top-level, then reparent.
            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                CHILD_CLASS,
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(hinst.into()),
                None,
            )?;
            SetParent(hwnd, Some(parent))?;
            // SetParent does not adjust the style; a reparented window must be WS_CHILD.
            SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_CHILD.0 | WS_VISIBLE.0) as isize);
            // MSDN: setting WS_VISIBLE via SetWindowLongPtr does not make the window appear.
            let _ = ShowWindow(hwnd, SW_SHOW);
            Ok(Self { hwnd })
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// `rect` is in virtual-desktop screen coordinates.
    pub fn place(&self, parent: HWND, rect: Rect) -> Result<()> {
        unsafe {
            let mut o = POINT {
                x: rect.x,
                y: rect.y,
            };
            ScreenToClient(parent, &mut o).ok()?;
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOP),
                o.x,
                o.y,
                rect.w,
                rect.h,
                SWP_NOACTIVATE | SWP_FRAMECHANGED,
            )?;
        }
        Ok(())
    }

    /// Other wallpaper apps (Wallpaper Engine) parent their surfaces to the same WorkerW.
    /// Nothing guarantees we stay above them, so check and raise. Two calls; do it per tick.
    pub fn raise_if_covered(&self, parent: HWND) {
        unsafe {
            if GetWindow(parent, GW_CHILD).ok() != Some(self.hwnd) {
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOP),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                );
            }
        }
    }
}

impl Drop for ChildWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_null_handle_is_not_alive() {
        assert!(!is_alive(HWND(std::ptr::null_mut())));
    }

    #[test]
    #[ignore = "requires a live Explorer desktop; run with --ignored"]
    fn acquires_a_live_workerw() {
        let hwnd = acquire().unwrap();
        assert!(is_alive(hwnd));
    }

    #[test]
    #[ignore = "requires a live Explorer desktop; run with --ignored"]
    fn creates_a_child_and_places_it_in_screen_coordinates() {
        use windows::Win32::Foundation::RECT;
        use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

        let parent = acquire().unwrap();
        let child = ChildWindow::create(parent).unwrap();
        child
            .place(
                parent,
                Rect {
                    x: 120,
                    y: 140,
                    w: 300,
                    h: 80,
                },
            )
            .unwrap();

        // GetWindowRect reports screen coordinates even for a child window.
        let mut r = RECT::default();
        unsafe { GetWindowRect(child.hwnd(), &mut r) }.unwrap();
        assert_eq!((r.left, r.top, r.right, r.bottom), (120, 140, 420, 220));
    }

    /// Not run by default: prints which WorkerW-lookup strategy this machine needs.
    /// Run with `cargo test workerw -- --ignored --nocapture`.
    #[test]
    #[ignore = "requires a live Explorer desktop; run with --ignored"]
    fn acquire_reports_which_strategy_matched() {
        let (hwnd, strategy) = acquire_with_strategy().unwrap();
        println!("strategy = {}, hwnd = {hwnd:?}", strategy.label());
        assert!(is_alive(hwnd));
    }
}
