//! DirectComposition surfaces for the wallpaper-layer child windows.
//!
//! This is the only mechanism that composites per-pixel alpha for a child window.
//! `UpdateLayeredWindow` returns Ok and draws nothing there; a plain child window's
//! pixels are overpainted when Explorer redraws the wallpaper. See spec section 3.3.

use anyhow::Result;
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND, POINT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Device, ID2D1DeviceContext, ID2D1Factory1, ID2D1RenderTarget,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionSurface, IDCompositionTarget,
    IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface};

pub struct Compositor {
    dcomp: IDCompositionDevice,
    d2d_device: ID2D1Device,
    /// Held so the DXGI/DComp/D2D devices derived from it stay valid.
    _d3d: ID3D11Device,
}

/// One composition target per window.
pub struct Surface {
    _target: IDCompositionTarget,
    visual: IDCompositionVisual,
    surface: Option<IDCompositionSurface>,
    size: (u32, u32),
}

impl Surface {
    /// Only the tests read this back: `draw` recreates the surface itself when the size
    /// changes, so no caller has a reason to ask.
    #[cfg(test)]
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
}

/// Creates a D3D11 device with BGRA support (required for Direct2D interop).
///
/// `D3D_DRIVER_TYPE_HARDWARE` can fail with `DXGI_ERROR_UNSUPPORTED` when there is no
/// usable GPU adapter, which happens in some remote-desktop sessions. Retry with the
/// software `D3D_DRIVER_TYPE_WARP` rasterizer in that case: slower, but it keeps the
/// app working instead of turning a missing GPU into a hard failure.
fn create_d3d11_device() -> Result<ID3D11Device> {
    unsafe {
        for (driver, name) in [
            (D3D_DRIVER_TYPE_HARDWARE, "HARDWARE"),
            (D3D_DRIVER_TYPE_WARP, "WARP"),
        ] {
            let mut d3d: Option<ID3D11Device> = None;
            let result = D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            );
            match (result, d3d) {
                (Ok(()), Some(d3d)) => {
                    tracing::info!(driver = name, "D3D11 device created");
                    return Ok(d3d);
                }
                (Err(err), _) if driver == D3D_DRIVER_TYPE_HARDWARE => {
                    tracing::warn!(?err, "D3D_DRIVER_TYPE_HARDWARE failed, retrying with WARP");
                }
                (Err(err), _) => return Err(err.into()),
                (Ok(()), None) => unreachable!("D3D11CreateDevice succeeded without a device"),
            }
        }
        unreachable!("loop returns or errors on its last iteration")
    }
}

/// Releases the surface lock taken by `IDCompositionSurface::BeginDraw`, even on unwind.
/// Skipping it leaves the surface locked and every later frame fails with
/// DCOMPOSITION_ERROR_SURFACE_BEING_RENDERED.
struct SurfaceLock<'a> {
    surface: &'a IDCompositionSurface,
    armed: bool,
}

impl SurfaceLock<'_> {
    /// Ends the draw and returns its result, disarming the guard.
    fn end(mut self) -> windows::core::Result<()> {
        self.armed = false;
        unsafe { self.surface.EndDraw() }
    }
}

impl Drop for SurfaceLock<'_> {
    fn drop(&mut self) {
        if self.armed {
            unsafe {
                let _ = self.surface.EndDraw();
            }
        }
    }
}

/// Ends the `ID2D1DeviceContext` draw started by `BeginDraw`, even on unwind. Skipping
/// `EndDraw` loses Direct2D's coalesced per-frame error and leaves this frame's drawing
/// commands unflushed to the surface bitmap they targeted.
struct DeviceContextDraw<'a> {
    dc: &'a ID2D1DeviceContext,
    armed: bool,
}

impl DeviceContextDraw<'_> {
    /// Ends the draw and returns its result, disarming the guard.
    fn end(mut self) -> windows::core::Result<()> {
        self.armed = false;
        unsafe { self.dc.EndDraw(None, None) }
    }
}

impl Drop for DeviceContextDraw<'_> {
    fn drop(&mut self) {
        if self.armed {
            unsafe {
                let _ = self.dc.EndDraw(None, None);
            }
        }
    }
}

impl Compositor {
    pub fn new(d2d_factory: &ID2D1Factory1) -> Result<Self> {
        unsafe {
            let d3d = create_d3d11_device()?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let d2d_device: ID2D1Device = d2d_factory.CreateDevice(&dxgi)?;
            Ok(Self {
                dcomp,
                d2d_device,
                _d3d: d3d,
            })
        }
    }

    pub fn attach(&self, hwnd: HWND) -> Result<Surface> {
        unsafe {
            let target = self.dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = self.dcomp.CreateVisual()?;
            target.SetRoot(&visual)?;
            Ok(Surface {
                _target: target,
                visual,
                surface: None,
                size: (0, 0),
            })
        }
    }

