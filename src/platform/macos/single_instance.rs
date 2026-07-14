//! Lock-file single instance guard.
//!
//! `flock(LOCK_EX | LOCK_NB)` where Windows uses a named mutex. The kernel drops the lock
//! when the process dies, however it dies, so there is no stale-lock problem to solve --
//! the file may be left behind, but an abandoned file with no lock on it is just a file.

use std::fs::{File, OpenOptions};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};

pub struct SingleInstance {
    /// Holds the `flock`. Closing the file releases it, so this must outlive the app.
    _file: File,
}

impl SingleInstance {
    /// Takes an exclusive lock on `~/Library/Application Support/DesktopCountdown/{name}.lock`.
    /// Returns `Err` if another process already holds it.
    ///
    /// The renderer and the settings window pass different names, so the two never contend
    /// with each other.
    pub fn acquire(name: &str) -> Result<Self> {
        let path = lock_path(name)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("opening the lock file {}", path.display()))?;

        // SAFETY: `file` owns the descriptor and outlives the call.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if locked != 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                bail!("another instance is already running");
            }
            return Err(err).with_context(|| format!("locking {}", path.display()));
        }

        Ok(Self { _file: file })
    }
}

fn lock_path(name: &str) -> Result<PathBuf> {
    // Next to the config, which is a directory this app already creates and owns. `/tmp`
    // would be wrong: it is world-writable, so another user could squat the name.
    let mut p = crate::paths::config_path()?
        .parent()
        .ok_or_else(|| anyhow!("the config path has no parent directory"))?
        .to_path_buf();
    p.push(format!("{name}.lock"));
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Names of the tests' own, so a `DesktopCountdown.app` already running on the
    /// developer's machine (holding the production lock) does not fail them.
    const TEST_NAME: &str = "DesktopCountdown-Test-SingleInstance";

    /// The lock file lives in the real config directory, which is the user's -- so a test
    /// that leaves one behind litters the machine of everyone who ever runs `cargo test`.
    /// Sweeping up in the test itself is safe: the lock is held by an open descriptor, not
    /// by the directory entry, so unlinking it does not release anything.
    fn sweep(name: &str) {
        if let Ok(p) = lock_path(name) {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn second_acquire_fails_while_the_first_is_held() {
        let first = SingleInstance::acquire(TEST_NAME).unwrap();

        assert!(SingleInstance::acquire(TEST_NAME).is_err());

        drop(first);

        assert!(
            SingleInstance::acquire(TEST_NAME).is_ok(),
            "the lock was not released when the guard was dropped"
        );
        sweep(TEST_NAME);
    }

    /// What the name parameter is for: the renderer's lock must not block the settings
    /// window's, and vice versa.
    #[test]
    fn different_names_do_not_contend() {
        let _renderer = SingleInstance::acquire("DesktopCountdown-Test-A").unwrap();
        assert!(SingleInstance::acquire("DesktopCountdown-Test-B").is_ok());
        sweep("DesktopCountdown-Test-A");
        sweep("DesktopCountdown-Test-B");
    }
}
