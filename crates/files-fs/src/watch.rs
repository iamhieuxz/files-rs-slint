//! Directory change notifications.
//!
//! The original wraps `FileSystemWatcher` per navigated folder
//! (`WindowsFolderChangeWatcher`). This module exposes the same idea as a
//! channel: the app forwards changes to the frontend as a debounced
//! "this directory changed" signal rather than trying to mirror every event.

use files_core::FsError;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

/// A live watch. Dropping it stops delivery and releases the OS handle.
pub struct WatchHandle {
    _watcher: RecommendedWatcher,
    rx: Receiver<PathBuf>,
}

impl WatchHandle {
    /// Block until a change arrives or `timeout` elapses.
    ///
    /// Returns the directory that changed. The consumer decides whether to
    /// re-read the listing; coalescing several events into one refresh is the
    /// caller's job, because only it knows when a refresh is affordable.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<PathBuf> {
        self.rx.recv_timeout(timeout).ok()
    }

    pub fn try_recv(&self) -> Option<PathBuf> {
        self.rx.try_recv().ok()
    }
}

/// Watch `path` for entry-level changes.
pub fn watch_directory(path: &Path) -> Result<WatchHandle, FsError> {
    // The callback must be `'static`, so it owns its own copy of the path
    // rather than borrowing the caller's.
    let owned = path.to_path_buf();
    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            // Only structural changes matter for a listing. Access-time churn
            // would otherwise cause constant refreshes.
            use notify::EventKind;
            let relevant = matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_)
            );
            if relevant {
                let _ = tx.send(owned.clone());
            }
        }
    })
    .map_err(|e| FsError::new("watch_failed", e.to_string()))?;

    watcher
        .watch(path, RecursiveMode::NonRecursive)
        .map_err(|e| FsError::new("watch_failed", e.to_string()))?;

    Ok(WatchHandle { _watcher: watcher, rx })
}
