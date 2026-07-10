//! Spike: verify that a WS_EX_LAYERED child of WorkerW composites per-pixel alpha
//! over the desktop wallpaper. See spec section 8 for the success criteria.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{copy_nonoverlapping, null_mut};

use windows::core::{w, Result, BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, FindWindowExW, FindWindowW,
    GetMessageW, RegisterClassW, SendMessageTimeoutW, SetWindowPos, TranslateMessage,
    UpdateLayeredWindow, HWND_TOP, MSG, SMTO_NORMAL, SWP_NOACTIVATE, SWP_NOZORDER, ULW_ALPHA,
    WNDCLASSW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT, WS_VISIBLE,
};

const W: i32 = 600;
const H: i32 = 300;

/// Trampoline with the "system" ABI required for `WNDCLASSW::lpfnWndProc`.
/// `DefWindowProcW` in this `windows` version is a plain Rust-ABI wrapper, so it
/// cannot be assigned directly; this just forwards every message to it unchanged.
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn main() -> Result<()> {
    unsafe {
        let workerw = acquire_workerw()?;
        println!("WorkerW = {:?}", workerw);

        let hinst = GetModuleHandleW(None)?;
        let class = w!("SpikeLayeredChild");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            class,
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            W,
            H,
            Some(workerw),
            None,
            Some(hinst.into()),
            None,
        )?;

        // Child coordinates are relative to the parent's client area.
        let (sx, sy) = screen_origin_from_args();
        println!("screen origin = ({sx}, {sy})");
        let mut origin = POINT { x: sx, y: sy };
        windows::Win32::Graphics::Gdi::ScreenToClient(workerw, &mut origin).ok()?;
        println!("client origin = ({}, {})", origin.x, origin.y);
        SetWindowPos(hwnd, Some(HWND_TOP), origin.x, origin.y, W, H, SWP_NOACTIVATE | SWP_NOZORDER)?;

        push_gradient(hwnd)?;

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}

/// Virtual-desktop screen coordinates for the rectangle's top-left corner.
/// Defaults to (100, 100) on the primary monitor; pass `x y` to target another one,
/// including monitors at negative coordinates.
fn screen_origin_from_args() -> (i32, i32) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [x, y] => match (x.parse(), y.parse()) {
            (Ok(x), Ok(y)) => (x, y),
            _ => {
                eprintln!("usage: spike_layered [X Y]   (integers, virtual-desktop coordinates)");
                std::process::exit(2);
            }
        },
        [] => (100, 100),
        _ => {
            eprintln!("usage: spike_layered [X Y]   (integers, virtual-desktop coordinates)");
            std::process::exit(2);
        }
    }
}

/// Premultiplied BGRA: alpha ramps 0..255 left to right, colour is red.
fn gradient_pixels() -> Vec<u8> {
    let mut px = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let a = (x * 255 / (W - 1)) as u8;
            let i = ((y * W + x) * 4) as usize;
            px[i] = 0; // B
            px[i + 1] = 0; // G
            px[i + 2] = a; // R, premultiplied by alpha
            px[i + 3] = a; // A
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
            biHeight: -H, // top-down
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

    UpdateLayeredWindow(
        hwnd,
        Some(hdc_screen),
        None, // position already set by SetWindowPos
        Some(&size),
        Some(hdc_mem),
        Some(&src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    )?;

    SelectObject(hdc_mem, old);
    let _ = DeleteObject(HGDIOBJ(hbmp.0));
    let _ = DeleteDC(hdc_mem);
    ReleaseDC(None, hdc_screen);
    Ok(())
}

unsafe fn acquire_workerw() -> Result<HWND> {
    let progman = FindWindowW(w!("Progman"), None)?;
    let mut res = 0usize;

    SendMessageTimeoutW(progman, 0x052C, WPARAM(0), LPARAM(0), SMTO_NORMAL, 1000, Some(&mut res));
    if let Some(h) = find_workerw() {
        return Ok(h);
    }
    // Some Windows builds only spawn the WorkerW for this payload.
    SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(0x1), SMTO_NORMAL, 1000, Some(&mut res));
    find_workerw().ok_or_else(windows::core::Error::from_thread)
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // The WorkerW we want is the sibling that follows the window owning SHELLDLL_DefView.
    if FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None).is_ok() {
        if let Ok(worker) = FindWindowExW(None, Some(hwnd), w!("WorkerW"), None) {
            *(lparam.0 as *mut HWND) = worker;
            return BOOL(0); // stop enumeration
        }
    }
    BOOL(1)
}

unsafe fn find_workerw() -> Option<HWND> {
    let mut out = HWND(null_mut());
    let _ = EnumWindows(Some(enum_cb), LPARAM(&mut out as *mut HWND as isize));
    if out.0.is_null() {
        None
    } else {
        Some(out)
    }
}
