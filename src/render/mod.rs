//! Draws the countdown onto any ID2D1RenderTarget. Knows nothing about
//! DirectComposition, windows, or where the pixels end up.

mod outline;
mod text;

use anyhow::{anyhow, Result};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::IDWriteTextLayout;
use windows_numerics::Vector2;

use crate::color::parse_hex;
use crate::config::{DrawMode, Style};
use text::TextEngine;

/// Gap between the summary line's ink and the main line's ink, as a fraction of `size_px`.
/// Ink, not line box: see `ink_span`.
const LINE_GAP_RATIO: f32 = 0.12;
/// Summary line em size, as a fraction of `size_px`.
const SUMMARY_RATIO: f32 = 0.28;
/// Shadow offset in pixels. No blur; see spec section 3.2.
const SHADOW_OFFSET: f32 = 2.0;
const SHADOW_ALPHA: f32 = 0.55;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lines {
    pub summary: Option<String>,
    pub main: String,
}

pub struct Painter {
    d2d: ID2D1Factory1,
    text: TextEngine,
}

/// One line, ready to draw: its layout, the advance width used to centre it, and where
/// its ink sits inside its line box (see `ink_span`).
struct Line {
    layout: IDWriteTextLayout,
    width: f32,
    /// Distance from the layout box's top edge down to the first inked pixel.
    ink_top: f32,
    /// Height of the inked pixels themselves.
    ink_h: f32,
}

/// A laid-out pair of lines and the canvas they need. Built once per redraw by `compose`;
/// `size` sizes the composition surface and `paint` draws it. Held by the caller between
/// the two so the (not free -- see `ink_span`) layout work happens once, not twice.
pub struct Composed {
    main: Line,
    summary: Option<Line>,
    pad: f32,
    gap: f32,
    content_w: f32,
    width: u32,
    height: u32,
}

impl Composed {
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Where `layout`'s ink actually is: `(top, height)`, measured from the top edge of its
/// line box.
///
/// `DWRITE_TEXT_METRICS::height` is the *line box* -- ascent + descent + line gap -- which
/// is a property of the font, not of the string. A CJK font sizes its ascent to hold
/// Hangul and Han (Noto Sans Mono CJK: 1.16em) while digits only reach the cap height
/// (~0.7em), so a line of digits floats with ~0.46em of nothing above it. Stacking two
/// such boxes puts ~0.6em of dead space between the two lines' glyphs, five times the gap
/// this module thinks it is leaving. Measuring the ink instead makes `LINE_GAP_RATIO` mean
/// what it says, in any font.
///
/// The bounds come from the glyph outlines (the same ones `DrawMode::Outline` strokes), so
/// they are exact for the actual string.  `IDWriteTextLayout::GetOverhangMetrics` would be
/// cheaper but reports the font's global glyph box, not this string's -- it claims ink
/// 2.6x below the line box for both fonts tested here, which is useless for fitting.
///
/// A string with no ink at all (all spaces) falls back to the line box.
fn ink_span(d2d: &ID2D1Factory1, layout: &IDWriteTextLayout, box_h: f32) -> Result<(f32, f32)> {
    let mut top = f32::MAX;
    let mut bottom = f32::MIN;
    for geom in outline::collect_geometry(d2d, layout, 0.0, 0.0)? {
        let b = unsafe { geom.GetBounds(None)? };
        top = top.min(b.top);
        bottom = bottom.max(b.bottom);
    }
    if top > bottom {
        return Ok((0.0, box_h));
    }
    Ok((top, bottom - top))
}

impl Painter {
    pub fn new() -> Result<Self> {
        let d2d: ID2D1Factory1 =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        Ok(Self {
            d2d,
            text: TextEngine::new()?,
        })
    }

    /// Needed by `dcomp` to build a D2D device, and by tests to build a WIC target.
    pub fn d2d_factory(&self) -> &ID2D1Factory1 {
        &self.d2d
    }

    pub fn measure(&self, lines: &Lines, style: &Style) -> Result<(u32, u32)> {
        Ok(self.compose(lines, style)?.size())
    }

