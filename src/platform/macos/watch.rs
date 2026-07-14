//! Watches `config.toml` and wakes the run loop when it changes.
//!
//! Same division of labour as the Windows watcher: this file does not debounce and does not
//! queue, it just says "something happened" the moment the filesystem does. Debouncing is
//! the run loop's job (`mod.rs` restarts a short one-shot `NSTimer` per wake-up), because
//! the alternative -- a flag the loop has to poll -- can only notice a change as often as
//! the loop happens to ask, and the loop only ticks once a second.
//!
//! Where Windows does `PostMessageW`, this does `dispatch_async` onto the main queue.
//! Notify's callback runs on a thread of its own, and the run-loop state it needs to poke
//! lives on the main thread and is not `Send`; the dispatched block runs *on* the main
//! thread, where it can reach that state through `super::wake_for_config_change`.

use std::path::Path;

use anyhow::Result;
use dispatch2::DispatchQueue;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
}

impl ConfigWatcher {
    /// Starts watching the directory holding `path`.
    pub fn new(path: &Path) -> Result<Self> {
        let file_name = path.file_name().map(|s| s.to_owned());

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
                // Hops to the main thread. Like `PostMessageW`, this only enqueues and returns,
                // so it cannot deadlock against the run loop the way a synchronous call would.
                DispatchQueue::main().exec_async(super::wake_for_config_change);
            })?;

        // Watch the directory, not the file: editors replace the inode on save.
        let dir = path.parent().unwrap_or(Path::new("."));
        watcher.watch(dir, RecursiveMode::NonRecursive)?;

        Ok(Self { _watcher: watcher })
    }
}