    /// Draws into `s`, recreating its surface when the size changed.
    /// `f` receives the render target and the surface offset to add to every coordinate.
    pub fn draw<F>(&self, s: &mut Surface, w: u32, h: u32, f: F) -> Result<()>
    where
        F: FnOnce(&ID2D1RenderTarget, (f32, f32)) -> Result<()>,
    {
        // CreateSurface rejects a zero dimension.
        let w = w.max(1);
        let h = h.max(1);
        unsafe {
            if s.surface.is_none() || s.size != (w, h) {
                let surface = self.dcomp.CreateSurface(
                    w,
                    h,
                    DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_ALPHA_MODE_PREMULTIPLIED,
                )?;
                s.visual.SetContent(&surface)?;
                s.surface = Some(surface);
                s.size = (w, h);
            }
            let surface = s.surface.as_ref().expect("surface just created");

            let mut offset = POINT::default();
            let dxgi_surface: IDXGISurface = surface.BeginDraw(None, &mut offset)?;

            // The surface is locked from here. `SurfaceLock::drop` runs `surface.EndDraw()`
            // unconditionally -- on an early `?` return below *and* on a panic unwinding
            // through `f` -- so the surface is never left locked. See `SurfaceLock`.
            let surface_lock = SurfaceLock {
                surface,
                armed: true,
            };

            let result = (|| -> Result<()> {
                let dc: ID2D1DeviceContext = self
                    .d2d_device
                    .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
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
                // Obtained before `dc.BeginDraw()` so a failing cast cannot leave the
                // device context mid-draw.
                let rt: ID2D1RenderTarget = dc.cast()?;

                dc.BeginDraw();
                // Guards, not `catch_unwind`: `f` will run inside a window procedure
                // (Task 13), and unwinding across that `extern "system"` boundary is
                // undefined behaviour. If `f` panics here, unwinding drops `dc_lock`
                // (running `dc.EndDraw()`) and then `surface_lock` (running
                // `surface.EndDraw()`) on the way out, so the surface is released instead
                // of staying locked forever.
                let dc_lock = DeviceContextDraw {
                    dc: &dc,
                    armed: true,
                };
                let drawn = f(&rt, (offset.x as f32, offset.y as f32));
                let ended = dc_lock.end();
                drawn?;
                ended?;
                Ok(())
            })();

            surface_lock.end()?;
            result?;
            // Commit is skipped when the frame failed: leaving the last good frame on
            // screen beats publishing a half-drawn one.
            self.dcomp.Commit()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::render::Painter;
    use super::*;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, RegisterClassW, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
    };

    unsafe extern "system" fn test_wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        DefWindowProcW(h, m, w, l)
    }

    fn hidden_window() -> HWND {
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let wc = WNDCLASSW {
                lpfnWndProc: Some(test_wndproc),
                hInstance: hinst.into(),
                lpszClassName: w!("DCompTestWindow"),
                ..Default::default()
            };
            RegisterClassW(&wc);
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("DCompTestWindow"),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                64,
                64,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .unwrap()
        }
    }

    #[test]
    fn creates_a_device() {
        let p = Painter::new().unwrap();
        assert!(Compositor::new(p.d2d_factory()).is_ok());
    }

    #[test]
    fn attaches_a_target_to_a_window_and_draws() {
        let p = Painter::new().unwrap();
        let c = Compositor::new(p.d2d_factory()).unwrap();
        let mut s = c.attach(hidden_window()).unwrap();

        c.draw(&mut s, 64, 64, |rt, _offset| {
            unsafe {
                rt.Clear(Some(&D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                }))
            };
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn resizing_replaces_the_surface() {
        let p = Painter::new().unwrap();
        let c = Compositor::new(p.d2d_factory()).unwrap();
        let mut s = c.attach(hidden_window()).unwrap();

        c.draw(&mut s, 32, 32, |_, _| Ok(())).unwrap();
        assert_eq!(s.size(), (32, 32));
        c.draw(&mut s, 100, 50, |_, _| Ok(())).unwrap();
        assert_eq!(s.size(), (100, 50));
    }

    #[test]
    fn a_failing_callback_propagates() {
        let p = Painter::new().unwrap();
        let c = Compositor::new(p.d2d_factory()).unwrap();
        let mut s = c.attach(hidden_window()).unwrap();
        let r = c.draw(&mut s, 16, 16, |_, _| Err(anyhow::anyhow!("boom")));
        assert!(r.is_err());
    }

    /// `a_failing_callback_propagates` only proves the error surfaces. It does not prove
    /// the surface's `BeginDraw` lock was released. If `EndDraw` were skipped on the
    /// error path, every later `draw` call on the same `Surface` would fail too.
    #[test]
    fn surface_still_usable_after_a_failing_callback() {
        let p = Painter::new().unwrap();
        let c = Compositor::new(p.d2d_factory()).unwrap();
        let mut s = c.attach(hidden_window()).unwrap();

        let r = c.draw(&mut s, 16, 16, |_, _| Err(anyhow::anyhow!("boom")));
        assert!(r.is_err());

        let r = c.draw(&mut s, 16, 16, |_, _| Ok(()));
        assert!(
            r.is_ok(),
            "surface stayed locked after a failing callback: {r:?}"
        );
    }

    /// Companion to `surface_still_usable_after_a_failing_callback`, but for an unwinding
    /// callback instead of one that merely returns `Err`. Without the `SurfaceLock` /
    /// `DeviceContextDraw` guards, a panic inside `f` skips both `EndDraw` calls and the
    /// surface is left locked forever.
    ///
    /// The process-wide panic hook is swapped for a silent one only for the duration of
    /// this test's `catch_unwind` call and restored immediately after -- see the task
    /// report for why that is safe under `cargo test`'s default parallel harness.
    #[test]
    fn surface_survives_a_panicking_callback() {
        std::panic::set_hook(Box::new(|_| {}));

        let p = Painter::new().unwrap();
        let c = Compositor::new(p.d2d_factory()).unwrap();
        let mut s = c.attach(hidden_window()).unwrap();

        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.draw(&mut s, 16, 16, |_, _| panic!("boom"))
        }));

        let _ = std::panic::take_hook();

        assert!(caught.is_err(), "the panicking callback did not panic");

        let r = c.draw(&mut s, 16, 16, |_, _| Ok(()));
        assert!(
            r.is_ok(),
            "surface stayed locked after a panicking callback: {r:?}"
        );
    }
}
