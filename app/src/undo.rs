//! Undo and redo for file operations.
//!
//! Every operation that can be reversed records *how* to reverse it, not just
//! that it happened: which path was renamed to which, which files a paste
//! created, which sources a move took away. Undo then applies the inverse and
//! pushes the same record onto the redo stack.
//!
//! Two operations are deliberately **not** undoable: permanent delete (the data
//! is gone) and compress/extract (they create a file the user can simply delete).
//! The delete dialog says so out loud rather than offering a button that lies.

use files_core::FsError;
use std::path::{Path, PathBuf};

/// How to reverse one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoAction {
    /// `from` was renamed to `to`.
    Rename { from: PathBuf, to: PathBuf },
    /// A folder or file was created at `path`.
    Created { path: PathBuf, is_dir: bool },
    /// A paste copied `sources` into `target`, producing `created`.
    Copied {
        sources: Vec<PathBuf>,
        created: Vec<PathBuf>,
        target: PathBuf,
    },
    /// A paste moved `(from, to)` pairs.
    Moved { pairs: Vec<(PathBuf, PathBuf)> },
}

impl UndoAction {
    /// What the status bar says while undoing.
    pub fn undo_label(&self) -> String {
        match self {
            Self::Rename { to, .. } => format!("Đã hoàn tác đổi tên {}", name_of(to)),
            Self::Created { path, .. } => format!("Đã hoàn tác tạo {}", name_of(path)),
            Self::Copied { created, .. } => {
                format!("Đã hoàn tác sao chép {} mục", created.len())
            }
            Self::Moved { pairs } => format!("Đã hoàn tác di chuyển {} mục", pairs.len()),
        }
    }

    /// What the status bar says while redoing.
    pub fn redo_label(&self) -> String {
        match self {
            Self::Rename { to, .. } => format!("Đã làm lại đổi tên {}", name_of(to)),
            Self::Created { path, .. } => format!("Đã làm lại tạo {}", name_of(path)),
            Self::Copied { created, .. } => {
                format!("Đã làm lại sao chép {} mục", created.len())
            }
            Self::Moved { pairs } => format!("Đã làm lại di chuyển {} mục", pairs.len()),
        }
    }
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}

/// Bounded undo history.
///
/// Pushing a new action clears the redo branch, which is what every editor does:
/// once the user changes direction, the old future is no longer reachable.
pub struct UndoStack {
    done: Vec<UndoAction>,
    undone: Vec<UndoAction>,
    capacity: usize,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::with_capacity(50)
    }
}

impl UndoStack {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            done: Vec::new(),
            undone: Vec::new(),
            // A zero-capacity history would silently drop everything; one entry
            // is the smallest useful value.
            capacity: capacity.max(1),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn record(&mut self, action: UndoAction) {
        self.undone.clear();
        self.done.push(action);
        // Drop the oldest entries first: recent history is the useful history.
        if self.done.len() > self.capacity {
            let excess = self.done.len() - self.capacity;
            self.done.drain(0..excess);
        }
    }

    /// Take the action to undo. The caller pushes it back with [`Self::undone`]
    /// only after the inverse actually succeeded.
    pub fn take_undo(&mut self) -> Option<UndoAction> {
        self.done.pop()
    }

    /// Take the action to redo.
    pub fn take_redo(&mut self) -> Option<UndoAction> {
        self.undone.pop()
    }

    /// Put an action back after a successful undo.
    pub fn undone(&mut self, action: UndoAction) {
        self.undone.push(action);
    }

    /// Put an action back after a successful redo.
    pub fn redone(&mut self, action: UndoAction) {
        self.done.push(action);
    }
}

/// Apply the inverse of `action`.
pub fn undo(action: &UndoAction) -> Result<(), FsError> {
    match action {
        UndoAction::Rename { from, to } => {
            // `rename` giữ nguyên thư mục, chỉ đổi tên, nên chỉ cần tên cũ.
            files_fs::rename(to, &name_of(from)).map(|_| ())
        }
        UndoAction::Created { path, .. } => {
            // Something we created can be removed. Recycle rather than destroy:
            // the user may have put work into it between create and undo.
            files_fs::delete_to_recycle_bin(std::slice::from_ref(path)).map(|_| ())
        }
        UndoAction::Copied { created, .. } => files_fs::delete_to_recycle_bin(created).map(|_| ()),
        UndoAction::Moved { pairs } => {
            for (from, to) in pairs {
                let parent = from.parent().unwrap_or(Path::new("."));
                files_fs::move_into(to, parent)?;
            }
            Ok(())
        }
    }
}

