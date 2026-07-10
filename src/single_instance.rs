//! Named-mutex single instance guard, scoped to the current session.

use anyhow::{bail, Result};
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

pub struct SingleInstance(HANDLE);

impl SingleInstance {
    /// Returns `Err` if another instance already holds the mutex.
    pub fn acquire() -> Result<Self> {
        unsafe {
            let handle = CreateMutexW(None, true, w!("Local\\DesktopCountdown"))?;
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

    /// Uses the production mutex name, so this test will fail if
    /// `desktop-countdown.exe` is already running on the developer's machine.
    #[test]
    fn second_acquire_fails_while_the_first_is_held() {
        let first = SingleInstance::acquire().unwrap();

        assert!(SingleInstance::acquire().is_err());

        drop(first);

        assert!(SingleInstance::acquire().is_ok());
    }
}
