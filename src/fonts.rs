//! System font family enumeration for the settings window's font picker.
//! Shared conceptually with the renderer, but kept separate so `settings` need not
//! depend on `render`.

use anyhow::{anyhow, Result};
use windows::core::HSTRING;
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
}