    /// Lays both lines out and sizes the canvas they need. Vertically the canvas holds the
    /// two lines' *ink* plus the gap between them, not their line boxes -- see `ink_span`.
    pub fn compose(&self, lines: &Lines, style: &Style) -> Result<Composed> {
        let family = self.text.resolve_family(&style.font_family);

        let main = self.line(&lines.main, &family, style, style.size_px)?;
        let summary = match (&lines.summary, style.show_summary_line) {
            (Some(s), true) => Some(self.line(s, &family, style, style.size_px * SUMMARY_RATIO)?),
            _ => None,
        };

        let gap = if summary.is_some() {
            style.size_px * LINE_GAP_RATIO
        } else {
            0.0
        };
        let sum_w = summary.as_ref().map(|s| s.width).unwrap_or(0.0);
        let sum_ink_h = summary.as_ref().map(|s| s.ink_h).unwrap_or(0.0);

        let pad = (style.outline_width_px.max(0.0)
            + 4.0
            + if style.shadow { SHADOW_OFFSET } else { 0.0 })
        .ceil();

        let content_w = main.width.max(sum_w);
        let content_h = sum_ink_h + gap + main.ink_h;

        Ok(Composed {
            main,
            summary,
            pad,
            gap,
            content_w,
            width: (content_w + pad * 2.0).ceil().max(1.0) as u32,
            height: (content_h + pad * 2.0).ceil().max(1.0) as u32,
        })
    }

    fn line(&self, text: &str, family: &str, style: &Style, size_px: f32) -> Result<Line> {
        let layout = self.text.layout(text, family, style, size_px)?;
        let (width, box_h) = TextEngine::measure(&layout)?;
        let (ink_top, ink_h) = ink_span(&self.d2d, &layout, box_h)?;
        Ok(Line {
            layout,
            width,
            ink_top,
            ink_h,
        })
    }

    /// `origin` is the surface offset from `IDCompositionSurface::BeginDraw`.
    /// The caller owns BeginDraw/EndDraw on `rt`.
    pub fn paint(
        &self,
        rt: &ID2D1RenderTarget,
        c: &Composed,
        style: &Style,
        origin: (f32, f32),
    ) -> Result<()> {
        let (ox, oy) = origin;
        let alpha = style.opacity.clamp(0.0, 1.0);

        unsafe {
            // ClearType is subpixel and would corrupt the alpha channel we composite with.
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

            // The surface may be part of an atlas: never touch pixels outside our slot.
            let clip = D2D_RECT_F {
                left: ox,
                top: oy,
                right: ox + c.width as f32,
                bottom: oy + c.height as f32,
            };
            rt.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_ALIASED);
            rt.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));

            let result = self.paint_inner(rt, c, style, ox, oy, alpha);

