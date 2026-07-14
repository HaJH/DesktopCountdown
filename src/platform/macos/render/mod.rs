//! Draws the countdown into a CoreGraphics bitmap. Knows nothing about `NSWindow`,
//! `CALayer`, or where the pixels end up. The macOS counterpart of the Windows backend's
//! `render/`.
//!
//! # Coordinates
//!
//! CoreGraphics is y-**up**: the origin is the bottom-left corner and a glyph's outline
//! comes out of CoreText measured from its baseline, upward. The Windows backend is
//! y-down and places a line by the top-left of its layout box. Rather than flip the
//! context -- which would flip the glyphs with it, and mean fighting the text matrix
//! back -- the stacking below is worked out in y-down, exactly as on Windows, and turned
//! into a y-up baseline at the last moment (`h - (ink_top + ink_above)`).

mod text;

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use anyhow::{anyhow, Result};
use objc2_core_foundation::{
    CFRange, CFRetained, CFString, CGAffineTransform, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapContextCreate, CGBitmapContextGetBytesPerRow,
    CGBitmapContextGetData, CGColor, CGColorSpace, CGContext, CGGlyph, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGMutablePath, CGPath, CGPathDrawingMode,
};
use objc2_core_text::{kCTFontAttributeName, CTFont, CTLine, CTRun};

use crate::color::parse_hex;
use crate::config::{Align, DrawMode, Line, Style};
use text::TextEngine;

/// Gap between one line's ink and the next line's ink, as a fraction of `size_px`. Ink, not
/// line box: see `ink_span`. A fraction of the base size, not of either neighbour's size, so
/// it is the same between every pair however the individual lines are scaled.
const LINE_GAP_RATIO: f64 = 0.12;
/// Shadow offset in pixels. No blur; see spec section 3.2.
const SHADOW_OFFSET: f64 = 2.0;
const SHADOW_ALPHA: f64 = 0.55;

pub struct Painter {
    text: TextEngine,
}

/// One line, laid out and ready to draw: its `CTLine`, the advance width used to align
/// it, where its ink sits relative to its baseline (see `ink_span`), and the two per-line
/// settings that survive from the config.
struct Laid {
    line: CFRetained<CTLine>,
    width: f64,
    /// Height of the inked pixels themselves.
    ink_h: f64,
    /// Distance from the baseline **up** to the top of the ink.
    ink_above: f64,
    align: Align,
    color: Option<String>,
}

/// A laid-out stack of lines and the canvas they need. Built once per redraw by `compose`;
/// `size` sizes the bitmap and `render` draws it. Held by the caller between the two so
/// the (not free -- see `ink_span`) layout work happens once, not twice.
pub struct Composed {
    lines: Vec<Laid>,
    pad: f64,
    gap: f64,
    content_w: f64,
    width: u32,
    height: u32,
}

