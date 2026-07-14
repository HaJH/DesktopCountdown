//! CoreText line layout. No CoreGraphics drawing. The macOS counterpart of the Windows
//! backend's `render/text.rs`.

use std::ffi::c_void;
use std::ptr;

use anyhow::{anyhow, Result};
use objc2_core_foundation::{
    kCFTypeArrayCallBacks, kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks,
    CFMutableArray, CFMutableAttributedString, CFMutableDictionary, CFNumber, CFNumberType,
    CFRange, CFRetained, CFString,
};
use objc2_core_text::{
    kCTFontAttributeName, kCTFontFamilyNameAttribute, kCTFontFeatureSettingsAttribute,
    kCTFontOpenTypeFeatureTag, kCTFontOpenTypeFeatureValue, kCTFontTraitsAttribute,
    kCTFontWeightTrait, kCTTrackingAttributeName, CTFont, CTFontDescriptor,
    CTFontManagerCopyAvailableFontFamilyNames, CTLine,
};

use crate::config::Style;

/// Families tried, in order, when the configured one is not installed. The Windows
/// backend's `["Consolas", "Segoe UI"]`, in macOS terms: a monospace face first, because a
/// countdown is digits and they should not jitter.
const FALLBACKS: [&str; 3] = ["SF Mono", "Menlo", "Helvetica Neue"];

/// The config carries DirectWrite's 100..900 weights. CoreText wants `kCTFontWeightTrait`,
/// which runs -1.0..1.0 on a scale of its own.
///
/// These anchors are AppKit's `NSFont.Weight` constants (ultraLight, thin, light, regular,
/// medium, semibold, bold, heavy, black) lined up against the CSS/DirectWrite weights they
/// conventionally correspond to. **Apple does not document the numeric values** -- they are
/// read out of AppKit at runtime by everyone who needs them -- so treat this table as
/// empirical, not normative. The tests below pin down its shape; only a person looking at
/// rendered text can confirm the feel.
const WEIGHT_ANCHORS: [(f32, f32); 9] = [
    (100.0, -0.80),
    (200.0, -0.60),
    (300.0, -0.40),
    (400.0, 0.00),
    (500.0, 0.23),
    (600.0, 0.30),
    (700.0, 0.40),
    (800.0, 0.56),
    (900.0, 0.62),
];

/// Piecewise-linear interpolation over `WEIGHT_ANCHORS`, clamped at both ends.
fn ct_weight(dwrite: u16) -> f32 {
    let w = f32::from(dwrite);
    let first = WEIGHT_ANCHORS[0];
    let last = WEIGHT_ANCHORS[WEIGHT_ANCHORS.len() - 1];
    if w <= first.0 {
        return first.1;
    }
    for pair in WEIGHT_ANCHORS.windows(2) {
        let (lo_w, lo_ct) = pair[0];
        let (hi_w, hi_ct) = pair[1];
        if w <= hi_w {
            let t = (w - lo_w) / (hi_w - lo_w);
            return lo_ct + t * (hi_ct - lo_ct);
        }
    }
    last.1
}

/// One laid-out line, plus the metrics `render` needs in order to stack it.
pub struct Laid {
    pub line: CFRetained<CTLine>,
    /// Advance width, trailing whitespace included -- what DirectWrite calls
    /// `widthIncludingTrailingWhitespace`.
    pub width: f64,
    /// The line box: ascent + descent + leading. Only the extent a string with no ink at
    /// all falls back to.
    pub box_h: f64,
    /// Distance from the baseline up to the top of the line box. The no-ink fallback places
    /// the line by this.
    pub ascent: f64,
}

pub struct TextEngine;

impl TextEngine {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    /// The configured family if installed, otherwise the first installed fallback.
    ///
    /// CoreText answers a request for a family it does not have with some other font, and
    /// says nothing -- so, exactly as on Windows, we check first and choose the fallback
    /// ourselves rather than let it choose for us.
    pub fn resolve_family(&self, family: &str) -> String {
        if family_exists(family) {
            return family.to_string();
        }
        tracing::warn!(family, "font family not installed, falling back");
        for f in FALLBACKS {
            if family_exists(f) {
                return f.to_string();
            }
        }
        family.to_string()
    }

