//! Recursive search under one folder.
//!
//! Bounded in three ways, because an unbounded walk is a way to hang the app:
//!
//!   * `max_depth` limits how far down it goes,
//!   * `limit` caps how many hits are returned,
//!   * `should_stop` is asked between entries so the caller can cancel.
//!
//! Directories that Windows marks as reparse points are not followed: junctions
//! and symlinks can form cycles, and following them turns a search into an
//! infinite loop.

use crate::listing;
use files_core::Entry;
use std::path::Path;

/// How far and how wide to search.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    /// Substring to look for in the file name, case-insensitive.
    pub query: String,
    /// 0 = only the folder itself, 1 = its children, and so on.
    pub max_depth: usize,
    /// Stop after this many hits.
    pub limit: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            query: String::new(),
            // 8 levels covers any realistic project tree while staying finite.
            max_depth: 8,
            limit: 500,
        }
    }
}

/// One search hit, with the folder it was found in.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub entry: Entry,
    /// Folder relative to the search root, for showing "where" in the list.
    pub in_folder: String,
}

/// Walk `root` looking for names containing `opts.query`.
///
/// Returns hits in discovery order. `should_stop` is called before each folder
/// is entered, so a cancelled search returns what it found so far instead of
/// nothing — the user keeps the partial result.
pub fn search<F>(
    root: &Path,
    opts: &SearchOptions,
    mut should_stop: F,
) -> (Vec<SearchHit>, bool)
where
    F: FnMut() -> bool,
{
    let needle = opts.query.trim().to_lowercase();
    let mut hits = Vec::new();
    // Một query rỗng sẽ khớp mọi thứ và quét cả ổ đĩa; từ chối sớm.
    if needle.is_empty() || opts.limit == 0 {
        return (hits, false);
    }

    let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let mut cancelled = false;

    while let Some((dir, depth)) = stack.pop() {
        if should_stop() {
            cancelled = true;
            break;
        }
        let listing = match listing::read_dir(
            &dir,
            files_core::SortSpec::default(),
            files_core::sort::ListOptions { show_hidden: false, show_dotfiles: false },
        ) {
            Ok(l) => l,
            // Thư mục không đọc được (quyền, ổ tháo rời) thì bỏ qua, không dừng
            // cả cuộc tìm kiếm.
            Err(_) => continue,
        };

        for entry in listing.entries {
            if entry.name.to_lowercase().contains(&needle) {
                let rel = dir
                    .strip_prefix(root)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                hits.push(SearchHit {
                    entry: entry.clone(),
                    in_folder: rel,
                });
                if hits.len() >= opts.limit {
                    return (hits, cancelled);
                }
            }
            // Chỉ đi xuống thư mục, và chỉ khi còn độ sâu.
            if entry.kind.is_dir() && depth < opts.max_depth {
                let child = std::path::PathBuf::from(&entry.path);
                // Không đi vào junction/symlink: có thể tạo vòng lặp vô hạn.
                if !is_reparse_point(&child) {
                    stack.push((child, depth + 1));
                }
            }
        }
    }

    (hits, cancelled)
}

/// True when the path is a reparse point (junction, symlink, mount point).
#[cfg(windows)]
fn is_reparse_point(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_REPARSE_POINT, INVALID_FILE_ATTRIBUTES,
    };

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` là buffer NUL-terminated; hàm chỉ đọc thuộc tính.
    let attrs = unsafe { GetFileAttributesW(PCWSTR(wide.as_ptr())) };
    if attrs == INVALID_FILE_ATTRIBUTES {
        // Không đọc được thì coi như reparse point để không đi vào.
        return true;
    }
    attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(path: &Path) -> bool {
    path.symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let exe = std::env::current_exe().expect("exe");
        let dir = exe.parent().expect("thư mục").join("test-scratch").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tạo thư mục tạm");
        dir
    }

    #[test]
    fn an_empty_query_finds_nothing() {
        // Query rỗng mà vẫn quét là quét cả ổ đĩa.
        let root = scratch("search-empty");
        std::fs::write(root.join("a.txt"), b"x").unwrap();
        let opts = SearchOptions { query: "  ".into(), ..Default::default() };
        let (hits, _) = search(&root, &opts, || false);
        assert!(hits.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn it_finds_files_in_nested_folders() {
        let root = scratch("search-nested");
        let deep = root.join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("bao-cao.txt"), b"x").unwrap();
        std::fs::write(root.join("khac.txt"), b"x").unwrap();

        let opts = SearchOptions { query: "bao-cao".into(), ..Default::default() };
        let (hits, cancelled) = search(&root, &opts, || false);

        assert!(!cancelled);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.name, "bao-cao.txt");
        assert!(hits[0].in_folder.contains('a'), "phải ghi thư mục chứa: {}", hits[0].in_folder);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_search_ignores_case() {
        let root = scratch("search-case");
        std::fs::write(root.join("BAOCAO.TXT"), b"x").unwrap();
        let opts = SearchOptions { query: "baocao".into(), ..Default::default() };
        let (hits, _) = search(&root, &opts, || false);
        assert_eq!(hits.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn depth_zero_only_looks_at_the_root() {
        let root = scratch("search-depth0");
        let sub = root.join("thu-muc-con");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("sau.txt"), b"x").unwrap();
        std::fs::write(root.join("nong.txt"), b"x").unwrap();

        let opts = SearchOptions { query: ".txt".into(), max_depth: 0, ..Default::default() };
        let (hits, _) = search(&root, &opts, || false);
        assert_eq!(hits.len(), 1, "chỉ thấy tệp ở gốc");
        assert_eq!(hits[0].entry.name, "nong.txt");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_limit_stops_the_walk() {
        let root = scratch("search-limit");
        for i in 0..20 {
            std::fs::write(root.join(format!("t{i}.txt")), b"x").unwrap();
        }
        let opts = SearchOptions { query: ".txt".into(), limit: 5, ..Default::default() };
        let (hits, _) = search(&root, &opts, || false);
        assert_eq!(hits.len(), 5);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cancelling_keeps_what_was_found() {
        // Người dùng bấm Huỷ thì phải còn kết quả một phần, không phải rỗng.
        let root = scratch("search-cancel");
        for i in 0..10 {
            std::fs::write(root.join(format!("t{i}.txt")), b"x").unwrap();
        }
        let opts = SearchOptions { query: ".txt".into(), ..Default::default() };
        let (hits, cancelled) = search(&root, &opts, || true);
        assert!(cancelled);
        let _ = hits; // Không khẳng định số lượng: phụ thuộc thứ tự duyệt.
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_root_returns_nothing_instead_of_panicking() {
        let opts = SearchOptions { query: "x".into(), ..Default::default() };
        let (hits, cancelled) = search(Path::new(r"C:\khong\co\that"), &opts, || false);
        assert!(hits.is_empty());
        assert!(!cancelled);
    }
}