impl Composed {
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Where a line's ink actually is, relative to its baseline: `(height, distance from the
/// baseline up to the ink's top edge)`.
///
/// `CTLineGetTypographicBounds` gives the *line box* -- ascent + descent + leading -- which
/// is a property of the font, not of the string. A CJK font sizes its ascent to hold Hangul
/// and Han while digits only reach the cap height, so a line of digits floats with a large
/// band of nothing above it. Stacking two such boxes puts several times `LINE_GAP_RATIO` of
/// dead space between the two lines' glyphs. Measuring the ink instead makes the constant
/// mean what it says, in any font. This is the same trick, and the same reason, as the
/// Windows backend's `ink_span`.
///
/// `CGPathGetPathBoundingBox` -- not `CGPathGetBoundingBox`, which bounds the *control
/// points* and so overshoots a curve's true extent -- is the exact counterpart of the
/// Direct2D geometry bounds used there. The path is built anyway for `DrawMode::Outline`,
/// so it costs nothing extra.
///
/// A string with no ink at all (all spaces) has no outlines and falls back to the line box.
fn ink_span(line: &CTLine, ascent: f64, box_h: f64) -> Result<(f64, f64)> {
    let path = glyph_path(line, 0.0, 0.0)?;
    let b = CGPath::path_bounding_box(Some(&path));
    // A path with no contours bounds to CGRectNull -- an infinite origin and a zero size --
    // so guard both the degenerate height and the non-finite origin it comes with.
    let height = b.size.height;
    if !height.is_finite() || height <= 0.0 || !b.origin.y.is_finite() {
        return Ok((box_h, ascent));
    }
    Ok((height, b.origin.y + height))
}

/// Every glyph outline in `line`, as one path, positioned as if the line's baseline origin
/// sat at (`x`, `y`) in the context's own y-up coordinates.
fn glyph_path(line: &CTLine, x: f64, y: f64) -> Result<CFRetained<CGMutablePath>> {
    let path = CGMutablePath::new();

    // SAFETY: `CTLineGetGlyphRuns` returns a CFArray of CTRun owned by the line.
    let runs = unsafe { line.glyph_runs() };
    for i in 0..runs.count() {
        let run = unsafe { runs.value_at_index(i) } as *const CTRun;
        // SAFETY: the array holds CTRun, and the line keeps them alive for this loop.
        let Some(run) = (unsafe { run.as_ref() }) else {
            continue;
        };

        let n = unsafe { run.glyph_count() };
        if n <= 0 {
            continue;
        }

        // SAFETY: every run carries the attributes it was created from, and we set the
        // font on the whole string.
        let attrs = unsafe { run.attributes() };
        let font = unsafe {
            attrs.value(kCTFontAttributeName as *const CFString as *const c_void) as *const CTFont
        };
        let Some(font) = (unsafe { font.as_ref() }) else {
            tracing::warn!("a glyph run carries no font, skipping it");
            continue;
        };

        let (glyphs, positions) = run_contents(run, n as usize);
        for (glyph, pos) in glyphs.iter().zip(&positions) {
            // A glyph with no outline (a space) yields no path, and needs none.
            // SAFETY: `glyph` came out of this very run, so the font has it. A null matrix
            // is the identity.
            let Some(outline) = (unsafe { font.path_for_glyph(*glyph, ptr::null()) }) else {
                continue;
            };
            let at = translation(x + pos.x, y + pos.y);
            // SAFETY: `at` is a valid transform and `outline` a valid path.
            unsafe { CGMutablePath::add_path(Some(&path), &at, Some(&outline)) };
        }
    }
    Ok(path)
}

/// The run's glyphs and their positions.
///
/// Always the copying variants, never `CTRunGetGlyphsPtr`/`CTRunGetPositionsPtr`: those
/// are documented to return NULL whenever the run's internal storage is not already in the
/// requested format, and a caller that does not handle the NULL silently draws nothing.
/// One memcpy per line per second is not worth the trap.
fn run_contents(run: &CTRun, n: usize) -> (Vec<CGGlyph>, Vec<CGPoint>) {
    let mut glyphs: Vec<CGGlyph> = vec![0; n];
    let mut positions: Vec<CGPoint> = vec![CGPoint::new(0.0, 0.0); n];
    // A zero length means "the whole run", which is what we want and also what `n` is.
    let all = CFRange {
        location: 0,
        length: n as isize,
    };
    // SAFETY: both buffers hold exactly `n` elements, which is the run's glyph count, so
    // CoreText writes in bounds.
    unsafe {
        run.glyphs(all, NonNull::new_unchecked(glyphs.as_mut_ptr()));
        run.positions(all, NonNull::new_unchecked(positions.as_mut_ptr()));
    }
    (glyphs, positions)
}

fn translation(tx: f64, ty: f64) -> CGAffineTransform {
    CGAffineTransform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx,
        ty,
    }
}

fn cg_color(hex: &str, alpha: f64) -> Result<CFRetained<CGColor>> {
    let c = parse_hex(hex).ok_or_else(|| anyhow!("invalid colour {hex}"))?;
    // sRGB, to match the bitmap's own sRGB colour space: CoreGraphics converts between
    // colour spaces when they differ, and then "#FF0000" no longer lands on exactly
    // (255, 0, 0) in the buffer.
    Ok(CGColor::new_srgb(
        f64::from(c.r) / 255.0,
        f64::from(c.g) / 255.0,
        f64::from(c.b) / 255.0,
        alpha,
    ))
}

