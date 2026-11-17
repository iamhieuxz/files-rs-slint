//! Git working-tree status for the folder being shown.
//!
//! Read-only and dependency-free: it asks the `git` command that is already on
//! the machine instead of linking a Git library. That keeps the build small and
//! means the status always matches what the user sees in their terminal.
//!
//! The lookup is deliberately cheap and cached by the caller: running `git` on
//! every folder open is fine, running it per row is not.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What happened to one path in the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitState {
    /// New file git does not know about yet.
    Untracked,
    /// Modified since the last commit.
    Modified,
    /// Staged change waiting to be committed.
    Staged,
    /// Removed from the working tree.
    Deleted,
    /// Merge conflict.
    Conflicted,
}

impl GitState {
    /// Single letter for the list column.
    pub fn letter(self) -> &'static str {
        match self {
            Self::Untracked => "U",
            Self::Modified => "M",
            Self::Staged => "S",
            Self::Deleted => "D",
            Self::Conflicted => "!",
        }
    }

    /// Vietnamese label for the tooltip and properties pane.
    pub fn label(self) -> &'static str {
        match self {
            Self::Untracked => "Chưa được git theo dõi",
            Self::Modified => "Đã sửa",
            Self::Staged => "Đã đưa vào staging",
            Self::Deleted => "Đã xoá",
            Self::Conflicted => "Đang xung đột",
        }
    }

    /// Colour for the letter, matching git's own conventions.
    pub fn color(self) -> u32 {
        match self {
            Self::Untracked => 0x8a8f98,
            Self::Modified => 0xd8a838,
            Self::Staged => 0x3fb950,
            Self::Deleted => 0xd9534f,
            Self::Conflicted => 0xf85149,
        }
    }
}

/// Map from absolute path to its git state, for one folder.
pub type StatusMap = HashMap<PathBuf, GitState>;

/// Read `git status` for the repository containing `dir`.
///
/// Returns an empty map when `dir` is not inside a repository, when `git` is not
/// installed, or when the command fails — all of which are normal and must not
/// surface as errors to the user.
pub fn status_for(dir: &Path) -> StatusMap {
    let mut map = StatusMap::new();

    // `--porcelain=v1 -z` gives a stable, parseable form; `-z` avoids quoting
    // problems with spaces and non-ASCII names.
    let output = Command::new("git")
        .args([
            // `--no-optional-locks` là option TOÀN CỤC của git, phải đứng trước
            // subcommand. Đặt sau `status` thì git báo "unknown option" và lệnh
            // luôn thất bại — lỗi này từng làm tính năng im lặng không chạy.
            "--no-optional-locks",
            "-C",
            &dir.to_string_lossy(),
            "status",
            "--porcelain=v1",
            "-z",
            // Include untracked files individually instead of collapsing folders.
            "--untracked-files=all",
        ])
        .output();

    let Ok(output) = output else {
        return map;
    };
    if !output.status.success() {
        return map;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    for (path, state) in parse_porcelain(&text) {
        // Ghép với `dir` để có đường dẫn tuyệt đối khớp với danh sách tệp.
        map.insert(dir.join(path), state);
    }
    map
}

/// Parse `git status --porcelain=v1 -z` output into (path, state) pairs.
///
/// Split out so it can be tested without a repository: the format is the part
/// that breaks silently when git changes it.
pub fn parse_porcelain(text: &str) -> Vec<(String, GitState)> {
    let mut out = Vec::new();
    // `-z` separates records with NUL. A rename record has two NUL-separated
    // paths, so consume the extra one.
    let mut parts = text.split('\0').filter(|s| !s.is_empty());
    while let Some(record) = parts.next() {
        if record.len() < 4 {
            continue;
        }
        let bytes = record.as_bytes();
        let (x, y) = (bytes[0] as char, bytes[1] as char);
        // Dạng: "XY path" — hai ký tự trạng thái, một dấu cách, rồi đường dẫn.
        let path = record[3..].to_string();

        let state = if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
            GitState::Conflicted
        } else if x == '?' && y == '?' {
            GitState::Untracked
        } else if x == 'D' || y == 'D' {
            GitState::Deleted
        } else if x != ' ' && x != '?' {
            // Có thay đổi đã staged ở cột X.
            GitState::Staged
        } else {
            GitState::Modified
        };

        // Rename/copy: trạng thái 'R' hoặc 'C' kèm thêm một trường đường dẫn cũ.
        if x == 'R' || x == 'C' || y == 'R' || y == 'C' {
            let _ = parts.next();
        }

        out.push((path, state));
    }
    out
}

