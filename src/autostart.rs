//! Registers the executable under HKCU\...\Run so it survives a reboot.

use anyhow::{Context, Result};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const VALUE_NAME: PCWSTR = w!("DesktopCountdown");

fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Result<HKEY> {
    let mut key = HKEY::default();
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, Some(0), access, &mut key).ok()? };
    Ok(key)
}

/// Is the `DesktopCountdown` value present under the Run key?
pub fn is_enabled() -> Result<bool> {
    let key = open(KEY_READ)?;
    let mut size = 0u32;
    let status = unsafe { RegQueryValueExW(key, VALUE_NAME, None, None, None, Some(&mut size)) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    Ok(status.is_ok())
}

/// Writes the quoted current-exe path as a `REG_SZ` value when `on`, deletes the
/// value otherwise. Deleting an absent value is success.
pub fn set_enabled(on: bool) -> Result<()> {
    let key = open(KEY_WRITE | KEY_READ)?;
    let result = if on {
        let exe = std::env::current_exe().context("current_exe")?;
        let quoted = format!("\"{}\"", exe.display());
        // REG_SZ data must include the terminating NUL. `HSTRING` derefs to `[u16]`
        // (windows-strings 0.5, used by windows 0.62; this crate version has no
        // `as_wide()` method), and that slice does not include the trailing NUL, so
        // pushing a `0u16` before measuring the byte length avoids reading past the
        // end of `wide`.
        let mut wide: Vec<u16> = HSTRING::from(quoted).to_vec();
        wide.push(0);
        let bytes =
            unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
        unsafe { RegSetValueExW(key, VALUE_NAME, Some(0), REG_SZ, Some(bytes)) }.ok()
    } else {
        let status = unsafe { RegDeleteValueW(key, VALUE_NAME) };
        // Deleting an absent value is success from the caller's point of view.
        if status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            status.ok()
        }
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    result.context("updating the Run key")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Both tests mutate the same registry value; serialise them.
    static REGISTRY: Mutex<()> = Mutex::new(());

    #[test]
    fn enable_then_disable_round_trips() {
        let _lock = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        let original = is_enabled().unwrap();

        set_enabled(true).unwrap();
        assert!(is_enabled().unwrap());

        set_enabled(false).unwrap();
        assert!(!is_enabled().unwrap());

        set_enabled(original).unwrap();
        assert_eq!(is_enabled().unwrap(), original);
    }

    #[test]
    fn disabling_when_absent_is_not_an_error() {
        let _lock = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        let original = is_enabled().unwrap();

        set_enabled(false).unwrap();
        assert!(set_enabled(false).is_ok());

        set_enabled(original).unwrap();
    }
}
