//! Extracts glyph outlines from a text layout so they can be stroked.
//!
//! DirectWrite hands glyph runs to an `IDWriteTextRenderer` implementation; we turn each
//! run into an `ID2D1PathGeometry` translated to its baseline position.

use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;

use anyhow::Result;
use windows::core::{implement, Interface, Ref, BOOL};
use windows::Win32::Foundation::TRUE;
use windows::Win32::Graphics::Direct2D::{ID2D1Factory1, ID2D1Geometry};
use windows::Win32::Graphics::DirectWrite::*;
use windows_numerics::Matrix3x2;

/// The collected geometries live behind an `Rc` so `collect_geometry` can read them
/// after `Draw` returns, without reaching inside the COM object.
type Collected = Rc<RefCell<Vec<ID2D1Geometry>>>;

#[implement(IDWriteTextRenderer)]
struct OutlineCollector {
    d2d: ID2D1Factory1,
    geoms: Collected,
}

#[allow(non_snake_case)]
impl IDWritePixelSnapping_Impl for OutlineCollector_Impl {
    fn IsPixelSnappingDisabled(&self, _ctx: *const c_void) -> windows::core::Result<BOOL> {
        Ok(TRUE)
    }
    fn GetCurrentTransform(
        &self,
        _ctx: *const c_void,
        transform: *mut DWRITE_MATRIX,
    ) -> windows::core::Result<()> {
        // DrawGlyphRun below can panic-free-ly assume this pointer is valid: it is an
        // out-parameter DirectWrite allocates on its own stack before calling us.
        unsafe {
            *transform = DWRITE_MATRIX { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 };
        }
        Ok(())
    }
    fn GetPixelsPerDip(&self, _ctx: *const c_void) -> windows::core::Result<f32> {
        Ok(1.0)
    }
}

#[allow(non_snake_case)]
impl IDWriteTextRenderer_Impl for OutlineCollector_Impl {
    fn DrawGlyphRun(
        &self,
        _ctx: *const c_void,
        baseline_x: f32,
        baseline_y: f32,
        _mode: DWRITE_MEASURING_MODE,
        glyph_run: *const DWRITE_GLYPH_RUN,
        _desc: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _effect: Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        // Nothing here may panic: this function is called by DirectWrite through an
        // extern "system" vtable thunk, and unwinding across that boundary is UB.
        // `glyph_run` is a valid pointer for the duration of the call (DirectWrite's
        // contract for IDWriteTextRenderer::DrawGlyphRun), so the deref is sound;
        // everything past it uses `?` instead of `unwrap`/indexing panics.
        unsafe {
            let run = &*glyph_run;
            let Some(face) = run.fontFace.as_ref() else { return Ok(()) };

            let path = self.d2d.CreatePathGeometry()?;
            let sink = path.Open()?;
            face.GetGlyphRunOutline(
                run.fontEmSize,
                run.glyphIndices,
                Some(run.glyphAdvances),
                Some(run.glyphOffsets),
                run.glyphCount,
                run.isSideways.as_bool(),
                run.bidiLevel % 2 == 1,
                &sink,
            )?;
            sink.Close()?;

            // GetGlyphRunOutline emits coordinates relative to the baseline origin.
            let translate = Matrix3x2 {
                M11: 1.0,
                M12: 0.0,
                M21: 0.0,
                M22: 1.0,
                M31: baseline_x,
                M32: baseline_y,
            };
            let moved = self.d2d.CreateTransformedGeometry(&path, &translate)?;
            // `borrow_mut` cannot be reentered here: nothing else holds the RefCell
            // borrow while DirectWrite is synchronously calling into this method.
            self.geoms.borrow_mut().push(moved.cast()?);
        }
        Ok(())
    }

    fn DrawUnderline(
        &self,
        _ctx: *const c_void,
        _x: f32,
        _y: f32,
        _underline: *const DWRITE_UNDERLINE,
        _effect: Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn DrawStrikethrough(
        &self,
        _ctx: *const c_void,
        _x: f32,
        _y: f32,
        _strikethrough: *const DWRITE_STRIKETHROUGH,
        _effect: Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn DrawInlineObject(
        &self,
        _ctx: *const c_void,
        _x: f32,
        _y: f32,
        _obj: Ref<IDWriteInlineObject>,
        _sideways: BOOL,
        _right_to_left: BOOL,
        _effect: Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

/// One geometry per glyph run in `layout`, positioned as if the layout were drawn
/// at (`origin_x`, `origin_y`).
pub(crate) fn collect_geometry(
    d2d: &ID2D1Factory1,
    layout: &IDWriteTextLayout,
    origin_x: f32,
    origin_y: f32,
) -> Result<Vec<ID2D1Geometry>> {
    let geoms: Collected = Rc::new(RefCell::new(Vec::new()));
    let collector = OutlineCollector { d2d: d2d.clone(), geoms: Rc::clone(&geoms) };
    let renderer: IDWriteTextRenderer = collector.into();

    unsafe { layout.Draw(None, &renderer, origin_x, origin_y)? };
    drop(renderer); // release the COM object's Rc handle before reading geoms

    let out = geoms.borrow().clone();
    Ok(out)
}
