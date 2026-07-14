//! Named-mutex single instance guard, scoped to the current session.

use anyhow::Result;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

pub struct SingleInstance(HANDLE);

impl SingleInstance {
    /// Takes the `Local\{name}` mutex. `Ok(None)` means another process already holds
    /// it -- an expected outcome the caller decides what to do with, not an error.
    /// `Err` is a real failure to create the mutex.
    ///
    /// The renderer and the settings window pass different names, so the two never
    /// contend with each other.
    pub fn acquire(name: &str) -> Result<Option<Self>> {
        // Owned, NUL-terminated: `w!` only takes a literal, and the name is a parameter now.
        let wide: Vec<u16> = format!("Local\\{name}\0").encode_utf16().collect();
        unsafe {
            let handle = CreateMutexW(None, true, PCWSTR(wide.as_ptr()))?;
            if windows::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return Ok(None);
            }
            Ok(Some(Self(handle)))
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
    fn second_acquire_reports_already_running() {
        let first = SingleInstance::acquire(TEST_NAME).unwrap().unwrap();

        assert!(SingleInstance::acquire(TEST_NAME).unwrap().is_none());

        drop(first);

        assert!(SingleInstance::acquire(TEST_NAME).unwrap().is_some());
    }

    /// What the name parameter is for: the renderer's lock must not block the settings
    /// window's, and vice versa.
    #[test]
    fn different_names_do_not_contend() {
        let _renderer = SingleInstance::acquire("DesktopCountdown-Test-A")
            .unwrap()
            .unwrap();
        assert!(SingleInstance::acquire("DesktopCountdown-Test-B")
            .unwrap()
            .is_some());
    }
}
