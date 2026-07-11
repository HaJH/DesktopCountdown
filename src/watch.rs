//! Watches `config.toml` and wakes the renderer's message loop when it changes.
//!
//! The watcher does not debounce and does not queue: it posts a message the moment the
//! filesystem reports a write. Debouncing is the receiver's job (`app` restarts a short
//! Win32 timer per message), because the alternative -- a `changed()` the message loop
//! has to poll -- can only notice a change as often as the loop happens to ask, and the
//! renderer only ticks once a second.

use std::path::Path;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
    /// The window to post to, as a raw `isize` so the watcher thread can read it: `HWND`
    /// is not `Send`, but posting to one from another thread is what `PostMessageW` is for.
    /// `0` until `notify_window` runs.
    target: Arc<AtomicIsize>,
}

impl ConfigWatcher {
    /// Starts watching the directory holding `path`. Every filesystem event touching the
    /// file posts `msg` to the window last given to `notify_window`.
    pub fn new(path: &Path, msg: u32) -> Result<Self> {
        let target = Arc::new(AtomicIsize::new(0));
        let file_name = path.file_name().map(|s| s.to_owned());
        let post_to = Arc::clone(&target);

        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                let Ok(event) = res else { return };
                let touches_config = event
                    .paths
                    .iter()
                    .any(|p| p.file_name().map(|s| s.to_owned()) == file_name);
                if !touches_config {
                    return;
                }
                let hwnd = post_to.load(Ordering::Acquire);
                if hwnd == 0 {
                    return; // no window listening yet
                }
                // Runs on notify's own thread. `PostMessageW` only appends to the target
                // thread's queue and returns, so it cannot deadlock against the message
                // loop the way a `SendMessage` would.
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd as *mut _)), msg, WPARAM(0), LPARAM(0));
                }
            })?;

        // Watch the directory, not the file: editors replace the inode on save.
        let dir = path.parent().unwrap_or(Path::new("."));
        watcher.watch(dir, RecursiveMode::NonRecursive)?;

        Ok(Self {
            _watcher: watcher,
            target,
        })
    }

    /// Directs the watcher at the window that should be woken. Events arriving before this
    /// are dropped: the only window that exists that early is none, and the config was just
    /// read at startup anyway.
    pub fn notify_window(&self, hwnd: HWND) {
        self.target.store(hwnd.0 as isize, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, WINDOW_EX_STYLE};
    use windows::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, PeekMessageW, RegisterClassW, MSG, PM_REMOVE, WM_APP, WNDCLASSW, WS_POPUP,
    };

    const TEST_MSG: u32 = WM_APP + 7;

    unsafe extern "system" fn test_wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        DefWindowProcW(h, m, w, l)
    }

    fn hidden_window() -> HWND {
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let wc = WNDCLASSW {
                lpfnWndProc: Some(test_wndproc),
                hInstance: hinst.into(),
                lpszClassName: w!("ConfigWatcherTestWindow"),
                ..Default::default()
            };
            RegisterClassW(&wc);
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("ConfigWatcherTestWindow"),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .unwrap()
        }
    }

    /// Pumps this thread's queue until `TEST_MSG` shows up or `timeout` expires.
    fn wait_for_post(timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                if msg.message == TEST_MSG {
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// The whole point of the watcher: a write to the config file must reach the message
    /// loop on its own, without the loop polling for it. Before this was event-driven, a
    /// change could sit unnoticed until the next 1 Hz tick (and then a second one, for the
    /// debounce) -- the settings window's edits took seconds to show up.
    #[test]
    fn writing_the_config_posts_to_the_window() {
        let mut dir = std::env::temp_dir();
        dir.push("dc-test-watch");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "target = \"2030-01-01T00:00:00\"\n").unwrap();

        let hwnd = hidden_window();
        let watcher = ConfigWatcher::new(&path, TEST_MSG).unwrap();
        watcher.notify_window(hwnd);

        std::fs::write(&path, "target = \"2031-01-01T00:00:00\"\n").unwrap();

        assert!(
            wait_for_post(Duration::from_secs(5)),
            "a config write did not wake the message loop"
        );
    }

    /// A write to some other file in the same directory must not wake anyone: the watcher
    /// watches the directory (editors replace the file's inode on save), so it sees its
    /// neighbours' writes too -- including our own `config.toml.tmp`, written on every save.
    #[test]
    fn writing_an_unrelated_file_posts_nothing() {
        let mut dir = std::env::temp_dir();
        dir.push("dc-test-watch-unrelated");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "target = \"2030-01-01T00:00:00\"\n").unwrap();

        let hwnd = hidden_window();
        let watcher = ConfigWatcher::new(&path, TEST_MSG).unwrap();
        watcher.notify_window(hwnd);

        std::fs::write(dir.join("notes.txt"), "hello").unwrap();

        assert!(
            !wait_for_post(Duration::from_millis(700)),
            "an unrelated file's write woke the message loop"
        );
    }
}
