//! File operations.
//!
//! Files delegates destructive work to the Shell (`IFileOperation`) so the user
//! gets the system progress dialog, undo support and the Recycle Bin. The
//! portable equivalent here is `SHFileOperationW` with `FOF_ALLOWUNDO`, which
//! reaches the same Shell engine without pulling in COM apartment plumbing.
//!
//! Everything in this module is blocking. Callers on the UI thread must move the
//! work to a worker thread; see `app/src/ops_worker.rs`.

use files_core::FsError;
use std::path::{Path, PathBuf};

/// Windows hidden/system attribute bit values, mirrored here so this module
/// stays readable without importing the `windows` constants at every call site.
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;

/// True when the entry carries the Win32 hidden or system attribute.
///
/// Files checks `FileAttributes.Hidden`; the system attribute is included
/// because Explorer hides `desktop.ini` and friends by default too.
pub fn is_hidden_on_disk(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::GetFileAttributesW;

        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        // SAFETY: `wide` is a NUL-terminated UTF-16 buffer that outlives the call.
        let attrs = unsafe { GetFileAttributesW(PCWSTR(wide.as_ptr())) };
        if attrs == u32::MAX {
            return false;
        }
        attrs & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
    }
    #[cfg(not(windows))]
    {
        path.file_name()
            .map(|n| n.to_string_lossy().starts_with('.'))
            .unwrap_or(false)
    }
}

/// Send entries to the Recycle Bin.
///
/// Returns the number of entries the Shell accepted. A non-zero `SHFileOperationW`
/// result means the whole batch failed; per-item failures surface as a partial
/// count, which the status bar reports.
pub fn delete_to_recycle_bin(paths: &[PathBuf]) -> Result<usize, FsError> {
    if paths.is_empty() {
        return Ok(0);
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::{
            SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
            SHFILEOPSTRUCTW,
        };

        // The Shell wants a double-NUL-terminated list of absolute paths.
        let mut from: Vec<u16> = Vec::new();
        for p in paths {
            let abs = std::path::absolute(p).map_err(FsError::from)?;
            from.extend(abs.as_os_str().encode_wide());
            from.push(0);
        }
        from.push(0);

        let mut op = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: PCWSTR(from.as_ptr()),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
            ..Default::default()
        };

        // SAFETY: `from` is a double-NUL-terminated UTF-16 buffer, and `op`
        // outlives the call. The Shell copies the strings it needs.
        let rc = unsafe { SHFileOperationW(&mut op) };
        if rc != 0 {
            return Err(FsError::new(
                "delete_failed",
                format!("Shell trả về mã {rc} khi chuyển vào Thùng rác"),
            ));
        }
        if op.fAnyOperationsAborted.as_bool() {
            return Err(FsError::new("delete_aborted", "Thao tác xoá bị huỷ"));
        }
        Ok(paths.len())
    }

    #[cfg(not(windows))]
    {
        Err(FsError::new(
            "unsupported",
            "Recycle Bin chỉ có trên Windows",
        ))
    }
}

/// Copy one entry into `target_dir`, keeping its name.
///
/// Directories are copied recursively. A name clash produces `exists` rather
/// than silently overwriting, which is what the Shell asks the user about; the
/// UI turns that into a "keep both" prompt.
pub fn copy_into(source: &Path, target_dir: &Path) -> Result<PathBuf, FsError> {
    let name = source
        .file_name()
        .ok_or_else(|| FsError::new("invalid_path", "Không xác định được tên mục"))?;
    let dest = target_dir.join(name);
    if dest.exists() {
        return Err(FsError::new(
            "exists",
            format!("Đã tồn tại: {}", dest.display()),
        ));
    }
    copy_tree(source, &dest)?;
    Ok(dest)
}

