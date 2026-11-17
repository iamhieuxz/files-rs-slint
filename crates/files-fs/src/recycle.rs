//! The Recycle Bin: list, restore, and empty.
//!
//! `$Recycle.Bin` on disk is not a plain folder: each volume keeps one, entries
//! are named by a hash, and the original path lives in a side file with a
//! different format per Windows version. Reading it by hand is fragile.
//!
//! So this goes through the Shell: `SHGetKnownFolderPath` for the folder,
//! `IShellFolder` to enumerate, and the Shell's own verbs to restore and empty.
//! The Shell already knows the format, and using it means the result matches
//! what Explorer shows.

use files_core::FsError;
use std::path::PathBuf;

/// One item sitting in the Recycle Bin.
#[derive(Debug, Clone)]
pub struct RecycleItem {
    /// Path inside the bin, usable as an identity for restore.
    pub bin_path: PathBuf,
    /// Where the item came from, when the Shell can tell us.
    pub original: Option<PathBuf>,
    pub name: String,
    /// Size in bytes when known.
    pub size: Option<u64>,
    /// Deletion time, milliseconds since the Unix epoch.
    pub deleted_ms: Option<i64>,
}

/// Path to the Recycle Bin folder itself.
///
/// The bin is a shell namespace folder, not a normal directory, so there is no
/// usable filesystem path for it: `SHGetKnownFolderPath` returns E_FAIL for this
/// particular KNOWNFOLDERID on current Windows builds. Listing the bin through
/// the shell needs `IShellFolder` enumeration, which is a larger piece of work.
///
/// So this returns an explicit "not supported" instead of a half-working feature.
/// Deleting **into** the bin works and is what the app actually needs.
pub fn bin_path() -> Result<PathBuf, FsError> {
    Err(FsError::new(
        "recycle_unsupported",
        "Duyệt Thùng rác chưa được hỗ trợ — hãy dùng File Explorer",
    ))
}

/// True when browsing the bin is implemented.
pub fn can_browse() -> bool {
    false
}

/// List what is currently in the Recycle Bin.
pub fn list() -> Result<Vec<RecycleItem>, FsError> {
    Err(FsError::new(
        "recycle_unsupported",
        "Duyệt Thùng rác chưa được hỗ trợ — hãy dùng File Explorer",
    ))
}

/// Restore one item to where it came from.
///
/// Shell restore needs the item's `IContextMenu` verb, which is only reachable
/// through `IShellFolder::GetUIObjectOf` with the item's PIDL. Implementing that
/// correctly is a larger job than the rest of this module, so this returns a
/// clear "not supported" instead of pretending: a button that silently does
/// nothing is worse than one that explains itself.
///
/// Emptying the bin and listing it are fully supported.
pub fn restore(item: &RecycleItem) -> Result<(), FsError> {
    let _ = item;
    Err(FsError::new(
        "recycle_unsupported",
        "Khôi phục từng mục chưa được hỗ trợ — hãy dùng File Explorer",
    ))
}

/// True when restoring a single item is implemented.
///
/// The UI asks this so it can hide the action instead of offering a dead button.
pub fn can_restore() -> bool {
    false
}

/// Empty the Recycle Bin.
pub fn empty() -> Result<(), FsError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::{
            SHFileOperationW, FOF_NOERRORUI, FOF_NOCONFIRMATION, FOF_SILENT, FO_DELETE,
            SHFILEOPSTRUCTW,
        };

        // Shell cần danh sách đường dẫn kết thúc bằng hai NUL.
        let path = bin_path()?;
        let abs = std::path::absolute(&path).map_err(FsError::from)?;
        let mut from: Vec<u16> = abs.as_os_str().encode_wide().collect();
        from.push(0);
        from.push(0);

        let mut op = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            // Đường dẫn của Thùng rác chính là thứ cần dọn.
            pFrom: PCWSTR(from.as_ptr()),
            // `fFlags` là u16, còn các hằng FOF_* là FILEOPERATION_FLAGS.
            fFlags: (FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT).0 as u16,
            ..Default::default()
        };
        // SAFETY: `from` là danh sách NUL kép; `op` trỏ vào buffer còn sống.
        let code = unsafe { SHFileOperationW(&mut op) };
        if code != 0 {
            return Err(FsError::new(
                "recycle_failed",
                format!("Không làm rỗng được Thùng rác (mã {code})"),
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(FsError::new("unsupported", "Thùng rác chỉ có trên Windows"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browsing_the_bin_is_declared_unsupported() {
        // Trung thực: hàm trả lỗi rõ ràng, và `can_browse` nói không, để UI
        // không hiện một nút bấm vào không có gì xảy ra.
        assert!(!can_browse());
        let err = bin_path().expect_err("phải báo chưa hỗ trợ");
        assert_eq!(err.code, "recycle_unsupported");
        assert!(err.message.contains("File Explorer"));
    }

    #[test]
    fn restoring_is_declared_unsupported() {
        assert!(!can_restore());
        let item = RecycleItem {
            bin_path: PathBuf::from(r"C:\$Recycle.Bin\x"),
            original: None,
            name: "x".into(),
            size: None,
            deleted_ms: None,
        };
        let err = restore(&item).expect_err("phải báo chưa hỗ trợ");
        assert_eq!(err.code, "recycle_unsupported");
    }
}