    /// A single line, laid out. `size_px` is the em size in physical pixels.
    pub fn layout(&self, text: &str, family: &str, style: &Style, size_px: f32) -> Result<Laid> {
        let font = make_font(family, style, f64::from(size_px))?;

        let attributed = CFMutableAttributedString::new(None, 0)
            .ok_or_else(|| anyhow!("could not allocate a CFAttributedString"))?;
        let cf_text = CFString::from_str(text);
        // CoreFoundation counts UTF-16 code units, not bytes and not chars.
        let len = text.encode_utf16().count() as isize;

        // SAFETY: the replaced range is empty and at offset 0, which is in bounds of the
        // empty string just created; `whole` then covers exactly what was inserted. Every
        // attribute value below is of the type its key requires.
        unsafe {
            CFMutableAttributedString::replace_string(
                Some(&attributed),
                CFRange {
                    location: 0,
                    length: 0,
                },
                Some(&cf_text),
            );

            let whole = CFRange {
                location: 0,
                length: len,
            };
            CFMutableAttributedString::set_attribute(
                Some(&attributed),
                whole,
                Some(kCTFontAttributeName),
                Some(&**font),
            );

            if style.letter_spacing_em != 0.0 {
                // CoreText tracking is in points, which is the same absolute quantity as
                // the trailing spacing DirectWrite's `SetCharacterSpacing` takes -- so the
                // config's em value converts the same way on both platforms.
                let tracking = cf_number(f64::from(style.letter_spacing_em) * f64::from(size_px))?;
                CFMutableAttributedString::set_attribute(
                    Some(&attributed),
                    whole,
                    Some(kCTTrackingAttributeName),
                    Some(&**tracking),
                );
            }
        }

        // SAFETY: a CFMutableAttributedString is a CFAttributedString.
        let line = unsafe { CTLine::with_attributed_string(&attributed) };

        let mut ascent = 0.0f64;
        let mut descent = 0.0f64;
        let mut leading = 0.0f64;
        // SAFETY: all three out-pointers are valid and initialized.
        let width = unsafe { line.typographic_bounds(&mut ascent, &mut descent, &mut leading) };

        Ok(Laid {
            line,
            width,
            box_h: ascent + descent + leading,
            ascent,
        })
    }
}

fn family_exists(family: &str) -> bool {
    let want = CFString::from_str(family);
    // SAFETY: the API returns a CFArray of CFString.
    let names = unsafe { CTFontManagerCopyAvailableFontFamilyNames() };
    (0..names.count()).any(|i| {
        let p = unsafe { names.value_at_index(i) } as *const CFString;
        // SAFETY: the array holds CFStrings and outlives this comparison.
        unsafe { p.as_ref() }.is_some_and(|name| name == &*want)
    })
}

/// A `CTFont` for `family` at `size_px`, carrying the config's weight and -- when asked for
/// -- tabular figures.
fn make_font(family: &str, style: &Style, size_px: f64) -> Result<CFRetained<CTFont>> {
    let attrs = new_dictionary()?;
    let cf_family = CFString::from_str(family);

    let traits = new_dictionary()?;
    let weight = cf_number(f64::from(ct_weight(style.font_weight)))?;

    // SAFETY: each key is the documented CoreText attribute key, and each value has the
    // type that key requires. `CFDictionarySetValue` retains both under the CFType
    // callbacks `new_dictionary` installs.
    unsafe {
        CFMutableDictionary::set_value(Some(&traits), as_ptr(kCTFontWeightTrait), as_ptr(&*weight));
        CFMutableDictionary::set_value(
            Some(&attrs),
            as_ptr(kCTFontFamilyNameAttribute),
            as_ptr(&*cf_family),
        );
        CFMutableDictionary::set_value(
            Some(&attrs),
            as_ptr(kCTFontTraitsAttribute),
            as_ptr(&*traits),
        );

        if style.tabular_figures {
            let settings = tabular_figures_setting()?;
            CFMutableDictionary::set_value(
                Some(&attrs),
                as_ptr(kCTFontFeatureSettingsAttribute),
                as_ptr(&*settings),
            );
        }
    }

    // SAFETY: `attrs` is a CFMutableDictionary, which is a CFDictionary. A null matrix is
    // the identity.
    let descriptor = unsafe { CTFontDescriptor::with_attributes(&attrs) };
    Ok(unsafe { CTFont::with_font_descriptor(&descriptor, size_px, ptr::null()) })
}