            // Pop on every path, including an error from paint_inner, so a failed
            // paint never leaves the render target's clip stack unbalanced.
            rt.PopAxisAlignedClip();
            result?;
        }
        Ok(())
    }

    /// The actual drawing, run inside the clip pushed by `paint`. Split out so
    /// `paint` can guarantee the clip is popped on every return path, including
    /// an error here (see `PopAxisAlignedClip` above).
    fn paint_inner(
        &self,
        rt: &ID2D1RenderTarget,
        c: &Composed,
        style: &Style,
        ox: f32,
        oy: f32,
        alpha: f32,
    ) -> Result<()> {
        let ink = self.brush(rt, &style.color, alpha)?;
        let line = self.brush(rt, &style.outline_color, alpha)?;
        let shadow = self.brush(rt, "#000000", SHADOW_ALPHA * alpha)?;

        let sum_w = c.summary.as_ref().map(|s| s.width).unwrap_or(0.0);
        let sum_ink_h = c.summary.as_ref().map(|s| s.ink_h).unwrap_or(0.0);
        let sum_x = ox + c.pad + (c.content_w - sum_w) / 2.0;
        let main_x = ox + c.pad + (c.content_w - c.main.width) / 2.0;

        // `DrawTextLayout` takes the layout box's top-left, but the canvas was sized to the
        // ink. Shift each line up by the dead space above its own ink so the ink lands where
        // the layout put it: summary ink at the top pad, main ink one gap below it.
        let sum_y = oy + c.pad - c.summary.as_ref().map(|s| s.ink_top).unwrap_or(0.0);
        let main_y = oy + c.pad + sum_ink_h + c.gap - c.main.ink_top;

        if style.shadow {
            // Both fill and stroke draw in the shadow colour here, otherwise an
            // outline-mode glyph would show a filled shadow behind a hollow glyph.
            self.pass(
                rt,
                c,
                sum_x + SHADOW_OFFSET,
                sum_y + SHADOW_OFFSET,
                main_x + SHADOW_OFFSET,
                main_y + SHADOW_OFFSET,
                style,
                &shadow,
                &shadow,
            )?;
        }
        self.pass(rt, c, sum_x, sum_y, main_x, main_y, style, &ink, &line)?;
        Ok(())
    }

    /// One draw of both lines: `DrawMode::Fill`/`Both` fill with `fill`,
    /// `DrawMode::Outline`/`Both` stroke glyph outlines with `stroke`.
    #[allow(clippy::too_many_arguments)]
    fn pass(
        &self,
        rt: &ID2D1RenderTarget,
        c: &Composed,
        sum_x: f32,
        sum_y: f32,
        main_x: f32,
        main_y: f32,
        style: &Style,
        fill: &ID2D1SolidColorBrush,
        stroke: &ID2D1SolidColorBrush,
    ) -> Result<()> {
        if matches!(style.mode, DrawMode::Fill | DrawMode::Both) {
            unsafe {
                if let Some(s) = &c.summary {
                    rt.DrawTextLayout(
                        Vector2 { X: sum_x, Y: sum_y },
                        &s.layout,
                        fill,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                }
                rt.DrawTextLayout(
                    Vector2 {
                        X: main_x,
                        Y: main_y,
                    },
                    &c.main.layout,
                    fill,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }
        if matches!(style.mode, DrawMode::Outline | DrawMode::Both) {
            if let Some(s) = &c.summary {
                self.stroke_layout(rt, &s.layout, sum_x, sum_y, stroke, style.outline_width_px)?;
            }
            self.stroke_layout(
                rt,
                &c.main.layout,
                main_x,
                main_y,
                stroke,
                style.outline_width_px,
            )?;
        }
        Ok(())
    }

    /// Strokes every glyph run's outline geometry in `layout`, drawn as if `layout`
    /// were placed at (`x`, `y`) via `DrawTextLayout`.
    fn stroke_layout(
        &self,
        rt: &ID2D1RenderTarget,
        layout: &IDWriteTextLayout,
        x: f32,
        y: f32,
        brush: &ID2D1SolidColorBrush,
        width: f32,
    ) -> Result<()> {
        for geom in outline::collect_geometry(&self.d2d, layout, x, y)? {
            unsafe { rt.DrawGeometry(&geom, brush, width, None) };
        }
        Ok(())
    }

    fn brush(&self, rt: &ID2D1RenderTarget, hex: &str, alpha: f32) -> Result<ID2D1SolidColorBrush> {
        let c = parse_hex(hex).ok_or_else(|| anyhow!("invalid colour {hex}"))?;
        let color = D2D1_COLOR_F {
            r: c.r as f32 / 255.0,
            g: c.g as f32 / 255.0,
            b: c.b as f32 / 255.0,
            a: alpha,
        };
        Ok(unsafe { rt.CreateSolidColorBrush(&color, None)? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;
    use windows::Win32::Graphics::Direct2D::Common::{
        D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
    };
    use windows::Win32::Graphics::Direct2D::{
        ID2D1RenderTarget, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
        D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    use windows::Win32::Graphics::Imaging::*;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };

    /// An offscreen render target. `render` cannot tell it apart from a DComp surface.
    fn canvas(p: &Painter, w: u32, h: u32) -> (IWICBitmap, ID2D1RenderTarget) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).unwrap();
            let bitmap = wic
                .CreateBitmap(w, h, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)
                .unwrap();
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let rt = p
                .d2d_factory()
                .CreateWicBitmapRenderTarget(&bitmap, &props)
                .unwrap();
            (bitmap, rt)
        }
    }

    /// Draws `lines` and returns the tightly packed premultiplied BGRA pixels.
    fn draw(p: &Painter, lines: &Lines, style: &Style) -> (Vec<u8>, u32, u32) {
        let composed = p.compose(lines, style).unwrap();
        let (w, h) = composed.size();
        let (bitmap, rt) = canvas(p, w, h);
        unsafe {
            rt.BeginDraw();
            p.paint(&rt, &composed, style, (0.0, 0.0)).unwrap();
            rt.EndDraw(None, None).unwrap();

            let rect = WICRect {
                X: 0,
                Y: 0,
                Width: w as i32,
                Height: h as i32,
            };
            let lock = bitmap.Lock(&rect, WICBitmapLockRead.0 as u32).unwrap();
            let stride = lock.GetStride().unwrap();
            let mut size = 0u32;
            let mut ptr = std::ptr::null_mut();
            lock.GetDataPointer(&mut size, &mut ptr).unwrap();

            let row = (w * 4) as usize;
            let mut out = vec![0u8; row * h as usize];
            for y in 0..h as usize {
                std::ptr::copy_nonoverlapping(
                    ptr.add(y * stride as usize),
                    out.as_mut_ptr().add(y * row),
                    row,
                );
            }
            (out, w, h)
        }
    }

    fn lines() -> Lines {
        Lines {
            summary: Some("3m 2w 0d".into()),
            main: "2544:18:07".into(),
        }
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

    #[test]
    fn renders_something() {
        let p = Painter::new().unwrap();
        let (px, w, h) = draw(&p, &lines(), &Style::default());
        assert!(w > 0 && h > 0);
        assert!(coverage(&px, w, h) > 0.01, "nothing was drawn");
        assert!(coverage(&px, w, h) < 0.9, "canvas is almost fully opaque");
    }

    /// The vertical extent of a row of pixels that has no ink at all, between the summary
    /// line's ink and the main line's ink. This is the gap a person actually sees.
    fn empty_rows_between_lines(px: &[u8], w: u32, h: u32) -> u32 {
        let inked = |y: u32| (0..w).any(|x| px[((y * w + x) * 4 + 3) as usize] != 0);
        let first = (0..h).find(|&y| inked(y)).expect("no ink at all");
        // Skip the summary's own rows, then count the empty ones before the main line.
        let after_summary = (first..h)
            .find(|&y| !inked(y))
            .expect("only one band of ink");
        (after_summary..h).take_while(|&y| !inked(y)).count() as u32
    }

    /// The gap between the two lines must be the gap this module says it leaves, in any
    /// font. It used to be the gap *plus* whatever dead space the font's ascent and descent
    /// left around the glyphs -- ~5x too much in a CJK font, whose ascent is sized for
    /// Hangul while the countdown only ever draws digits.
    #[test]
    fn the_visible_gap_between_the_lines_is_the_configured_gap() {
        let p = Painter::new().unwrap();
        for family in ["Consolas", "Malgun Gothic"] {
            let size = 200.0;
            let style = Style {
                font_family: family.into(),
                size_px: size,
                shadow: false,
                outline_width_px: 0.0,
                ..Style::default()
            };
            let (px, w, h) = draw(&p, &lines(), &style);
            let gap = empty_rows_between_lines(&px, w, h) as f32;
            let want = size * LINE_GAP_RATIO;
            assert!(
                (gap - want).abs() <= 2.0,
                "{family}: visible gap was {gap}px, expected {want}px"
            );
        }
    }

    /// ...and the same dead space must not pad the canvas itself: the ink starts at the top
    /// padding and ends at the bottom padding.
    #[test]
    fn the_canvas_hugs_the_ink_vertically() {
        let p = Painter::new().unwrap();
        let style = Style {
            font_family: "Malgun Gothic".into(),
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
    fn measure_matches_what_paint_needs() {
        let p = Painter::new().unwrap();
        let (w, h) = p.measure(&lines(), &Style::default()).unwrap();
        let (px, cw, ch) = draw(&p, &lines(), &Style::default());
        assert_eq!((w, h), (cw, ch));
        assert_eq!(px.len(), (w * h * 4) as usize);
    }

    #[test]
    fn hiding_the_summary_line_shrinks_the_canvas() {
        let p = Painter::new().unwrap();
        let tall = p.measure(&lines(), &Style::default()).unwrap();
        let short = p
            .measure(
                &Lines {
                    summary: None,
                    main: "2544:18:07".into(),
                },
                &Style::default(),
            )
            .unwrap();
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
            show_summary_line: false,
            ..Style::default()
        };
        let a = p
            .measure(
                &Lines {
                    summary: None,
                    main: "11:11:11".into(),
                },
                &style,
            )
            .unwrap();
        let b = p
            .measure(
                &Lines {
                    summary: None,
                    main: "00:00:00".into(),
                },
                &style,
            )
            .unwrap();
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
            show_summary_line: false,
            size_px: 200.0,
            outline_width_px: 1.5,
            ..Style::default()
        };
        let l = Lines {
            summary: None,
            main: "0".into(),
        };
        let (px, w, h) = draw(&p, &l, &style);
        let a = px[(((h / 2) * w + w / 2) * 4 + 3) as usize];
        assert_eq!(
            a, 0,
            "the centre of '0' should be transparent in outline mode"
        );
    }
}
