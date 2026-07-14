//! Named-mutex single instance guard, scoped to the current session.

use anyhow::{bail, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

pub struct SingleInstance(HANDLE);

impl SingleInstance {
    /// Takes the `Local\{name}` mutex. Returns `Err` if another process already holds it.
    ///
    /// The renderer and the settings window pass different names, so the two never
    /// contend with each other.
    pub fn acquire(name: &str) -> Result<Self> {
        // Owned, NUL-terminated: `w!` only takes a literal, and the name is a parameter now.
        let wide: Vec<u16> = format!("Local\\{name}\0").encode_utf16().collect();
        unsafe {
            let handle = CreateMutexW(None, true, PCWSTR(wide.as_ptr()))?;
            if windows::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                bail!("another instance is already running");
            }
            Ok(Self(handle))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name of this test's own. The old test took the production mutex, so it failed
    /// whenever `desktop-countdown.exe` happened to be running on the developer's machine.
    const TEST_NAME: &str = "DesktopCountdown-Test-SingleInstance";

    #[test]
    fn second_acquire_fails_while_the_first_is_held() {
        let first = SingleInstance::acquire(TEST_NAME).unwrap();

        assert!(SingleInstance::acquire(TEST_NAME).is_err());

        drop(first);

        assert!(SingleInstance::acquire(TEST_NAME).is_ok());
    }

    /// What the name parameter is for: the renderer's lock must not block the settings
    /// window's, and vice versa.
    #[test]
    fn different_names_do_not_contend() {
        let _renderer = SingleInstance::acquire("DesktopCountdown-Test-A").unwrap();
        assert!(SingleInstance::acquire("DesktopCountdown-Test-B").is_ok());
    }
}
