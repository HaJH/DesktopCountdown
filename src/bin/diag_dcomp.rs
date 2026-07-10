//! Throwaway diagnostic: the two remaining ways to get pixels onto the wallpaper layer,
//! now that UpdateLayeredWindow is known not to composite for child windows.
//!
//!   E (100,100) layered child + SetLayeredWindowAttributes(alpha 255)
//!               WM_PAINT draws BLUE left half / YELLOW right half.
//!               If this shows and survives, spec section 9 is viable: we own every pixel,
//!               so we must draw the wallpaper ourselves under the text.
//!
//!   F (750,100) plain child + DirectComposition visual, per-pixel alpha via D2D.
//!               A magenta disc on a transparent background, plus a 50%-alpha bar.
//!               If this shows with the wallpaper visible through the transparent parts,
//!               we get antialiased glyphs for free and never touch the wallpaper.
//!
//! Not part of the product.

use windows::core::{w, Interface, Result, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionSurface, IDCompositionTarget,
    IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, ScreenToClient, HBRUSH, HGDIOBJ,
    PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_numerics::Vector2;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowExW, FindWindowW, GetMessageW,
    RegisterClassW, SetLayeredWindowAttributes, SetParent, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, GWL_STYLE, HWND_TOP, LWA_ALPHA, MSG, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SW_SHOW, WINDOW_EX_STYLE, WM_PAINT, WNDCLASSW, WS_CHILD, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
};

const W: i32 = 600;
const H: i32 = 300;

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        // COLORREF is 0x00BBGGRR.
        let left = CreateSolidBrush(COLORREF(0x00FF_0000)); // blue
        let right = CreateSolidBrush(COLORREF(0x0000_FFFF)); // yellow
        FillRect(hdc, &RECT { left: 0, top: 0, right: W / 2, bottom: H }, left);
        FillRect(hdc, &RECT { left: W / 2, top: 0, right: W, bottom: H }, right);
        let _ = DeleteObject(HGDIOBJ(left.0));
        let _ = DeleteObject(HGDIOBJ(right.0));
        let _ = EndPaint(hwnd, &ps);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

unsafe fn make(ex: WINDOW_EX_STYLE) -> Result<HWND> {
    let hinst = GetModuleHandleW(None)?;
    CreateWindowExW(
        ex, w!("DiagDComp"), PCWSTR::null(), WS_POPUP,
        0, 0, W, H, None, None, Some(hinst.into()), None,
    )
}

unsafe fn adopt(workerw: HWND, hwnd: HWND, sx: i32, sy: i32) -> Result<()> {
    SetParent(hwnd, Some(workerw))?;
    SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_CHILD.0 | WS_VISIBLE.0) as isize);
    let mut o = POINT { x: sx, y: sy };
    ScreenToClient(workerw, &mut o).ok()?;
    SetWindowPos(hwnd, Some(HWND_TOP), o.x, o.y, W, H, SWP_NOACTIVATE | SWP_FRAMECHANGED)?;
    let _ = ShowWindow(hwnd, SW_SHOW);
    Ok(())
}

unsafe fn acquire_workerw() -> Result<HWND> {
    let progman = FindWindowW(w!("Progman"), None)?;
    FindWindowExW(Some(progman), None, w!("WorkerW"), None)
}

/// Builds a DirectComposition visual tree on `hwnd` and paints per-pixel alpha content.
/// Returns the objects so they stay alive for the process lifetime.
unsafe fn setup_dcomp(hwnd: HWND) -> Result<(IDCompositionDevice, IDCompositionTarget, IDCompositionVisual, IDCompositionSurface)> {
    let mut d3d: Option<ID3D11Device> = None;
    D3D11CreateDevice(
        None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(),
        D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION,
        Some(&mut d3d), None, None,
    )?;
    let dxgi: IDXGIDevice = d3d.expect("d3d device").cast()?;
    println!("  F: D3D11 + DXGI device ok");

    let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
    println!("  F: DCompositionCreateDevice ok");

    // The question this whole binary exists to answer.
    let target: IDCompositionTarget = dcomp.CreateTargetForHwnd(hwnd, true)?;
    println!("  F: CreateTargetForHwnd(child) ok  <=== the gate");

    let visual = dcomp.CreateVisual()?;
    let surface = dcomp.CreateSurface(
        W as u32, H as u32,
        DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_PREMULTIPLIED,
    )?;

    let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
    let d2d_device: ID2D1Device = factory.CreateDevice(&dxgi)?;
    let dc: ID2D1DeviceContext = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

    let mut offset = POINT::default();
    let dxgi_surface: IDXGISurface = surface.BeginDraw(None, &mut offset)?;

    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
        ..Default::default()
    };
    let bitmap = dc.CreateBitmapFromDxgiSurface(&dxgi_surface, Some(&props))?;
    dc.SetTarget(&bitmap);
    dc.BeginDraw();
    dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));

    let ox = offset.x as f32;
    let oy = offset.y as f32;

    // Opaque magenta disc: if the wallpaper shows around it, per-pixel alpha works.
    let disc = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 0.0, b: 1.0, a: 1.0 }, None)?;
    dc.FillEllipse(
        &D2D1_ELLIPSE {
            point: Vector2 { X: ox + 150.0, Y: oy + 150.0 },
            radiusX: 120.0,
            radiusY: 120.0,
        },
        &disc,
    );

    // Half-transparent white bar: tests intermediate alpha, not just on/off.
    let bar = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.5 }, None)?;
    dc.FillRectangle(
        &D2D_RECT_F { left: ox + 330.0, top: oy + 100.0, right: ox + 570.0, bottom: oy + 200.0 },
        &bar,
    );

    dc.EndDraw(None, None)?;
    surface.EndDraw()?;

    visual.SetContent(&surface)?;
    target.SetRoot(&visual)?;
    dcomp.Commit()?;
    println!("  F: visual committed");

    Ok((dcomp, target, visual, surface))
}

fn main() -> Result<()> {
    unsafe {
        let workerw = acquire_workerw()?;
        println!("WorkerW = {:?}\n", workerw.0);

        let hinst = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: w!("DiagDComp"),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let base = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;

        println!("E: layered child + LWA(alpha 255), GDI paint, at (100,100)");
        let e = make(base | WS_EX_LAYERED)?;
        adopt(workerw, e, 100, 100)?;
        let r = SetLayeredWindowAttributes(e, COLORREF(0), 255, LWA_ALPHA);
        println!("  E: SetLayeredWindowAttributes(255) -> {r:?}  expect BLUE|YELLOW halves\n");

        println!("F: plain child + DirectComposition, at (750,100)");
        let f = make(base)?;
        adopt(workerw, f, 750, 100)?;
        let _keep = match setup_dcomp(f) {
            Ok(objs) => {
                println!("  F: expect MAGENTA DISC + 50% WHITE BAR, wallpaper visible around them\n");
                Some(objs)
            }
            Err(err) => {
                println!("  F: FAILED -> {err:?}");
                println!("  F: DirectComposition is not usable on a child of WorkerW.\n");
                None
            }
        };

        println!("Primary monitor, y=100..400:");
        println!("  x 100..700   E  blue/yellow halves   (opaque layered child)");
        println!("  x 750..1350  F  magenta disc + bar   (DirectComposition per-pixel alpha)");

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}
