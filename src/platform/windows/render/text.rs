//! DirectWrite text layout construction. No Direct2D.

use anyhow::Result;
use windows::core::{Interface, HSTRING};
use windows::Win32::Graphics::DirectWrite::*;

use crate::config::Style;

/// Families tried, in order, when the configured one is not installed.
const FALLBACKS: [&str; 2] = ["Consolas", "Segoe UI"];

pub struct TextEngine {
    factory: IDWriteFactory,
}

impl TextEngine {
    pub fn new() -> Result<Self> {
        let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        Ok(Self { factory })
    }

    fn family_exists(&self, family: &str) -> bool {
        unsafe {
            let mut coll = None;
            if self
                .factory
                .GetSystemFontCollection(&mut coll, false)
                .is_err()
            {
                return false;
            }
            let Some(coll) = coll else { return false };
            let mut index = 0u32;
            let mut exists = windows::core::BOOL(0);
            if coll
                .FindFamilyName(&HSTRING::from(family), &mut index, &mut exists)
                .is_err()
            {
                return false;
            }
            exists.as_bool()
        }
    }

    /// The configured family if installed, otherwise the first installed fallback.
    pub fn resolve_family(&self, family: &str) -> String {
        if self.family_exists(family) {
            return family.to_string();
        }
        tracing::warn!(family, "font family not installed, falling back");
        for f in FALLBACKS {
            if self.family_exists(f) {
                return f.to_string();
            }
        }
        family.to_string()
    }

    /// A single-line layout. `size_px` is the em size in physical pixels.
    pub fn layout(
        &self,
        text: &str,
        family: &str,
        style: &Style,
        size_px: f32,
    ) -> Result<IDWriteTextLayout> {
        let utf16: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let format = self.factory.CreateTextFormat(
                &HSTRING::from(family),
                None,
                DWRITE_FONT_WEIGHT(style.font_weight as i32),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size_px,
                &HSTRING::from("ko-kr"),
            )?;
            let layout = self
                .factory
                .CreateTextLayout(&utf16, &format, 8192.0, 8192.0)?;
            let range = DWRITE_TEXT_RANGE {
                startPosition: 0,
                length: utf16.len() as u32,
            };

            if style.letter_spacing_em != 0.0 {
                let l1: IDWriteTextLayout1 = layout.cast()?;
                l1.SetCharacterSpacing(0.0, style.letter_spacing_em * size_px, 0.0, range)?;
            }
            if style.tabular_figures {
                let typo = self.factory.CreateTypography()?;
                typo.AddFontFeature(DWRITE_FONT_FEATURE {
                    nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
                    parameter: 1,
                })?;
                layout.SetTypography(&typo, range)?;
            }
            Ok(layout)
        }
    }

    pub fn measure(layout: &IDWriteTextLayout) -> Result<(f32, f32)> {
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut m)? };
        Ok((m.widthIncludingTrailingWhitespace, m.height))
    }
}