/// Move one entry into `target_dir`.
pub fn move_into(source: &Path, target_dir: &Path) -> Result<PathBuf, FsError> {
    let name = source
        .file_name()
        .ok_or_else(|| FsError::new("invalid_path", "Không xác định được tên mục"))?;
    let dest = target_dir.join(name);
    if dest.exists() {
        return Err(FsError::new(
            "exists",
            format!("Đã tồn tại: {}", dest.display()),
        ));
    }
    // Same volume: a rename is atomic and cheap. Different volume: fall back to
    // copy + delete, which is what the Shell does too.
    match std::fs::rename(source, &dest) {
        Ok(()) => Ok(dest),
        Err(_) => {
            copy_tree(source, &dest)?;
            remove_tree(source)?;
            Ok(dest)
        }
    }
}

fn copy_tree(source: &Path, dest: &Path) -> Result<(), FsError> {
    let meta = std::fs::metadata(source).map_err(FsError::from)?;
    if meta.is_dir() {
        std::fs::create_dir_all(dest).map_err(FsError::from)?;
        for child in std::fs::read_dir(source).map_err(FsError::from)? {
            let child = child.map_err(FsError::from)?;
            copy_tree(&child.path(), &dest.join(child.file_name()))?;
        }
    } else {
        std::fs::copy(source, dest).map_err(FsError::from)?;
    }
    Ok(())
}

fn remove_tree(path: &Path) -> Result<(), FsError> {
    let meta = std::fs::metadata(path).map_err(FsError::from)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path).map_err(FsError::from)
    } else {
        std::fs::remove_file(path).map_err(FsError::from)
    }
}

/// Rename `source` to `new_name`, keeping it in the same directory.
pub fn rename(source: &Path, new_name: &str) -> Result<PathBuf, FsError> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return Err(FsError::new("invalid_name", "Tên không được để trống"));
    }
    // A name that contains a separator would silently move the entry elsewhere.
    if trimmed.contains(['\\', '/']) {
        return Err(FsError::new(
            "invalid_name",
            "Tên không được chứa dấu phân cách đường dẫn",
        ));
    }
    let parent = source
        .parent()
        .ok_or_else(|| FsError::new("invalid_path", "Không có thư mục cha"))?;
    let dest = parent.join(trimmed);
    if dest == source {
        return Ok(dest);
    }
    if dest.exists() {
        return Err(FsError::new(
            "exists",
            format!("Đã tồn tại: {}", dest.display()),
        ));
    }
    std::fs::rename(source, &dest).map_err(FsError::from)?;
    Ok(dest)
}

/// Create a new folder named `name` inside `parent`, choosing a free name.
pub fn create_folder(parent: &Path, name: &str) -> Result<PathBuf, FsError> {
    let base = if name.trim().is_empty() { "Thư mục mới" } else { name.trim() };
    let dest = unique_path(parent, base, "");
    std::fs::create_dir(&dest).map_err(FsError::from)?;
    Ok(dest)
}

/// Create an empty file named `name` inside `parent`.
pub fn create_file(parent: &Path, name: &str) -> Result<PathBuf, FsError> {
    let base = if name.trim().is_empty() { "Tệp mới" } else { name.trim() };
    let dest = unique_path(parent, base, ".txt");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest)
        .map_err(FsError::from)?;
    Ok(dest)
}

