//! Throwaway diagnostic: three child windows of WorkerW, side by side, each rendering
//! a different way. Whichever ones appear tell us what the wallpaper layer supports.
//!
//!   A (100,100)  : plain child, no layering, solid RED painted in WM_PAINT
//!   B (750,100)  : layered child + SetLayeredWindowAttributes(alpha 128), solid GREEN
//!   C (1400,100) : layered child + UpdateLayeredWindow, per-pixel alpha RED gradient
//!
//! All three get an explicit ShowWindow(SW_SHOW), so "we never showed it" is ruled out
//! for all three equally. Not part of the product.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{copy_nonoverlapping, null_mut};

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, CreateDIBSection, CreateSolidBrush, DeleteDC, DeleteObject,
    EndPaint, FillRect, GetDC, ReleaseDC, ScreenToClient, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBRUSH, HGDIOBJ,
    PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowExW, FindWindowW, GetMessageW,
    RegisterClassW, SetLayeredWindowAttributes, SetParent, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, UpdateLayeredWindow, GWL_STYLE, HWND_TOP, LWA_ALPHA, MSG,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SW_SHOW, ULW_ALPHA, WM_PAINT, WNDCLASSW, WS_CHILD,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
    WINDOW_EX_STYLE,
};

const W: i32 = 600;
const H: i32 = 300;

/// COLORREF is 0x00BBGGRR.
const RED: u32 = 0x0000_00FF;
const GREEN: u32 = 0x0000_FF00;

static mut PAINT_COLOR: u32 = RED;

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let rect = RECT { left: 0, top: 0, right: W, bottom: H };
        let brush: HBRUSH = CreateSolidBrush(COLORREF(PAINT_COLOR));
        FillRect(hdc, &rect, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
        let _ = EndPaint(hwnd, &ps);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

/// Creates a top-level window and reparents it into `workerw` at (`sx`, `sy`) screen coords.
unsafe fn make_child(workerw: HWND, ex: WINDOW_EX_STYLE, sx: i32, sy: i32) -> Result<HWND> {
    let hinst = GetModuleHandleW(None)?;
    let hwnd = CreateWindowExW(
        ex,
        w!("DiagPaintChild"),
        PCWSTR::null(),
        WS_POPUP,
        0,
        0,
        W,
        H,
        None,
        None,
        Some(hinst.into()),
        None,
    )?;
    SetParent(hwnd, Some(workerw))?;
    SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_CHILD.0 | WS_VISIBLE.0) as isize);

    let mut origin = POINT { x: sx, y: sy };
    ScreenToClient(workerw, &mut origin).ok()?;
    SetWindowPos(hwnd, Some(HWND_TOP), origin.x, origin.y, W, H, SWP_NOACTIVATE | SWP_FRAMECHANGED)?;
    let _ = ShowWindow(hwnd, SW_SHOW);
    Ok(hwnd)
}

fn gradient_pixels() -> Vec<u8> {
    let mut px = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let a = (x * 255 / (W - 1)) as u8;
            let i = ((y * W + x) * 4) as usize;
            px[i] = 0;
            px[i + 1] = 0;
            px[i + 2] = a;
            px[i + 3] = a;
        }
    }
    px
}

unsafe fn push_gradient(hwnd: HWND) -> Result<()> {
    let pixels = gradient_pixels();
    let hdc_screen = GetDC(None);
    let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
    let bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: W,
            biHeight: -H,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = null_mut();
    let hbmp = CreateDIBSection(Some(hdc_mem), &bi, DIB_RGB_COLORS, &mut bits, None, 0)?;
    copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
    let old = SelectObject(hdc_mem, HGDIOBJ(hbmp.0));

    let size = SIZE { cx: W, cy: H };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let r = UpdateLayeredWindow(
        hwnd,
        Some(hdc_screen),
        None,
        Some(&size),
        Some(hdc_mem),
        Some(&src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );
    println!("  UpdateLayeredWindow -> {r:?}");

    SelectObject(hdc_mem, old);
    let _ = DeleteObject(HGDIOBJ(hbmp.0));
    let _ = DeleteDC(hdc_mem);
    ReleaseDC(None, hdc_screen);
    Ok(())
}

unsafe fn acquire_workerw() -> Result<HWND> {
    let progman = FindWindowW(w!("Progman"), None)?;
    for (wp, lp) in [(0usize, 0isize), (0xD, 0x1)] {
        if let Ok(h) = FindWindowExW(Some(progman), None, w!("WorkerW"), None) {
            return Ok(h);
        }
        let mut res = 0usize;
        windows::Win32::UI::WindowsAndMessaging::SendMessageTimeoutW(
            progman, 0x052C, WPARAM(wp), LPARAM(lp),
            windows::Win32::UI::WindowsAndMessaging::SMTO_NORMAL, 1000, Some(&mut res),
        );
    }
    FindWindowExW(Some(progman), None, w!("WorkerW"), None)
}

fn main() -> Result<()> {
    unsafe {
        let workerw = acquire_workerw()?;
        println!("WorkerW = {:?}", workerw.0);

        let hinst = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: w!("DiagPaintChild"),
            ..Default::default()
        };
        RegisterClassW(&wc);

        println!("A: plain child, solid RED, at (100,100)");
        PAINT_COLOR = RED;
        let _a = make_child(workerw, WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW, 100, 100)?;

        println!("B: layered child + SetLayeredWindowAttributes(128), solid GREEN, at (750,100)");
        let b = make_child(
            workerw,
            WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            750,
            100,
        )?;
        PAINT_COLOR = GREEN;
        let r = SetLayeredWindowAttributes(b, COLORREF(0), 128, LWA_ALPHA);
        println!("  SetLayeredWindowAttributes -> {r:?}");

        println!("C: layered child + UpdateLayeredWindow gradient, at (1400,100)");
        let c = make_child(
            workerw,
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            1400,
            100,
        )?;
        push_gradient(c)?;

        println!("\nAll three are up. Look at the primary monitor, y=100..400:");
        println!("  x  100.. 700 -> A solid red   (plain child)");
        println!("  x  750..1350 -> B solid green (layered, constant alpha)");
        println!("  x 1400..2000 -> C red gradient (layered, per-pixel alpha)");

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}
