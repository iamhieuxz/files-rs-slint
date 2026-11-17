//! Navigation history for one tab.
//!
//! Kept separate from `main.rs` so the back/forward rules can be tested without
//! a window. The invariant is the browser one: navigating truncates the forward
//! branch, and a failed navigation must not leave a dead entry behind.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct History {
    entries: Vec<PathBuf>,
    cursor: usize,
}

impl History {
    pub fn new(start: PathBuf) -> Self {
        Self { entries: vec![start], cursor: 0 }
    }

    pub fn current(&self) -> &Path {
        &self.entries[self.cursor]
    }

    /// Record a new location, dropping the forward branch.
    pub fn push(&mut self, path: PathBuf) {
        self.entries.truncate(self.cursor + 1);
        if self.entries.last() == Some(&path) {
            // Re-entering the same directory should not stack duplicates.
            return;
        }
        self.entries.push(path);
        self.cursor = self.entries.len() - 1;
    }

    pub fn back(&mut self) -> Option<PathBuf> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        Some(self.entries[self.cursor].clone())
    }

    pub fn forward(&mut self) -> Option<PathBuf> {
        if self.cursor + 1 >= self.entries.len() {
            return None;
        }
        self.cursor += 1;
        Some(self.entries[self.cursor].clone())
    }

    pub fn can_back(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }

    /// Replace the current entry, used when a navigation fails after it was
    /// recorded: the history must not point at a directory the user never saw.
    pub fn replace_current(&mut self, path: PathBuf) {
        self.entries[self.cursor] = path;
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn back_and_forward_walk_the_stack() {
        let mut h = History::new(p("C:\\a"));
        h.push(p("C:\\b"));
        h.push(p("C:\\c"));
        assert_eq!(h.current(), p("C:\\c").as_path());
        assert_eq!(h.back(), Some(p("C:\\b")));
        assert_eq!(h.back(), Some(p("C:\\a")));
        assert_eq!(h.back(), None, "cannot walk past the first entry");
        assert_eq!(h.forward(), Some(p("C:\\b")));
    }

    #[test]
    fn navigating_after_back_drops_the_forward_branch() {
        let mut h = History::new(p("C:\\a"));
        h.push(p("C:\\b"));
        h.push(p("C:\\c"));
        h.back();
        h.push(p("C:\\d"));
        assert!(!h.can_forward(), "the old forward branch is gone");
        assert_eq!(h.current(), p("C:\\d").as_path());
    }

    #[test]
    fn pushing_the_current_path_does_not_duplicate_it() {
        let mut h = History::new(p("C:\\a"));
        h.push(p("C:\\a"));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn a_failed_navigation_can_be_rewritten() {
        let mut h = History::new(p("C:\\a"));
        h.push(p("C:\\missing"));
        // The load failed, so the tab goes back to where it came from.
        h.replace_current(p("C:\\a"));
        assert_eq!(h.current(), p("C:\\a").as_path());
    }
}