/// `[{ CTFeatureOpenTypeTag: "tnum", CTFeatureOpenTypeValue: 1 }]` -- the OpenType spelling
/// of the same `tnum` feature the Windows backend asks DirectWrite for with
/// `DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES`.
fn tabular_figures_setting() -> Result<CFRetained<CFMutableArray>> {
    let setting = new_dictionary()?;
    let tag = CFString::from_str("tnum");
    let on = cf_number(1.0)?;

    // SAFETY: documented keys, values of the types they require, and a CFType-callback
    // array that retains what it is handed.
    let settings = unsafe {
        CFMutableDictionary::set_value(
            Some(&setting),
            as_ptr(kCTFontOpenTypeFeatureTag),
            as_ptr(&*tag),
        );
        CFMutableDictionary::set_value(
            Some(&setting),
            as_ptr(kCTFontOpenTypeFeatureValue),
            as_ptr(&*on),
        );

        let settings = CFMutableArray::new(None, 0, &kCFTypeArrayCallBacks)
            .ok_or_else(|| anyhow!("could not allocate a CFArray"))?;
        CFMutableArray::append_value(Some(&settings), as_ptr(&*setting));
        settings
    };
    Ok(settings)
}

/// A mutable dictionary that retains its keys and values, which is what every CoreText
/// attribute dictionary needs.
fn new_dictionary() -> Result<CFRetained<CFMutableDictionary>> {
    // SAFETY: the two callback tables are CoreFoundation's own.
    unsafe {
        CFMutableDictionary::new(
            None,
            0,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )
    }
    .ok_or_else(|| anyhow!("could not allocate a CFDictionary"))
}

fn cf_number(v: f64) -> Result<CFRetained<CFNumber>> {
    // SAFETY: `DoubleType` matches the `f64` being pointed at.
    unsafe {
        CFNumber::new(
            None,
            CFNumberType::DoubleType,
            &v as *const f64 as *const c_void,
        )
    }
    .ok_or_else(|| anyhow!("could not allocate a CFNumber"))
}

/// CoreFoundation's untyped-pointer collections take everything as `*const c_void`.
fn as_ptr<T>(v: &T) -> *const c_void {
    v as *const T as *const c_void
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_weight_map_hits_its_anchors_exactly() {
        for (dwrite, ct) in WEIGHT_ANCHORS {
            assert_eq!(ct_weight(dwrite as u16), ct, "weight {dwrite}");
        }
    }

    /// The config validates 100..900, but a hand-edited file can still carry anything a
    /// `u16` holds. Whatever comes in, CoreText must get a value inside its own range, and
    /// heavier text must never map to a lighter face.
    #[test]
    fn the_weight_map_is_monotonic_and_bounded() {
        let mut prev = f32::MIN;
        for w in (0..=1000u16).step_by(10) {
            let ct = ct_weight(w);
            assert!((-1.0..=1.0).contains(&ct), "weight {w} mapped to {ct}");
            assert!(ct >= prev, "weight {w} went backwards: {ct} after {prev}");
            prev = ct;
        }
        assert_eq!(
            ct_weight(0),
            -0.80,
            "below the table clamps to the lightest"
        );
        assert_eq!(ct_weight(u16::MAX), 0.62, "above it clamps to the heaviest");
    }

    #[test]
    fn interpolates_between_two_anchors() {
        // Halfway between 400 (0.0) and 500 (0.23).
        assert!((ct_weight(450) - 0.115).abs() < 1e-5);
    }

    /// The whole point of `resolve_family`: CoreText answers a request for a font it does
    /// not have with some other font, silently. We must not let it.
    #[test]
    fn a_missing_family_resolves_to_an_installed_fallback() {
        let e = TextEngine::new().unwrap();
        let got = e.resolve_family("NoSuchFontFamily12345");
        assert_ne!(got, "NoSuchFontFamily12345");
        assert!(
            family_exists(&got),
            "fell back to a font that is not installed"
        );
    }

    #[test]
    fn an_installed_family_resolves_to_itself() {
        let e = TextEngine::new().unwrap();
        assert_eq!(e.resolve_family("Menlo"), "Menlo");
    }

    #[test]
    fn at_least_one_fallback_is_installed() {
        assert!(
            FALLBACKS.iter().any(|f| family_exists(f)),
            "none of {FALLBACKS:?} is installed; the fallback chain is useless"
        );
    }
}