/// An offscreen premultiplied-BGRA bitmap.
///
/// `PremultipliedFirst | ByteOrder32Little` puts the bytes in memory as B, G, R, A -- byte
/// for byte the format the Windows backend composites (`GUID_WICPixelFormat32bppPBGRA`),
/// so the two renderers' pixel tests are the same tests.
///
/// Needs no `NSApplication`, no window server, no GPU: a bitmap context is a plain CPU
/// buffer, and CoreText lays out against font files. This whole module therefore runs on a
/// headless CI runner, exactly as the Direct2D-into-a-WIC-bitmap tests do on Windows.
pub struct Canvas {
    ctx: CFRetained<CGContext>,
    width: u32,
    height: u32,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        // CoreGraphics rejects a zero dimension.
        let width = width.max(1);
        let height = height.max(1);

        // SAFETY: reading an `extern "C"` constant.
        let name = unsafe { kCGColorSpaceSRGB };
        let space = CGColorSpace::with_name(Some(name))
            .ok_or_else(|| anyhow!("the sRGB colour space is unavailable"))?;

        let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
        // SAFETY: a null data pointer asks CoreGraphics to own the buffer, and a zero
        // bytes-per-row asks it to choose the row padding.
        let ctx = unsafe {
            CGBitmapContextCreate(
                ptr::null_mut(),
                width as usize,
                height as usize,
                8,
                0,
                Some(&space),
                info,
            )
        }
        .ok_or_else(|| anyhow!("could not create a {width}x{height} bitmap context"))?;

        Ok(Self { ctx, width, height })
    }

    pub fn context(&self) -> &CGContext {
        &self.ctx
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The pixels, tightly packed as BGRA. CoreGraphics pads its rows, so this copies row
    /// by row rather than handing the raw buffer back.
    pub fn pixels(&self) -> Vec<u8> {
        let stride = CGBitmapContextGetBytesPerRow(Some(&self.ctx));
        let data = CGBitmapContextGetData(Some(&self.ctx)) as *const u8;
        let row = self.width as usize * 4;
        let mut out = vec![0u8; row * self.height as usize];
        if data.is_null() {
            return out;
        }
        for y in 0..self.height as usize {
            // SAFETY: the context owns a buffer of `stride * height` bytes, and every read
            // below stays inside row `y`, which is `row <= stride` bytes wide.
            unsafe {
                ptr::copy_nonoverlapping(data.add(y * stride), out.as_mut_ptr().add(y * row), row);
            }
        }
        out
    }
}

impl Painter {
    pub fn new() -> Result<Self> {
        Ok(Self {
            text: TextEngine::new()?,
        })
    }

    pub fn measure(&self, lines: &[Line], style: &Style) -> Result<(u32, u32)> {
        Ok(self.compose(lines, style)?.size())
    }

    /// Lays every line out and sizes the canvas they need. Vertically the canvas holds the
    /// lines' *ink* plus one gap between each adjacent pair, not their line boxes -- see
    /// `ink_span`. A line whose text is empty is dropped: it draws nothing and takes no gap.
    pub fn compose(&self, lines: &[Line], style: &Style) -> Result<Composed> {
        let family = self.text.resolve_family(&style.font_family);

        let mut laid = Vec::with_capacity(lines.len());
        for l in lines {
            if l.text.is_empty() {
                continue;
            }
            laid.push(self.lay(l, &family, style)?);
        }

        let pad = (f64::from(style.outline_width_px.max(0.0))
            + 4.0
            + if style.shadow { SHADOW_OFFSET } else { 0.0 })
        .ceil();
        let gap = f64::from(style.size_px) * LINE_GAP_RATIO;

        let content_w = laid.iter().map(|l| l.width).fold(0.0f64, f64::max);
        let content_h =
            laid.iter().map(|l| l.ink_h).sum::<f64>() + gap * laid.len().saturating_sub(1) as f64;

        Ok(Composed {
            lines: laid,
            pad,
            gap,
            content_w,
            width: (content_w + pad * 2.0).ceil().max(1.0) as u32,
            height: (content_h + pad * 2.0).ceil().max(1.0) as u32,
        })
    }

    fn lay(&self, line: &Line, family: &str, style: &Style) -> Result<Laid> {
        // CoreText rejects a zero or negative em size. `config::validate` already keeps
        // size_ratio > 0, so this only guards a pathologically small product.
        let size_px = (style.size_px * line.size_ratio).max(1.0);
        let laid = self.text.layout(&line.text, family, style, size_px)?;
        let (ink_h, ink_above) = ink_span(&laid.line, laid.ascent, laid.box_h)?;
        Ok(Laid {
            line: laid.line,
            width: laid.width,
            ink_h,
            ink_above,
            align: line.align,
            color: line.color.clone(),
        })
    }

