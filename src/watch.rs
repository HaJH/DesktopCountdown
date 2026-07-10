//! Watches `config.toml` and reports debounced changes.

use std::path::Path;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// Editors often write a file in several steps; wait for the writes to settle.
fn should_fire(pending_since: Option<Instant>, now: Instant, debounce: Duration) -> bool {
    match pending_since {
        Some(t) => now.duration_since(t) >= debounce,
        None => false,
    }
}

pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<()>,
    pending_since: Option<Instant>,
}

impl ConfigWatcher {
    pub fn new(path: &Path) -> Result<Self> {
        let (tx, rx) = channel();
        let file_name = path.file_name().map(|s| s.to_owned());

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            let touches_config =
                event.paths.iter().any(|p| p.file_name().map(|s| s.to_owned()) == file_name);
            if touches_config {
                let _ = tx.send(());
            }
        })?;

        // Watch the directory, not the file: editors replace the inode on save.
        let dir = path.parent().unwrap_or(Path::new("."));
        watcher.watch(dir, RecursiveMode::NonRecursive)?;

        Ok(Self { _watcher: watcher, rx, pending_since: None })
    }

    /// Call this every tick. Returns `true` exactly once per settled change.
    pub fn changed(&mut self) -> bool {
        loop {
            match self.rx.try_recv() {
                Ok(()) => self.pending_since = Some(Instant::now()),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        if should_fire(self.pending_since, Instant::now(), DEBOUNCE) {
            self.pending_since = None;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn pending_change_is_not_reported_before_the_debounce_elapses() {
        let now = Instant::now();
        assert!(!should_fire(Some(now), now, DEBOUNCE));
    }

    #[test]
    fn pending_change_fires_after_the_debounce() {
        let now = Instant::now();
        let later = now + DEBOUNCE + Duration::from_millis(1);
        assert!(should_fire(Some(now), later, DEBOUNCE));
    }

    #[test]
    fn no_pending_change_never_fires() {
        assert!(!should_fire(None, Instant::now(), DEBOUNCE));
    }
}