/// Apply `action` again.
pub fn redo(action: &UndoAction) -> Result<(), FsError> {
    match action {
        UndoAction::Rename { from, to } => files_fs::rename(from, &name_of(to)).map(|_| ()),
        UndoAction::Created { path, is_dir } => {
            // Đúng đường dẫn đã ghi, không qua `create_file` (nó thêm `.txt` và
            // đổi sang "tên (2)" khi trùng, làm redo tạo ra thứ khác).
            if *is_dir {
                std::fs::create_dir(path).map_err(FsError::from)
            } else {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map(|_| ())
                    .map_err(FsError::from)
            }
        }
        UndoAction::Copied {
            sources, target, ..
        } => {
            for src in sources {
                files_fs::copy_into(src, target)?;
            }
            Ok(())
        }
        UndoAction::Moved { pairs } => {
            for (from, to) in pairs {
                files_fs::move_into(from, to.parent().unwrap_or(Path::new(".")))?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(n: &str) -> UndoAction {
        UndoAction::Created {
            path: PathBuf::from(format!("C:\\x\\{n}")),
            is_dir: true,
        }
    }

    #[test]
    fn a_fresh_stack_has_nothing_to_undo_or_redo() {
        let s = UndoStack::default();
        assert!(!s.can_undo());
        assert!(!s.can_redo());
    }

    #[test]
    fn recording_makes_undo_available() {
        let mut s = UndoStack::default();
        s.record(action("a"));
        assert!(s.can_undo());
        assert!(!s.can_redo(), "chưa hoàn tác thì chưa có gì để làm lại");
    }

    #[test]
    fn undo_and_redo_move_the_same_action_back_and_forth() {
        let mut s = UndoStack::default();
        s.record(action("a"));

        let a = s.take_undo().expect("phải có để hoàn tác");
        s.undone(a);
        assert!(!s.can_undo(), "đã lấy ra thì ngăn xếp undo rỗng");
        assert!(s.can_redo());

        let b = s.take_redo().expect("phải có để làm lại");
        s.redone(b);
        assert!(s.can_undo());
        assert!(!s.can_redo());
    }

    #[test]
    fn recording_a_new_action_clears_the_redo_branch() {
        // Đổi hướng thì tương lai cũ không còn với tới được nữa.
        let mut s = UndoStack::default();
        s.record(action("a"));
        let a = s.take_undo().unwrap();
        s.undone(a);
        assert!(s.can_redo());

        s.record(action("b"));
        assert!(!s.can_redo(), "nhánh làm lại phải bị xoá");
    }

    #[test]
    fn the_history_stops_growing_at_capacity() {
        let mut s = UndoStack::with_capacity(3);
        for n in 0..10 {
            s.record(action(&n.to_string()));
        }
        let mut seen = 0;
        while s.take_undo().is_some() {
            seen += 1;
        }
        assert_eq!(seen, 3, "chỉ giữ 3 mục gần nhất");
    }

    #[test]
    fn the_oldest_entries_are_the_ones_dropped() {
        let mut s = UndoStack::with_capacity(2);
        s.record(action("cu"));
        s.record(action("moi"));
        s.record(action("moi-nhat"));
        // Lấy ra theo thứ tự mới nhất trước.
        assert_eq!(s.take_undo(), Some(action("moi-nhat")));
        assert_eq!(s.take_undo(), Some(action("moi")));
        assert_eq!(s.take_undo(), None, "mục cũ nhất đã bị bỏ");
    }

    #[test]
    fn a_zero_capacity_stack_still_keeps_one_entry() {
        // Ngăn xếp rỗng hoàn toàn thì tính năng vô dụng; kẹp về 1.
        let mut s = UndoStack::with_capacity(0);
        s.record(action("a"));
        assert!(s.can_undo());
    }

    #[test]
    fn redoing_a_created_file_keeps_its_exact_name() {
        let dir = std::env::temp_dir().join("files-rs-undo-redo-name");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ghi-chu.md");

        redo(&UndoAction::Created {
            path: path.clone(),
            is_dir: false,
        })
        .expect("redo phải tạo được");

        assert!(path.is_file(), "đúng tên .md, không thành .md.txt");
        assert!(!dir.join("ghi-chu.md.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn labels_name_the_item() {
        let a = UndoAction::Rename {
            from: PathBuf::from(r"C:\x\cu.txt"),
            to: PathBuf::from(r"C:\x\moi.txt"),
        };
        assert!(a.undo_label().contains("moi.txt"));
        assert!(a.redo_label().contains("moi.txt"));
    }
}