    /// Draws `c` into a fresh bitmap of `c.size() * scale`.
    ///
    /// `scale` is the display's `backingScaleFactor`: the layout above is in points, and
    /// the bitmap is in pixels, so the context is scaled and everything below keeps
    /// working in points.
    pub fn render(&self, c: &Composed, style: &Style, scale: f32) -> Result<Canvas> {
        let scale = if scale.is_finite() && scale > 0.0 {
            f64::from(scale)
        } else {
            1.0
        };
        let canvas = Canvas::new(
            (f64::from(c.width) * scale).ceil() as u32,
            (f64::from(c.height) * scale).ceil() as u32,
        )?;

        let ctx = canvas.context();
        CGContext::scale_ctm(Some(ctx), scale, scale);
        self.paint(ctx, c, style)?;
        Ok(canvas)
    }

    /// Draws onto `ctx`, in points, y-up. The caller owns the context's scale.
    fn paint(&self, ctx: &CGContext, c: &Composed, style: &Style) -> Result<()> {
        let alpha = f64::from(style.opacity.clamp(0.0, 1.0));
        let bounds = CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(f64::from(c.width), f64::from(c.height)),
        );

        CGContext::save_g_state(Some(ctx));
        // Never touch pixels outside our slot, and start from nothing: a `Canvas` may be
        // handed to us more than once.
        CGContext::clip_to_rect(Some(ctx), bounds);
        CGContext::clear_rect(Some(ctx), bounds);
        CGContext::set_should_antialias(Some(ctx), true);
        // Rounded joins keep a thick outline from growing spikes at sharp glyph corners,
        // which is what Direct2D's default stroke style does too.
        CGContext::set_line_join(Some(ctx), objc2_core_graphics::CGLineJoin::Round);

        let result = self.paint_inner(ctx, c, style, alpha);

