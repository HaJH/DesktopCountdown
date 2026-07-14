//! System font family enumeration for the settings window's font picker.
//! Shared conceptually with the renderer, but kept separate so `settings` need not
//! depend on `render`.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr;

use anyhow::Result;
use objc2_core_foundation::{
    kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks, CFMutableDictionary,
    CFRetained, CFString, CFURLPathStyle, CFURL,
};
use objc2_core_text::{
    kCTFontFamilyNameAttribute, kCTFontURLAttribute, CTFont, CTFontDescriptor,
    CTFontManagerCopyAvailableFontFamilyNames,
};
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};

/// Enough of a picker to work with when enumeration fails outright.
const FALLBACK: [&str; 2] = ["Menlo", "Helvetica Neue"];

/// Installed font family names, sorted and de-duplicated. On failure, returns a small
/// fallback list rather than erroring, so the picker always has options.
pub fn system_families() -> Result<Vec<String>> {
    let mut v = enumerate();
    if v.is_empty() {
        tracing::warn!("font enumeration came back empty, using fallback");
        return Ok(FALLBACK.iter().map(|s| s.to_string()).collect());
    }
    v.sort();
    v.dedup();
    Ok(v)
}

fn enumerate() -> Vec<String> {
    // SAFETY: the API returns a CFArray of CFString.
    let names = unsafe { CTFontManagerCopyAvailableFontFamilyNames() };
    (0..names.count())
        .filter_map(|i| {
            let p = unsafe { names.value_at_index(i) } as *const CFString;
            // SAFETY: the array holds CFStrings and outlives this loop.
            unsafe { p.as_ref() }.map(|s| s.to_string())
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// A font's file bytes plus the face index within the file (for `.ttc` collections).
pub struct FontFile {
    pub bytes: Vec<u8>,
    pub index: u32,
}

/// Loads the font file for a family name via CoreText, validated as parseable.
///
/// Returns `None` if the family is unknown, has no local file, or fails to parse (so a
/// broken font file never reaches egui, which panics on a parse failure rather than
/// erroring gracefully). Never panics; safe to call per visible dropdown row.
pub fn font_file(family: &str) -> Option<FontFile> {
    // CoreText hands back *some* font for a family it does not have, so an existence check
    // has to come first or every unknown name would resolve to a real file.
    if !enumerate().iter().any(|f| f == family) {
        return None;
    }

    let path = font_path(family)?;
    let bytes = std::fs::read(&path).ok()?;
    let index = face_index(&bytes, family)?;
    Some(FontFile { bytes, index })
}

/// The on-disk file backing `family`, via the descriptor's `kCTFontURLAttribute`.
fn font_path(family: &str) -> Option<PathBuf> {
    let font = font_for_family(family)?;

    // SAFETY: `kCTFontURLAttribute` is documented to yield a CFURL, or nothing for a font
    // with no local file (a downloadable one, say).
    let url = unsafe { font.attribute(kCTFontURLAttribute) }?;
    let url = url.downcast_ref::<CFURL>()?;
    let path = url.file_system_path(CFURLPathStyle::CFURLPOSIXPathStyle)?;
    let path = PathBuf::from(path.to_string());
    if path.as_os_str().is_empty() {
        return None;
    }
    Some(path)
}

fn font_for_family(family: &str) -> Option<CFRetained<CTFont>> {
    // SAFETY: CoreFoundation's own callback tables; a documented key with a CFString value.
    let attrs = unsafe {
        let attrs = CFMutableDictionary::new(
            None,
            0,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )?;
        let name = CFString::from_str(family);
        CFMutableDictionary::set_value(
            Some(&attrs),
            kCTFontFamilyNameAttribute as *const CFString as *const c_void,
            &*name as *const CFString as *const c_void,
        );
        attrs
    };

    // SAFETY: `attrs` is a CFDictionary of descriptor attributes; a null matrix is the
    // identity. The size is irrelevant -- we only want the file behind the face.
    let descriptor = unsafe { CTFontDescriptor::with_attributes(&attrs) };
    Some(unsafe { CTFont::with_font_descriptor(&descriptor, 12.0, ptr::null()) })
}

/// Which face inside the file is `family`.
///
/// Most macOS system fonts live in `.ttc` collections -- Menlo, Helvetica and friends all
/// share a file with their siblings -- and CoreText, unlike DirectWrite, never tells us
/// which face it picked. Handing egui index 0 would draw "Menlo" in whatever face happens
/// to come first in `Menlo.ttc`. So find it: walk the collection and take the face whose
/// own family name matches.
///
/// Also the parse check. A face that skrifa cannot read is one epaint would panic on.
fn face_index(bytes: &[u8], family: &str) -> Option<u32> {
    // A single-face file is index 0 and needs no search -- but still needs the parse check.
    let mut fallback = None;

    for i in 0..MAX_FACES {
        let Ok(font) = FontRef::from_index(bytes, i) else {
            break; // past the end of the collection
        };
        if fallback.is_none() {
            fallback = Some(i);
        }
        // A .ttc face names itself with the same family string CoreText enumerated.
        for id in [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME] {
            if font
                .localized_strings(id)
                .any(|s| s.to_string().eq_ignore_ascii_case(family))
            {
                return Some(i);
            }
        }
    }

    // No face claimed the name (a font whose CoreText family name differs from its `name`
    // table, which happens). The first parseable face is still better than nothing, and it
    // has been parse-checked.
    fallback
}

/// No real collection is anywhere near this large; the bound just keeps a corrupt file
/// from spinning us.
const MAX_FACES: u32 = 64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_a_nonempty_sorted_unique_list() {
        let fams = system_families().unwrap();
        assert!(!fams.is_empty(), "no font families enumerated");
        let mut sorted = fams.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), fams.len(), "duplicate families in list");
        // Every macOS install has these.
        assert!(
            fams.iter().any(|f| f == "Menlo" || f == "Helvetica"),
            "expected a common family, got {} families",
            fams.len()
        );
    }

    #[test]
    fn font_file_loads_a_common_family() {
        let f = font_file("Menlo").expect("Menlo font file");
        assert!(!f.bytes.is_empty());
        // The bytes must be a font skrifa accepts (egui uses skrifa and panics otherwise).
        assert!(skrifa::FontRef::from_index(&f.bytes, f.index).is_ok());
    }

    /// The reason `face_index` exists: Menlo ships as a `.ttc`, and index 0 is not
    /// necessarily the face called "Menlo".
    #[test]
    fn the_face_index_points_at_the_family_that_was_asked_for() {
        let f = font_file("Menlo").expect("Menlo font file");
        let font = FontRef::from_index(&f.bytes, f.index).unwrap();
        let names: Vec<String> = font
            .localized_strings(StringId::FAMILY_NAME)
            .map(|s| s.to_string())
            .collect();
        assert!(
            names.iter().any(|n| n.eq_ignore_ascii_case("Menlo")),
            "face {} of the file is {names:?}, not Menlo",
            f.index
        );
    }

    /// CoreText substitutes silently, so this would happily return some unrelated font's
    /// file if `font_file` did not check the family list first.
    #[test]
    fn font_file_returns_none_for_unknown_family() {
        assert!(font_file("NoSuchFamily12345XYZ").is_none());
    }

    #[test]
    fn every_enumerated_family_has_a_loadable_file_or_is_skipped() {
        // Not all fonts must load, but the ones that do must parse (no panic risk
        // downstream). Sample the first 20 to keep the test fast.
        for name in system_families().unwrap().into_iter().take(20) {
            if let Some(f) = font_file(&name) {
                assert!(
                    skrifa::FontRef::from_index(&f.bytes, f.index).is_ok(),
                    "{name} parsed-check"
                );
            }
        }
    }
}
