//! System font family enumeration for the settings window's font picker.
//! Shared conceptually with the renderer, but kept separate so `settings` need not
//! depend on `render`.

use anyhow::{anyhow, Result};
use windows::core::{Interface, HSTRING};
use windows::Win32::Graphics::DirectWrite::*;

const FALLBACK: [&str; 2] = ["Consolas", "Segoe UI"];

/// Returns installed font family names, sorted and de-duplicated. On failure,
/// returns a small fallback list rather than erroring, so the picker always has options.
pub fn system_families() -> Result<Vec<String>> {
    match enumerate() {
        Ok(mut v) if !v.is_empty() => {
            v.sort();
            v.dedup();
            Ok(v)
        }
        other => {
            if let Err(e) = &other {
                tracing::warn!("font enumeration failed: {e:#}, using fallback");
            }
            Ok(FALLBACK.iter().map(|s| s.to_string()).collect())
        }
    }
}

fn enumerate() -> Result<Vec<String>> {
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let mut collection: Option<IDWriteFontCollection> = None;
        factory.GetSystemFontCollection(&mut collection, false)?;
        let collection = collection.ok_or_else(|| anyhow!("no system font collection"))?;

        let count = collection.GetFontFamilyCount();
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            let family = collection.GetFontFamily(i)?;
            let names = family.GetFamilyNames()?;
            // Prefer the user's locale, fall back to index 0.
            let mut index = 0u32;
            let mut exists = windows::core::BOOL(0);
            let locale = HSTRING::from("ko-kr");
            let _ = names.FindLocaleName(&locale, &mut index, &mut exists);
            if !exists.as_bool() {
                index = 0;
            }
            let len = names.GetStringLength(index)? as usize;
            let mut buf = vec![0u16; len + 1];
            names.GetString(index, &mut buf)?;
            let name = String::from_utf16_lossy(&buf[..len]);
            if !name.is_empty() {
                out.push(name);
            }
        }
        Ok(out)
    }
}

/// A font's file bytes plus the face index within the file (for `.ttc` collections).
pub struct FontFile {
    pub bytes: Vec<u8>,
    pub index: u32,
}

/// Loads the font file for a family name via DirectWrite, validated as parseable.
///
/// Returns `None` if the family is unknown, has no local file, or fails to parse (so a
/// broken font file never reaches egui, which panics on a parse failure). Never panics;
/// safe to call per visible dropdown row.
pub fn font_file(family: &str) -> Option<FontFile> {
    // SAFETY: every DirectWrite call result is checked with `.ok()?`/`?` before use;
    // no raw pointer is dereferenced outside the COM calls that produce/consume it.
    unsafe { font_file_inner(family) }
}

unsafe fn font_file_inner(family: &str) -> Option<FontFile> {
    let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
    let mut collection: Option<IDWriteFontCollection> = None;
    factory
        .GetSystemFontCollection(&mut collection, false)
        .ok()?;
    let collection = collection?;

    let mut index = 0u32;
    let mut exists = windows::core::BOOL(0);
    collection
        .FindFamilyName(&HSTRING::from(family), &mut index, &mut exists)
        .ok()?;
    if !exists.as_bool() {
        return None;
    }

    let fam = collection.GetFontFamily(index).ok()?;
    let font = fam.GetFont(0).ok()?;
    let font_face = font.CreateFontFace().ok()?;
    let face_index = font_face.GetIndex();

    // First call (fontfiles = None) just yields the file count.
    let mut file_count = 0u32;
    font_face.GetFiles(&mut file_count, None).ok()?;
    if file_count == 0 {
        return None;
    }
    let mut files: Vec<Option<IDWriteFontFile>> = vec![None; file_count as usize];
    font_face
        .GetFiles(&mut file_count, Some(files.as_mut_ptr()))
        .ok()?;
    let file = files.into_iter().next()??;

    // The reference key is an opaque buffer owned by `file`; valid only while it lives.
    let mut key_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut key_size = 0u32;
    file.GetReferenceKey(&mut key_ptr, &mut key_size).ok()?;

    let loader = file.GetLoader().ok()?;
    // Only local (on-disk) fonts have a file path; other loaders (e.g. downloadable
    // fonts) don't implement this interface, so the cast is how we detect and skip them.
    let local_loader: IDWriteLocalFontFileLoader = loader.cast().ok()?;

    let path_len = local_loader
        .GetFilePathLengthFromKey(key_ptr, key_size)
        .ok()?;
    let mut buf = vec![0u16; path_len as usize + 1];
    local_loader
        .GetFilePathFromKey(key_ptr, key_size, &mut buf)
        .ok()?;
    let nul = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let path = String::from_utf16_lossy(&buf[..nul]);
    if path.is_empty() {
        return None;
    }

    let bytes = std::fs::read(&path).ok()?;
    // Validate before returning: a corrupt or mismatched face index must not reach
    // egui/epaint, which panics on parse failure rather than erroring gracefully.
    skrifa::FontRef::from_index(&bytes, face_index).ok()?;

    Some(FontFile {
        bytes,
        index: face_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_a_nonempty_sorted_unique_list() {
        let fams = system_families().unwrap();
        assert!(!fams.is_empty(), "no font families enumerated");
        // No duplicates.
        let mut sorted = fams.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), fams.len(), "duplicate families in list");
        // Every Windows install has at least one common family.
        assert!(
            fams.iter()
                .any(|f| f == "Segoe UI" || f == "Consolas" || f == "Arial"),
            "expected a common family, got {} families",
            fams.len()
        );
    }

    #[test]
    fn font_file_loads_a_common_family() {
        // Arial ships on every Windows; its file must load and parse.
        let f = font_file("Arial").expect("Arial font file");
        assert!(!f.bytes.is_empty());
        // The bytes must be a font skrifa accepts (egui uses skrifa and panics otherwise).
        assert!(skrifa::FontRef::from_index(&f.bytes, f.index).is_ok());
    }

    #[test]
    fn font_file_returns_none_for_unknown_family() {
        assert!(font_file("NoSuchFamily12345XYZ").is_none());
    }

    #[test]
    fn every_enumerated_family_has_a_loadable_file_or_is_skipped() {
        // Not all fonts must load, but the ones that do must parse (no panic risk downstream).
        // Sample the first 20 to keep the test fast.
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