        // Restore on every path, including an error above, so a failed paint never leaves
        // the context's state stack unbalanced.
        CGContext::restore_g_state(Some(ctx));
        result
    }

    /// The actual drawing, run inside the state saved by `paint`. Split out so `paint` can
    /// guarantee the state is restored on every return path.
    fn paint_inner(&self, ctx: &CGContext, c: &Composed, style: &Style, alpha: f64) -> Result<()> {
        let stroke = cg_color(&style.outline_color, alpha)?;
        let shadow = cg_color("#000000", SHADOW_ALPHA * alpha)?;
        let h = f64::from(c.height);

        // Where each line's baseline goes. The canvas was sized to the ink, so line i's ink
        // top sits `pad + sum(earlier ink + gap)` below the top edge; its baseline is
        // `ink_above` further down again. That is all y-down; the context is y-up, hence
        // the subtraction from `h`.
        let mut placed: Vec<(f64, f64, &Laid)> = Vec::with_capacity(c.lines.len());
        let mut ink_top = c.pad;
        for l in &c.lines {
            let x = c.pad
                + match l.align {
                    Align::Left => 0.0,
                    Align::Center => (c.content_w - l.width) / 2.0,
                    Align::Right => c.content_w - l.width,
                };
            placed.push((x, h - (ink_top + l.ink_above), l));
            ink_top += l.ink_h + c.gap;
        }

        if style.shadow {
            // Both fill and stroke draw in the shadow colour here, otherwise an outline-mode
            // glyph would show a filled shadow behind a hollow glyph. A per-line colour is
            // ignored for the same reason. y is up, so "down" is a *negative* offset.
            for (x, y, l) in &placed {
                self.draw_line(
                    ctx,
                    l,
                    x + SHADOW_OFFSET,
                    y - SHADOW_OFFSET,
                    style,
                    &shadow,
                    &shadow,
                )?;
            }
        }
        for (x, y, l) in &placed {
            let fill = cg_color(l.color.as_deref().unwrap_or(&style.color), alpha)?;
            self.draw_line(ctx, l, *x, *y, style, &fill, &stroke)?;
        }
        Ok(())
    }

    /// One line, drawn as if its baseline origin were at (`x`, `y`). Fill and stroke share
    /// one path, which is what makes `DrawMode` map onto `CGPathDrawingMode` exactly.
    #[allow(clippy::too_many_arguments)]
    fn draw_line(
        &self,
        ctx: &CGContext,
        l: &Laid,
        x: f64,
        y: f64,
        style: &Style,
        fill: &CGColor,
        stroke: &CGColor,
    ) -> Result<()> {
        let width = f64::from(style.outline_width_px.max(0.0));
        let wants_fill = matches!(style.mode, DrawMode::Fill | DrawMode::Both);
        // A zero-width stroke is a no-op in Direct2D but a hairline in CoreGraphics, so
        // drop it here rather than draw something Windows would not.
        let wants_stroke = matches!(style.mode, DrawMode::Outline | DrawMode::Both) && width > 0.0;

        let mode = match (wants_fill, wants_stroke) {
            (true, true) => CGPathDrawingMode::FillStroke,
            (true, false) => CGPathDrawingMode::Fill,
            (false, true) => CGPathDrawingMode::Stroke,
            (false, false) => return Ok(()),
        };

        let path = glyph_path(&l.line, x, y)?;
        CGContext::set_fill_color_with_color(Some(ctx), Some(fill));
        CGContext::set_stroke_color_with_color(Some(ctx), Some(stroke));
        CGContext::set_line_width(Some(ctx), width);
        CGContext::begin_path(Some(ctx));
        CGContext::add_path(Some(ctx), Some(&path));
        CGContext::draw_path(Some(ctx), mode);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Align, Line, Style};

    /// Draws `lines` and returns the tightly packed premultiplied BGRA pixels -- the same
    /// bytes, in the same order, that the Windows backend's `draw` helper returns.
    fn draw(p: &Painter, lines: &[Line], style: &Style) -> (Vec<u8>, u32, u32) {
        let composed = p.compose(lines, style).unwrap();
        let (w, h) = composed.size();
        let canvas = p.render(&composed, style, 1.0).unwrap();
        assert_eq!(canvas.size(), (w, h));
        (canvas.pixels(), w, h)
    }

    /// The classic pair, with the templates already resolved -- the renderer never sees
    /// tokens, `AppCore` substitutes them first.
    fn lines() -> Vec<Line> {
        vec![
            Line {
                text: "3m 2w 0d".into(),
                size_ratio: crate::config::SUMMARY_SIZE_RATIO,
                ..Line::default()
            },
            Line {
                text: "2544:18:07".into(),
                ..Line::default()
            },
        ]
    }

    fn one(text: &str) -> Vec<Line> {
        vec![Line {
            text: text.into(),
            ..Line::default()
        }]
    }

    fn coverage(px: &[u8], w: u32, h: u32) -> f64 {
        let opaque = px.chunks_exact(4).filter(|p| p[3] != 0).count();
        opaque as f64 / (w * h) as f64
    }

    fn ink_bbox(px: &[u8], w: u32, h: u32) -> (u32, u32, u32, u32) {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for y in 0..h {
            for x in 0..w {
                if px[((y * w + x) * 4 + 3) as usize] != 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        (x0, y0, x1, y1)
    }

    /// The vertical extents of the fully empty row runs that sit *between* bands of ink.
    /// These are the gaps a person actually sees.
    fn empty_row_runs_between_ink(px: &[u8], w: u32, h: u32) -> Vec<u32> {
        let inked = |y: u32| (0..w).any(|x| px[((y * w + x) * 4 + 3) as usize] != 0);
        let first = (0..h).find(|&y| inked(y)).expect("no ink at all");
        let last = (0..h).rev().find(|&y| inked(y)).expect("no ink at all");

        let mut runs = Vec::new();
        let mut run = 0u32;
        for y in first..=last {
            if inked(y) {
                if run > 0 {
                    runs.push(run);
                    run = 0;
                }
            } else {
                run += 1;
            }
        }
        runs
    }

    #[test]
    fn renders_something() {
        let p = Painter::new().unwrap();
        let (px, w, h) = draw(&p, &lines(), &Style::default());
        assert!(w > 0 && h > 0);
        assert!(coverage(&px, w, h) > 0.01, "nothing was drawn");
        assert!(coverage(&px, w, h) < 0.9, "canvas is almost fully opaque");
    }

    /// The reason `ink_span` exists. The gap between the two lines must be the gap this
    /// module says it leaves, in any font -- including a CJK one, whose ascent is sized for
    /// Hangul while the countdown only ever draws digits. Measured against the line box
    /// instead, the visible gap would be several times too large.
    #[test]
    fn the_visible_gap_between_the_lines_is_the_configured_gap() {
        let p = Painter::new().unwrap();
        for family in ["Menlo", "Apple SD Gothic Neo"] {
            let size = 200.0;
            let style = Style {
                font_family: family.into(),
                size_px: size,
                shadow: false,
                outline_width_px: 0.0,
                ..Style::default()
            };
            let (px, w, h) = draw(&p, &lines(), &style);
            let runs = empty_row_runs_between_ink(&px, w, h);
            assert_eq!(
                runs.len(),
                1,
                "{family}: two lines leave one gap, got {runs:?}"
            );
            let gap = runs[0] as f32;
            let want = size * LINE_GAP_RATIO as f32;
            assert!(
                (gap - want).abs() <= 2.0,
                "{family}: visible gap was {gap}px, expected {want}px"
            );
        }
    }

    #[test]
    fn the_gap_is_uniform_across_three_lines() {
        let p = Painter::new().unwrap();
        let size = 200.0;
        let style = Style {
            size_px: size,
            shadow: false,
            outline_width_px: 0.0,
            ..Style::default()
        };
        let three = vec![
            Line {
                text: "111".into(),
                size_ratio: 0.3,
                ..Line::default()
            },
            Line {
                text: "222".into(),
                size_ratio: 0.5,
                ..Line::default()
            },
            Line {
                text: "333".into(),
                ..Line::default()
            },
        ];
        let (px, w, h) = draw(&p, &three, &style);
        let runs = empty_row_runs_between_ink(&px, w, h);
        assert_eq!(runs.len(), 2, "three lines leave two gaps, got {runs:?}");
        let want = size * LINE_GAP_RATIO as f32;
        for g in &runs {
            assert!(
                (*g as f32 - want).abs() <= 2.0,
                "gap was {g}px, expected {want}px"
            );
        }
    }

    #[test]
    fn an_empty_line_takes_no_room_at_all() {
        let p = Painter::new().unwrap();
        let mut with_blank = lines();
        with_blank.insert(1, Line::default()); // text: ""
        let a = p.measure(&lines(), &Style::default()).unwrap();
        let b = p.measure(&with_blank, &Style::default()).unwrap();
        assert_eq!(a, b, "a blank line must not add a gap or any height");
    }

    #[test]
    fn alignment_moves_the_short_line_within_the_canvas() {
        let p = Painter::new().unwrap();
        let style = Style {
            shadow: false,
            outline_width_px: 0.0,
            size_px: 120.0,
            ..Style::default()
        };
        // A short line over a long one, so the short one has room to move.
        let build = |align: Align| {
            vec![
                Line {
                    text: "8".into(),
                    size_ratio: 0.5,
                    align,
                    ..Line::default()
                },
                Line {
                    text: "88888888".into(),
                    ..Line::default()
                },
            ]
        };
        // The x of the first inked pixel on the topmost inked row says where the short line
        // sits inside the canvas.
        let top_ink_x = |align: Align| {
            let (px, w, h) = draw(&p, &build(align), &style);
            let row = (0..h)
                .find(|&y| (0..w).any(|x| px[((y * w + x) * 4 + 3) as usize] != 0))
                .expect("no ink");
            let x = (0..w)
                .find(|&x| px[((row * w + x) * 4 + 3) as usize] != 0)
                .expect("no ink in the first inked row");
            (x, w)
        };
        let (left, w) = top_ink_x(Align::Left);
        let (center, _) = top_ink_x(Align::Center);
        let (right, _) = top_ink_x(Align::Right);
        assert!(
            left < center,
            "left {left} should sit left of center {center}"
        );
        assert!(
            center < right,
            "center {center} should sit left of right {right}"
        );
        assert!(right < w, "right {right} must stay inside the canvas {w}");
    }

    /// Also pins down the sRGB-in, sRGB-out choice: a bitmap in a different colour space
    /// from the colours would convert, and pure red would no longer land on exactly
    /// (0, 0, 255, _) in BGRA.
    #[test]
    fn a_per_line_colour_overrides_the_global_one() {
        let p = Painter::new().unwrap();
        let style = Style {
            shadow: false,
            outline_width_px: 0.0,
            color: "#FFFFFF".into(),
            ..Style::default()
        };
        let red = vec![Line {
            text: "8".into(),
            color: Some("#FF0000".into()),
            ..Line::default()
        }];
        let (px, _, _) = draw(&p, &red, &style);
        // Premultiplied BGRA: a red line inks the red channel and nothing else.
        assert!(
            px.chunks_exact(4).any(|q| q[2] > 0),
            "nothing red was drawn"
        );
        assert!(
            px.chunks_exact(4).all(|q| q[0] == 0 && q[1] == 0),
            "a red line must not ink the blue or green channel"
        );
    }

    /// ...and the same dead space must not pad the canvas itself: the ink starts at the top
    /// padding and ends at the bottom padding.
    #[test]
    fn the_canvas_hugs_the_ink_vertically() {
        let p = Painter::new().unwrap();
        let style = Style {
            font_family: "Apple SD Gothic Neo".into(),
            size_px: 200.0,
            shadow: false,
            outline_width_px: 0.0,
            ..Style::default()
        };
        let (px, w, h) = draw(&p, &lines(), &style);
        let (_, y0, _, y1) = ink_bbox(&px, w, h);
        let pad = 4.0; // outline 0 + 4 + no shadow
        assert!(
            (y0 as f32 - pad).abs() <= 1.0,
            "ink starts {y0}px down, expected {pad}px of padding"
        );
        assert!(
            ((h - 1 - y1) as f32 - pad).abs() <= 1.0,
            "ink ends {}px from the bottom, expected {pad}px of padding",
            h - 1 - y1
        );
    }

    #[test]
    fn ink_stays_inside_the_canvas_with_padding() {
        let p = Painter::new().unwrap();
        let (px, w, h) = draw(&p, &lines(), &Style::default());
        let (x0, y0, x1, y1) = ink_bbox(&px, w, h);
        assert!(x0 >= 1 && y0 >= 1, "ink touches the top-left edge");
        assert!(
            x1 < w - 1 && y1 < h - 1,
            "ink touches the bottom-right edge"
        );
    }

    #[test]
    fn alpha_is_premultiplied() {
        let p = Painter::new().unwrap();
        let (px, _, _) = draw(&p, &lines(), &Style::default());
        for q in px.chunks_exact(4) {
            let (b, g, r, a) = (q[0], q[1], q[2], q[3]);
            assert!(b <= a && g <= a && r <= a, "channel exceeds alpha: {q:?}");
        }
    }

    #[test]
    fn measure_matches_what_render_needs() {
        let p = Painter::new().unwrap();
        let (w, h) = p.measure(&lines(), &Style::default()).unwrap();
        let (px, cw, ch) = draw(&p, &lines(), &Style::default());
        assert_eq!((w, h), (cw, ch));
        assert_eq!(px.len(), (w * h * 4) as usize);
    }

    /// Retina: the bitmap is in pixels, the layout in points.
    #[test]
    fn a_2x_scale_doubles_the_bitmap_but_not_the_layout() {
        let p = Painter::new().unwrap();
        let style = Style::default();
        let composed = p.compose(&lines(), &style).unwrap();
        let (w, h) = composed.size();

        let one = p.render(&composed, &style, 1.0).unwrap();
        let two = p.render(&composed, &style, 2.0).unwrap();

        assert_eq!(one.size(), (w, h));
        assert_eq!(two.size(), (w * 2, h * 2));
        // Same drawing, four times the pixels: the coverage ratio barely moves.
        let c1 = coverage(&one.pixels(), w, h);
        let c2 = coverage(&two.pixels(), w * 2, h * 2);
        assert!(
            (c1 - c2).abs() < 0.05,
            "scaling changed what is drawn: {c1} vs {c2}"
        );
    }

    #[test]
    fn dropping_a_line_shrinks_the_canvas() {
        let p = Painter::new().unwrap();
        let tall = p.measure(&lines(), &Style::default()).unwrap();
        let short = p.measure(&lines()[1..], &Style::default()).unwrap();
        assert!(short.1 < tall.1);
    }

    #[test]
    fn bigger_font_yields_a_bigger_canvas() {
        let p = Painter::new().unwrap();
        let small = p
            .measure(
                &lines(),
                &Style {
                    size_px: 32.0,
                    ..Style::default()
                },
            )
            .unwrap();
        let big = p
            .measure(
                &lines(),
                &Style {
                    size_px: 96.0,
                    ..Style::default()
                },
            )
            .unwrap();
        assert!(big.0 > small.0 && big.1 > small.1);
    }

    #[test]
    fn missing_font_falls_back_instead_of_failing() {
        let p = Painter::new().unwrap();
        let style = Style {
            font_family: "NoSuchFontFamily12345".into(),
            ..Style::default()
        };
        let (px, w, h) = draw(&p, &lines(), &style);
        assert!(coverage(&px, w, h) > 0.01);
    }

    #[test]
    fn shadow_adds_ink() {
        let p = Painter::new().unwrap();
        let (a, w1, h1) = draw(
            &p,
            &lines(),
            &Style {
                shadow: false,
                ..Style::default()
            },
        );
        let (b, w2, h2) = draw(
            &p,
            &lines(),
            &Style {
                shadow: true,
                ..Style::default()
            },
        );
        assert!(coverage(&b, w2, h2) > coverage(&a, w1, h1));
    }

    #[test]
    fn opacity_scales_alpha() {
        let p = Painter::new().unwrap();
        let full = Style {
            opacity: 1.0,
            shadow: false,
            ..Style::default()
        };
        let half = Style {
            opacity: 0.5,
            shadow: false,
            ..Style::default()
        };
        let (a, _, _) = draw(&p, &lines(), &full);
        let (b, _, _) = draw(&p, &lines(), &half);
        let peak = |px: &[u8]| px.chunks_exact(4).map(|q| q[3]).max().unwrap();
        assert_eq!(peak(&a), 255);
        assert!(
            (120..=136).contains(&peak(&b)),
            "peak alpha was {}",
            peak(&b)
        );
    }

    #[test]
    fn tabular_figures_keep_the_width_constant_across_digits() {
        let p = Painter::new().unwrap();
        let style = Style {
            tabular_figures: true,
            ..Style::default()
        };
        let a = p.measure(&one("11:11:11"), &style).unwrap();
        let b = p.measure(&one("00:00:00"), &style).unwrap();
        assert_eq!(a.0, b.0);
    }

    #[test]
    fn outline_mode_draws_less_ink_than_fill() {
        let p = Painter::new().unwrap();
        let base = Style {
            shadow: false,
            ..Style::default()
        };
        let (f, fw, fh) = draw(
            &p,
            &lines(),
            &Style {
                mode: DrawMode::Fill,
                ..base.clone()
            },
        );
        let (o, ow, oh) = draw(
            &p,
            &lines(),
            &Style {
                mode: DrawMode::Outline,
                ..base.clone()
            },
        );
        assert!(coverage(&o, ow, oh) > 0.005, "outline drew nothing");
        assert!(
            coverage(&o, ow, oh) < coverage(&f, fw, fh),
            "outline {} should be lighter than fill {}",
            coverage(&o, ow, oh),
            coverage(&f, fw, fh)
        );
    }

    #[test]
    fn both_mode_draws_more_ink_than_fill() {
        let p = Painter::new().unwrap();
        let base = Style {
            shadow: false,
            outline_width_px: 3.0,
            ..Style::default()
        };
        let (f, fw, fh) = draw(
            &p,
            &lines(),
            &Style {
                mode: DrawMode::Fill,
                ..base.clone()
            },
        );
        let (b, bw, bh) = draw(
            &p,
            &lines(),
            &Style {
                mode: DrawMode::Both,
                ..base.clone()
            },
        );
        assert!(coverage(&b, bw, bh) > coverage(&f, fw, fh));
    }

    #[test]
    fn outline_mode_leaves_glyph_centres_transparent() {
        // A thin outline of a large '0' must leave its interior empty.
        let p = Painter::new().unwrap();
        let style = Style {
            mode: DrawMode::Outline,
            shadow: false,
            size_px: 200.0,
            outline_width_px: 1.5,
            ..Style::default()
        };
        let (px, w, h) = draw(&p, &one("0"), &style);
        let a = px[(((h / 2) * w + w / 2) * 4 + 3) as usize];
        assert_eq!(
            a, 0,
            "the centre of '0' should be transparent in outline mode"
        );
    }
}