/// Find the repository root containing `dir`, if any.
///
/// Walks up looking for `.git`, which is cheap and avoids another `git` call.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    let mut current = Some(dir);
    while let Some(d) = current {
        if d.join(".git").exists() {
            return Some(d.to_path_buf());
        }
        current = d.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untracked_file_is_recognised() {
        let parsed = parse_porcelain("?? moi.txt\0");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, "moi.txt");
        assert_eq!(parsed[0].1, GitState::Untracked);
    }

    #[test]
    fn a_modified_file_is_recognised() {
        let parsed = parse_porcelain(" M sua.txt\0");
        assert_eq!(parsed[0].1, GitState::Modified);
    }

    #[test]
    fn a_staged_file_is_recognised() {
        let parsed = parse_porcelain("M  staged.txt\0");
        assert_eq!(parsed[0].1, GitState::Staged);
    }

    #[test]
    fn a_deleted_file_is_recognised() {
        let parsed = parse_porcelain(" D da-xoa.txt\0");
        assert_eq!(parsed[0].1, GitState::Deleted);
    }

    #[test]
    fn a_conflict_is_recognised() {
        // Cả hai phía sửa: git đánh dấu UU.
        let parsed = parse_porcelain("UU xung-dot.txt\0");
        assert_eq!(parsed[0].1, GitState::Conflicted);
        assert_eq!(parse_porcelain("AA ca-hai-them.txt\0")[0].1, GitState::Conflicted);
    }

    #[test]
    fn several_records_are_all_parsed() {
        let parsed = parse_porcelain("?? a.txt\0 M b.txt\0M  c.txt\0");
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].1, GitState::Untracked);
        assert_eq!(parsed[1].1, GitState::Modified);
        assert_eq!(parsed[2].1, GitState::Staged);
    }

    #[test]
    fn a_rename_consumes_its_extra_path() {
        // Bản ghi rename có HAI đường dẫn; nếu không bỏ qua cái thứ hai thì
        // đường dẫn cũ sẽ bị hiểu nhầm thành một bản ghi riêng.
        let parsed = parse_porcelain("R  ten-moi.txt\0ten-cu.txt\0?? khac.txt\0");
        assert_eq!(parsed.len(), 2, "chỉ hai mục thật");
        assert_eq!(parsed[0].0, "ten-moi.txt");
        assert_eq!(parsed[1].0, "khac.txt");
    }

    #[test]
    fn a_path_with_spaces_keeps_its_spaces() {
        let parsed = parse_porcelain("?? co dau cach.txt\0");
        assert_eq!(parsed[0].0, "co dau cach.txt");
    }

    #[test]
    fn empty_output_parses_to_nothing() {
        assert!(parse_porcelain("").is_empty());
        assert!(parse_porcelain("\0\0").is_empty());
    }

    #[test]
    fn a_short_garbage_record_is_skipped() {
        // Không được panic khi gặp dòng cụt.
        assert!(parse_porcelain("M\0").is_empty());
    }

    #[test]
    fn every_state_has_a_letter_and_a_colour() {
        for s in [
            GitState::Untracked,
            GitState::Modified,
            GitState::Staged,
            GitState::Deleted,
            GitState::Conflicted,
        ] {
            assert_eq!(s.letter().chars().count(), 1);
            assert!(!s.label().is_empty());
            assert_ne!(s.color(), 0);
        }
    }

    #[test]
    fn a_non_repository_returns_no_status() {
        // Thư mục ngoài repo: không lỗi, chỉ không có trạng thái.
        let map = status_for(Path::new(r"C:\khong\co\repo\nay"));
        assert!(map.is_empty());
    }

    #[test]
    fn the_repository_root_is_found_by_walking_up() {
        let dir = std::env::current_exe().expect("exe");
        // Bản build nằm trong target/ của chính repo này.
        if let Some(root) = repo_root(&dir) {
            assert!(root.join(".git").exists());
        }
    }
}
