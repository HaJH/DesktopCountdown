//! Throwaway diagnostic v2. Fixes two flaws in v1:
//!   * each window carries its own paint colour in GWL_USERDATA, instead of a
//!     `static mut` that every WM_PAINT read after it had already changed;
//!   * C now differs from B only in the alpha mechanism (no stray WS_EX_TRANSPARENT).
//!
//! Four windows, on the primary monitor:
//!
//!   A (100,100)  child of WorkerW, plain,   WM_PAINT solid RED
//!   B (750,100)  child of WorkerW, layered, WM_PAINT solid GREEN + SetLayeredWindowAttributes
//!   C (1400,100) child of WorkerW, layered, UpdateLayeredWindow  RED gradient
//!   D (100,500)  TOP-LEVEL,        layered, UpdateLayeredWindow  BLUE gradient  <- control
//!
//! D answers "does UpdateLayeredWindow work at all in this process?" independent of WorkerW.
//! Not part of the product.

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
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowExW, FindWindowW,
    GetWindowLongPtrW, GetMessageW, RegisterClassW, SendMessageTimeoutW, SetLayeredWindowAttributes,
    SetParent, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow,
    GWLP_USERDATA, GWL_STYLE, HWND_TOP, LWA_ALPHA, MSG, SMTO_NORMAL, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SW_SHOW, ULW_ALPHA, WINDOW_EX_STYLE, WM_PAINT, WNDCLASSW, WS_CHILD,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
};

const W: i32 = 600;
const H: i32 = 300;

/// COLORREF is 0x00BBGGRR.
const RED: isize = 0x0000_00FF;
const GREEN: isize = 0x0000_FF00;

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        let colour = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let rect = RECT { left: 0, top: 0, right: W, bottom: H };
        let brush: HBRUSH = CreateSolidBrush(COLORREF(colour as u32));
        FillRect(hdc, &rect, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
        let _ = EndPaint(hwnd, &ps);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

unsafe fn create(ex: WINDOW_EX_STYLE, colour: isize) -> Result<HWND> {
    let hinst = GetModuleHandleW(None)?;
    let hwnd = CreateWindowExW(
        ex,
        w!("DiagPaint2"),
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
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, colour);
    Ok(hwnd)
}

/// Reparents into `workerw` and positions at screen (`sx`, `sy`).
unsafe fn adopt(workerw: HWND, hwnd: HWND, sx: i32, sy: i32) -> Result<()> {
    SetParent(hwnd, Some(workerw))?;
    SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_CHILD.0 | WS_VISIBLE.0) as isize);
    let mut o = POINT { x: sx, y: sy };
    ScreenToClient(workerw, &mut o).ok()?;
    SetWindowPos(hwnd, Some(HWND_TOP), o.x, o.y, W, H, SWP_NOACTIVATE | SWP_FRAMECHANGED)?;
    let _ = ShowWindow(hwnd, SW_SHOW);
    Ok(())
}

fn gradient_pixels(blue: bool) -> Vec<u8> {
    let mut px = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let a = (x * 255 / (W - 1)) as u8;
            let i = ((y * W + x) * 4) as usize;
            if blue {
                px[i] = a; // B, premultiplied
            } else {
                px[i + 2] = a; // R, premultiplied
            }
            px[i + 3] = a;
        }
    }
    px
}

unsafe fn push_gradient(hwnd: HWND, blue: bool, label: &str) -> Result<()> {
    let pixels = gradient_pixels(blue);
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
        hwnd, Some(hdc_screen), None, Some(&size), Some(hdc_mem), Some(&src),
        COLORREF(0), Some(&blend), ULW_ALPHA,
    );
    println!("  {label}: UpdateLayeredWindow -> {r:?}");

    SelectObject(hdc_mem, old);
    let _ = DeleteObject(HGDIOBJ(hbmp.0));
    let _ = DeleteDC(hdc_mem);
    ReleaseDC(None, hdc_screen);
    Ok(())
}

unsafe fn acquire_workerw() -> Result<HWND> {
    let progman = FindWindowW(w!("Progman"), None)?;
    if let Ok(h) = FindWindowExW(Some(progman), None, w!("WorkerW"), None) {
        return Ok(h);
    }
    let mut res = 0usize;
    SendMessageTimeoutW(progman, 0x052C, WPARAM(0), LPARAM(0), SMTO_NORMAL, 1000, Some(&mut res));
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
            lpszClassName: w!("DiagPaint2"),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let base = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;

        // A: plain child, solid red.
        let a = create(base, RED)?;
        adopt(workerw, a, 100, 100)?;
        println!("  A child plain      {:?} at (100,100)  expect SOLID RED", a.0);

        // B: layered child, solid green, constant alpha.
        let b = create(base | WS_EX_LAYERED, GREEN)?;
        adopt(workerw, b, 750, 100)?;
        let r = SetLayeredWindowAttributes(b, COLORREF(0), 128, LWA_ALPHA);
        println!("  B child layered+LWA {:?} at (750,100)  expect TRANSLUCENT GREEN, SLWA -> {r:?}", b.0);

        // C: layered child, per-pixel alpha. Differs from B only in the alpha mechanism.
        let c = create(base | WS_EX_LAYERED, 0)?;
        adopt(workerw, c, 1400, 100)?;
        push_gradient(c, false, "C child layered+ULW")?;
        println!("  C {:?} at (1400,100) expect RED GRADIENT", c.0);

        // D: TOP-LEVEL layered, per-pixel alpha. Control: is ULW working at all here?
        let d = create(base | WS_EX_LAYERED, 0)?;
        SetWindowPos(d, Some(HWND_TOP), 100, 500, W, H, SWP_NOACTIVATE)?;
        push_gradient(d, true, "D toplevel layered+ULW")?;
        let _ = ShowWindow(d, SW_SHOW);
        println!("  D {:?} at (100,500)  expect BLUE GRADIENT", d.0);

        println!("\nPrimary monitor:");
        println!("  y 100..400: x 100..700 A red | x 750..1350 B green | x 1400..2000 C red gradient");
        println!("  y 500..800: x 100..700 D blue gradient  (top-level control)");

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}