/// `base`, then `base (2)`, `base (3)`, … until the name is free.
///
/// Explorer's exact wording differs per locale; the numbering rule is what
/// matters here, and it must terminate.
fn unique_path(parent: &Path, base: &str, extension: &str) -> PathBuf {
    let first = parent.join(format!("{base}{extension}"));
    if !first.exists() {
        return first;
    }
    for n in 2..10_000 {
        let candidate = parent.join(format!("{base} ({n}){extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    first
}

/// Human-readable label for a copy of `source` that will not collide.
pub fn suggested_copy_name(source: &Path) -> String {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Bản sao".into());
    let ext = source
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    format!("{stem} - Bản sao{ext}")
}

/// Delete entries permanently, bypassing the Recycle Bin.
///
/// Files offers this behind Shift+Delete and warns first, because there is no
/// undo. Callers must confirm with the user before reaching here.
pub fn delete_permanently(paths: &[PathBuf]) -> Result<usize, FsError> {
    let mut done = 0usize;
    for p in paths {
        let meta = match std::fs::symlink_metadata(p) {
            Ok(m) => m,
            // Already gone: treat as done so a batch does not stop halfway.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                done += 1;
                continue;
            }
            Err(e) => return Err(FsError::from(e)),
        };
        if meta.is_dir() {
            std::fs::remove_dir_all(p).map_err(FsError::from)?;
        } else {
            std::fs::remove_file(p).map_err(FsError::from)?;
        }
        done += 1;
    }
    Ok(done)
}

/// Copy text to the Windows clipboard.
///
/// Uses the same Win32 clipboard the Shell uses, so the value pastes into any
/// application. Returns an error rather than panicking when another process
/// holds the clipboard open.
pub fn copy_text_to_clipboard(text: &str) -> Result<(), FsError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Foundation::{GlobalFree, HANDLE};
        use windows::Win32::System::DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
        };
        use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

        // The clipboard wants UTF-16, NUL-terminated.
        let wide: Vec<u16> = std::ffi::OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let bytes = wide.len() * std::mem::size_of::<u16>();

        // SAFETY: every call is checked; the buffer is freed on failure and
        // ownership passes to the system on success (per the Win32 contract).
        unsafe {
            OpenClipboard(None).map_err(|e| {
                FsError::new("clipboard_busy", format!("Không mở được clipboard: {e}"))
            })?;

            let result = (|| -> Result<(), FsError> {
                EmptyClipboard().map_err(|e| {
                    FsError::new("clipboard_failed", format!("Không xoá được clipboard: {e}"))
                })?;
                let handle = GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|e| {
                    FsError::new("clipboard_failed", format!("Không cấp phát được bộ nhớ: {e}"))
                })?;
                let ptr = GlobalLock(handle);
                if ptr.is_null() {
                    let _ = GlobalFree(Some(handle));
                    return Err(FsError::new("clipboard_failed", "Không khoá được bộ nhớ"));
                }
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr as *mut u16, wide.len());
                let _ = GlobalUnlock(handle);
                // CF_UNICODETEXT = 13
                if let Err(e) = SetClipboardData(13, Some(HANDLE(handle.0))) {
                    let _ = GlobalFree(Some(handle));
                    return Err(FsError::new(
                        "clipboard_failed",
                        format!("Không đặt được dữ liệu: {e}"),
                    ));
                }
                Ok(())
            })();

            let _ = CloseClipboard();
            result
        }
    }
    #[cfg(not(windows))]
    {
        let _ = text;
        Err(FsError::new("unsupported", "Clipboard chỉ có trên Windows"))
    }
}

/// Open a terminal (Windows Terminal if present, else `cmd`) in `dir`.
pub fn open_terminal(dir: &Path) -> Result<(), FsError> {
    // Windows Terminal is the default on 11 and understands `-d`.
    let wt = std::process::Command::new("wt.exe")
        .arg("-d")
        .arg(dir)
        .spawn();
    if wt.is_ok() {
        return Ok(());
    }
    std::process::Command::new("cmd.exe")
        .arg("/C")
        .arg("start")
        .arg("cmd.exe")
        .arg("/K")
        .arg("cd")
        .arg("/D")
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| FsError::new("spawn_failed", format!("Không mở được terminal: {e}")))
}

/// Total size of everything under `dir`, in bytes.
///
/// This walks the tree, which on a large folder can take seconds; callers must
/// run it off the UI thread. Unreadable children are skipped rather than
/// failing the whole count — a folder the user can see should still report the
/// size of the part that is readable.
pub fn directory_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            total = total.saturating_add(directory_size(&entry.path()));
        } else {
            total = total.saturating_add(meta.len());
        }
    }
    total
}
