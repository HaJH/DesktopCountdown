//! Registers the executable under HKCU\...\Run so it survives a reboot.
//!
//! The Run key is not something Windows guarantees to exist: it is created the first time
//! something registers a startup entry, so a profile that never had one does not have the
//! key at all (fresh installs, CI runner images). `RegOpenKeyExW` does not create it, so
//! every path below has to cope with it being absent -- `enable` creates it, and the
//! read/disable paths treat "no key" as "nothing is registered".

use anyhow::{Context, Result};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const VALUE_NAME: PCWSTR = w!("DesktopCountdown");

/// `Ok(None)` when `run_key` does not exist. Every other failure is an error.
fn open(
    run_key: PCWSTR,
    access: windows::Win32::System::Registry::REG_SAM_FLAGS,
) -> Result<Option<HKEY>> {
    let mut key = HKEY::default();
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, run_key, Some(0), access, &mut key) };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok()?;
    Ok(Some(key))
}

/// Opens `run_key`, creating it if it does not exist. Only the enable path needs this:
/// creating the key merely to read from it, or to delete a value that cannot be there,
/// would write to the registry for no reason.
fn open_or_create(
    run_key: PCWSTR,
    access: windows::Win32::System::Registry::REG_SAM_FLAGS,
) -> Result<HKEY> {
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            run_key,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            access,
            None,
            &mut key,
            None,
        )
        .ok()?
    };
    Ok(key)
}

/// Is the `DesktopCountdown` value present under the Run key?
pub fn is_enabled() -> Result<bool> {
    is_enabled_in(RUN_KEY)
}

/// Writes the quoted current-exe path as a `REG_SZ` value when `on`, deletes the
/// value otherwise. Deleting an absent value is success.
pub fn set_enabled(on: bool) -> Result<()> {
    set_enabled_in(RUN_KEY, on)
}

/// The body of `is_enabled`, against an arbitrary key so the tests can exercise the
/// key-does-not-exist path without touching the real Run key.
fn is_enabled_in(run_key: PCWSTR) -> Result<bool> {
    // No Run key means no startup entry -- ours included.
    let Some(key) = open(run_key, KEY_READ)? else {
        return Ok(false);
    };
    let mut size = 0u32;
    let status = unsafe { RegQueryValueExW(key, VALUE_NAME, None, None, None, Some(&mut size)) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    Ok(status.is_ok())
}

/// The body of `set_enabled`. See `is_enabled_in` for why it takes the key.
fn set_enabled_in(run_key: PCWSTR, on: bool) -> Result<()> {
    if !on {
        // Nothing to delete from a key that is not there.
        let Some(key) = open(run_key, KEY_WRITE)? else {
            return Ok(());
        };
        let status = unsafe { RegDeleteValueW(key, VALUE_NAME) };
        unsafe {
            let _ = RegCloseKey(key);
        }
        // Deleting an absent value is success from the caller's point of view.
        if status != ERROR_FILE_NOT_FOUND {
            status.ok().context("updating the Run key")?;
        }
        return Ok(());
    }

    // Built before the key is opened: an early return from here must not leak the handle.
    //
    // REG_SZ data must include the terminating NUL. `HSTRING` derefs to `[u16]`
    // (windows-strings 0.5, used by windows 0.62; this crate version has no `as_wide()`
    // method), and that slice does not include the trailing NUL, so pushing a `0u16`
    // before measuring the byte length avoids reading past the end of `wide`.
    let exe = std::env::current_exe().context("current_exe")?;
    let quoted = format!("\"{}\"", exe.display());
    let mut wide: Vec<u16> = HSTRING::from(quoted).to_vec();
    wide.push(0);
    let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };

    let key = open_or_create(run_key, KEY_WRITE | KEY_READ)?;
    let result = unsafe { RegSetValueExW(key, VALUE_NAME, Some(0), REG_SZ, Some(bytes)) }.ok();
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
    use windows::Win32::System::Registry::RegDeleteKeyW;

    /// The tests mutate registry state; serialise them.
    static REGISTRY: Mutex<()> = Mutex::new(());

    /// A key of our own, so the absent-key tests can delete it outright. Deleting the real
    /// Run key would wipe every startup entry on the developer's machine.
    const SCRATCH_KEY: PCWSTR = w!(r"Software\DesktopCountdown\AutostartTest");

    fn delete_scratch_key() {
        unsafe {
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, SCRATCH_KEY);
        }
    }

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

    /// A profile that never registered a startup entry has no Run key at all, and
    /// `RegOpenKeyExW` does not create one. Reading must report "not enabled" rather than
    /// failing -- the whole key being missing is exactly the state in which nothing is
    /// registered.
    #[test]
    fn a_missing_run_key_reads_as_not_enabled() {
        let _lock = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        delete_scratch_key();

        assert!(!is_enabled_in(SCRATCH_KEY).unwrap());
    }

    /// Nor may disabling fail: there is nothing to delete, which is the state the caller
    /// asked for.
    #[test]
    fn disabling_with_no_run_key_is_not_an_error() {
        let _lock = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        delete_scratch_key();

        assert!(set_enabled_in(SCRATCH_KEY, false).is_ok());
        // And it did not create the key just to delete nothing from it.
        assert!(open(SCRATCH_KEY, KEY_READ).unwrap().is_none());
    }

    /// Enabling has to create the key, since nothing else will.
    #[test]
    fn enabling_creates_a_missing_run_key() {
        let _lock = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        delete_scratch_key();

        set_enabled_in(SCRATCH_KEY, true).unwrap();
        assert!(is_enabled_in(SCRATCH_KEY).unwrap());

        set_enabled_in(SCRATCH_KEY, false).unwrap();
        assert!(!is_enabled_in(SCRATCH_KEY).unwrap());

        delete_scratch_key();
    }
}
