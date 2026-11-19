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

/// Số ký tự tối đa của một thành phần tên trên NTFS.
const MAX_NAME_CHARS: usize = 255;

/// `ERROR_NOT_SAME_DEVICE` (Windows) và `EXDEV` (Unix).
///
/// Giữ thành hằng số có tên: con số trần `17`/`18` rất dễ bị đọc sai nghĩa khi
/// ai đó sửa nhánh lỗi.
#[cfg(windows)]
const CROSS_DEVICE_ERRNO: i32 = 17;
#[cfg(not(windows))]
const CROSS_DEVICE_ERRNO: i32 = 18;

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
            SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
            FO_DELETE, SHFILEOPSTRUCTW,
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
        // `FOF_NOERRORUI` ẩn lỗi từng mục, nên đếm theo thực tế: mục nào không
        // còn trên đĩa mới tính là đã xoá.
        Ok(paths
            .iter()
            .filter(|p| std::fs::symlink_metadata(p).is_err())
            .count())
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
    copy_into_with(source, target_dir, ConflictPolicy::Fail)
}

/// Như [`copy_into`] nhưng chỉ rõ cách xử lý trùng tên.
pub fn copy_into_with(
    source: &Path,
    target_dir: &Path,
    policy: ConflictPolicy,
) -> Result<PathBuf, FsError> {
    let dest = resolve_destination(source, target_dir, policy)?;
    // "Thay thế" vào chính thư mục nguồn nghĩa là dest == source. Xoá đích
    // trước khi chép sẽ xoá luôn nguồn, nên phải từ chối trước mọi thao tác.
    if same_entry(source, &dest) {
        return Err(same_path_error());
    }
    reject_into_descendant(source, target_dir)?;
    replace_staged(&dest, policy, || copy_tree(source, &dest))?;
    Ok(dest)
}

/// Move one entry into `target_dir`.
pub fn move_into(source: &Path, target_dir: &Path) -> Result<PathBuf, FsError> {
    move_into_with(source, target_dir, ConflictPolicy::Fail)
}

/// Như [`move_into`] nhưng chỉ rõ cách xử lý trùng tên.
pub fn move_into_with(
    source: &Path,
    target_dir: &Path,
    policy: ConflictPolicy,
) -> Result<PathBuf, FsError> {
    let dest = resolve_destination(source, target_dir, policy)?;
    // Cùng lý do với copy: xoá đích trùng nguồn là xoá mất dữ liệu.
    if same_entry(source, &dest) {
        return Err(same_path_error());
    }
    reject_into_descendant(source, target_dir)?;
    replace_staged(&dest, policy, || {
        // Same volume: a rename is atomic and cheap. Different volume: fall back
        // to copy + delete, which is what the Shell does too. Any other error
        // (access denied, locked file) must surface, not trigger a copy.
        match std::fs::rename(source, &dest) {
            Ok(()) => Ok(()),
            Err(e) if is_cross_device(&e) => {
                copy_tree(source, &dest)?;
                remove_tree(source)
            }
            Err(e) => Err(FsError::from(e)),
        }
    })?;
    Ok(dest)
}

/// `ERROR_NOT_SAME_DEVICE` (Windows) hoặc `EXDEV` (Unix).
fn is_cross_device(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(CROSS_DEVICE_ERRNO)
}

/// Từ chối chép/chuyển một thư mục vào chính nó hoặc thư mục con của nó.
///
/// Nếu không chặn, `copy_tree` sẽ đọc thư mục đang được chính nó ghi thêm và
/// không bao giờ dừng; `rename` thì hỏng giữa chừng.
fn reject_into_descendant(source: &Path, target_dir: &Path) -> Result<(), FsError> {
    if !source.is_dir() {
        return Ok(());
    }
    if let (Ok(src), Ok(dst)) = (
        std::fs::canonicalize(source),
        std::fs::canonicalize(target_dir),
    ) {
        if dst.starts_with(&src) {
            return Err(FsError::new(
                "into_self",
                "Không thể chép hoặc di chuyển một thư mục vào chính nó",
            ));
        }
    }
    Ok(())
}

/// Chạy `write` để tạo `dest`, xử lý đích đã tồn tại theo `policy`.
///
/// `Replace` không xoá trước: mục cũ được đổi sang tên tạm, `write` chạy, rồi
/// mới xoá bản cũ. Nếu `write` lỗi thì bản cũ được trả về chỗ cũ.
fn replace_staged(
    dest: &Path,
    policy: ConflictPolicy,
    write: impl FnOnce() -> Result<(), FsError>,
) -> Result<(), FsError> {
    if !dest.exists() {
        return write();
    }
    if policy != ConflictPolicy::Replace {
        return Err(FsError::new(
            "exists",
            format!("Đã tồn tại: {}", dest.display()),
        ));
    }
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let backup = unique_path(parent, &format!(".{name}.replaced"), "");
    std::fs::rename(dest, &backup).map_err(FsError::from)?;
    match write() {
        Ok(()) => {
            // Bản mới đã ghi xong; dọn bản cũ là việc phụ. Nếu dọn lỗi thì chỉ
            // ghi log — báo thao tác thất bại lúc này là nói dối người dùng,
            // vì dữ liệu mới đã nằm đúng chỗ.
            if let Err(e) = remove_tree(&backup) {
                crate::log::warn(&format!(
                    "không dọn được bản cũ {}: {}",
                    backup.display(),
                    e.message
                ));
            }
            Ok(())
        }
        Err(e) => {
            // Dọn phần ghi dở rồi trả bản cũ về. Nếu không dọn được thì bản cũ
            // vẫn nằm ở `backup`; phải nói rõ đường dẫn đó thay vì trả một lỗi
            // rename khó hiểu và để người dùng tưởng dữ liệu đã mất.
            let _ = remove_tree(dest);
            if let Err(restore) = std::fs::rename(&backup, dest) {
                return Err(FsError::new(
                    "replace_rollback_failed",
                    format!(
                        "Thao tác lỗi ({}) và không trả được bản cũ về chỗ cũ ({}). \
                         Dữ liệu cũ vẫn còn tại {}.",
                        e.message,
                        restore,
                        backup.display()
                    ),
                ));
            }
            Err(e)
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
    validate_name(trimmed)?;
    let parent = source
        .parent()
        .ok_or_else(|| FsError::new("invalid_path", "Không có thư mục cha"))?;
    let dest = parent.join(trimmed);
    if dest == source {
        return Ok(dest);
    }
    // NTFS không phân biệt hoa/thường: `a.txt` → `A.txt` khiến `dest.exists()`
    // đúng dù đó chính là tệp đang đổi tên. Chặn bằng `exists()` sẽ làm người
    // dùng không đổi được kiểu chữ của tên.
    if same_entry(source, &dest) {
        return rename_changing_case(source, &dest);
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

/// Đổi tên chỉ khác hoa/thường, qua một tên tạm trung gian.
///
/// Windows không cho `rename` trực tiếp sang tên chỉ khác kiểu chữ khi tên đích
/// đang tồn tại (nó chính là tệp nguồn). Đi qua tên tạm là cách Explorer làm.
fn rename_changing_case(source: &Path, dest: &Path) -> Result<PathBuf, FsError> {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let temp = unique_path(parent, ".files-rs-rename", ".tmp");
    std::fs::rename(source, &temp).map_err(FsError::from)?;
    match std::fs::rename(&temp, dest) {
        Ok(()) => Ok(dest.to_path_buf()),
        Err(e) => {
            // Trả tệp về tên cũ thay vì để nó mắc kẹt ở tên tạm.
            let _ = std::fs::rename(&temp, source);
            Err(FsError::from(e))
        }
    }
}

/// Kiểm tra một tên mục đơn lẻ (không phải đường dẫn).
///
/// Từ chối tên rỗng, dấu phân cách, `.` và `..`, ký tự cấm của Windows, tên
/// thiết bị DOS (`CON`, `NUL`, `COM1`…) và dấu chấm hoặc khoảng trắng ở cuối.
pub fn validate_name(name: &str) -> Result<(), FsError> {
    let bad = |msg: &str| Err(FsError::new("invalid_name", msg));
    if name.is_empty() {
        return bad("Tên không được để trống");
    }
    if name.contains(['\\', '/']) {
        return bad("Tên không được chứa dấu phân cách đường dẫn");
    }
    if name == "." || name == ".." {
        return bad("Tên không hợp lệ");
    }
    if name.contains(['<', '>', ':', '"', '|', '?', '*']) || name.chars().any(|c| c.is_control()) {
        return bad("Tên chứa ký tự không được phép: < > : \" | ? *");
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return bad("Tên không được kết thúc bằng dấu chấm hoặc khoảng trắng");
    }
    // NTFS giới hạn 255 ký tự cho một thành phần tên. Báo ở đây để người dùng
    // nhận thông báo hành động được thay vì lỗi OS khó hiểu.
    if name.chars().count() > MAX_NAME_CHARS {
        return bad("Tên quá dài (tối đa 255 ký tự)");
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    if reserved {
        return bad("Tên trùng với tên thiết bị dành riêng của Windows");
    }
    Ok(())
}

/// Create a new folder named `name` inside `parent`, choosing a free name.
pub fn create_folder(parent: &Path, name: &str) -> Result<PathBuf, FsError> {
    let base = if name.trim().is_empty() {
        "Thư mục mới"
    } else {
        name.trim()
    };
    validate_name(base)?;
    let dest = unique_path(parent, base, "");
    std::fs::create_dir(&dest).map_err(FsError::from)?;
    Ok(dest)
}

/// Create an empty file named `name` inside `parent`.
pub fn create_file(parent: &Path, name: &str) -> Result<PathBuf, FsError> {
    let base = if name.trim().is_empty() {
        "Tệp mới"
    } else {
        name.trim()
    };
    validate_name(base)?;
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

/// Đường dẫn con còn trống đầu tiên, dùng khi UI cần tự chọn tên.
///
/// Ví dụ nén `photos` khi `photos.zip` đã tồn tại sẽ nhận `photos (2).zip`
/// thay vì ghi đè archive cũ.
pub fn free_path(parent: &Path, base: &str, extension: &str) -> PathBuf {
    unique_path(parent, base, extension)
}

/// Cách xử lý khi mục đích đã tồn tại.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictPolicy {
    /// Từ chối và báo lỗi — hành vi cũ, an toàn nhất.
    #[default]
    Fail,
    /// Đổi tên mục mới thành "tên (2)" rồi tiếp tục.
    KeepBoth,
    /// Xoá mục cũ rồi ghi đè.
    Replace,
}

impl ConflictPolicy {
    /// Nhãn hiển thị trong hộp thoại.
    pub fn label(self) -> &'static str {
        match self {
            Self::Fail => "Bỏ qua",
            Self::KeepBoth => "Giữ cả hai",
            Self::Replace => "Thay thế",
        }
    }
}

/// Đường dẫn đích cho một mục, theo `policy`.
///
/// `KeepBoth` dùng `unique_path` để không đè lên bất cứ thứ gì; `Replace` trả
/// đúng đường dẫn cũ để hàm gọi tự xoá trước khi ghi.
fn resolve_destination(
    source: &Path,
    target_dir: &Path,
    policy: ConflictPolicy,
) -> Result<PathBuf, FsError> {
    let name = source
        .file_name()
        .ok_or_else(|| FsError::new("invalid_path", "Không xác định được tên mục"))?;
    let dest = target_dir.join(name);
    if !dest.exists() {
        return Ok(dest);
    }
    match policy {
        ConflictPolicy::Fail => Err(FsError::new(
            "exists",
            format!("Đã tồn tại: {}", dest.display()),
        )),
        ConflictPolicy::KeepBoth => {
            let stem = source
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Bản sao".into());
            let ext = source
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            Ok(unique_path(target_dir, &stem, &ext))
        }
        ConflictPolicy::Replace => Ok(dest),
    }
}

/// Hai đường dẫn có trỏ vào cùng một mục đang tồn tại hay không.
///
/// So sánh chuỗi đơn thuần bỏ sót khác biệt hoa/thường và dấu phân cách cuối,
/// nên khi cả hai tồn tại thì so sánh đường dẫn chuẩn hoá của hệ điều hành.
fn same_entry(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn same_path_error() -> FsError {
    FsError::new(
        "same_path",
        "Nguồn và đích là cùng một mục — không có gì để thay thế",
    )
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
        use windows::Win32::System::Memory::{
            GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
        };

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
                    FsError::new(
                        "clipboard_failed",
                        format!("Không cấp phát được bộ nhớ: {e}"),
                    )
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
    // Windows Terminal is the default on 11 and understands `-d`. Terminal là
    // thứ người dùng **muốn** thấy, nên không ẩn cửa sổ ở đây.
    let wt = std::process::Command::new("wt.exe")
        .arg("-d")
        .arg(dir)
        .spawn();
    if wt.is_ok() {
        return Ok(());
    }
    let mut command = std::process::Command::new("cmd.exe");
    command
        .arg("/C")
        .arg("start")
        .arg("cmd.exe")
        .arg("/K")
        .arg("cd")
        .arg("/D")
        .arg(dir);
    // Cửa sổ `cmd` con do `start` tạo là thứ người dùng cần; nhưng tiến trình
    // `cmd /C start` trung gian cũng tạo một console nháy lên trước đó. Ẩn nó đi
    // mà vẫn để `start` mở cửa sổ terminal thật.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| FsError::new("spawn_failed", format!("Không mở được terminal: {e}")))
}

/// Total size of everything under `dir`, in bytes.
///
/// This walks the tree, which on a large folder can take seconds; callers must
/// run it off the UI thread. Unreadable children are skipped rather than
/// failing the whole count — a folder the user can see should still report the
/// size of the part that is readable. Junctions/symlinks are not followed:
/// they can form cycles and would otherwise count the same tree many times.
pub fn directory_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        if is_reparse_point(&child) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            total = total.saturating_add(directory_size(&child));
        } else {
            total = total.saturating_add(meta.len());
        }
    }
    total
}

/// True when the path is a reparse point (junction, symlink, mount point).
#[cfg(windows)]
pub(crate) fn is_reparse_point(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_REPARSE_POINT, INVALID_FILE_ATTRIBUTES,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` là buffer NUL-terminated; hàm chỉ đọc thuộc tính.
    let attrs = unsafe { GetFileAttributesW(PCWSTR(wide.as_ptr())) };
    if attrs == INVALID_FILE_ATTRIBUTES {
        // Không đọc được thì coi như reparse point để không đi vào.
        return true;
    }
    attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

#[cfg(not(windows))]
pub(crate) fn is_reparse_point(path: &Path) -> bool {
    path.symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(true)
}

/// Đích mà một tệp `.lnk` trỏ tới.
///
/// Đọc bằng `IShellLinkW` qua COM vì định dạng `.lnk` là một shell item, không
/// phải tệp văn bản. Trả `None` khi tệp không phải shortcut hoặc đích không
/// đọc được — đó là chuyện bình thường với shortcut hỏng, không phải lỗi.
#[cfg(windows)]
pub fn shortcut_target(path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    // COM phải được khởi tạo trên luồng gọi; worker thì chưa.
    // SAFETY: `None` hợp lệ cho tham số reserved. Không `CoUninitialize` vì
    // luồng sống lâu hơn lời gọi và còn dùng Shell tiếp.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);

    // SAFETY: mọi lời gọi đều kiểm tra kết quả; buffer sống đủ lâu.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = link.cast().ok()?;
        persist
            .Load(
                PCWSTR(wide.as_ptr()),
                windows::Win32::System::Com::STGM_READ,
            )
            .ok()?;

        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        if end == 0 {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..end])))
    }
}

#[cfg(not(windows))]
pub fn shortcut_target(_path: &Path) -> Option<PathBuf> {
    None
}

/// Xoay ảnh 90° theo chiều kim đồng hồ và ghi đè lên chính tệp đó.
///
/// Chỉ hỗ trợ định dạng mà Windows đọc/ghi được qua WIC (JPEG, PNG, BMP, GIF,
/// TIFF). Định dạng khác trả lỗi rõ ràng thay vì ghi ra tệp hỏng.
///
/// Xoay là thao tác phá huỷ: ghi đè lên tệp gốc nên không có bản sao để lùi.
/// Người dùng được hỏi trước ở tầng UI.
pub fn rotate_image_clockwise(path: &Path) -> Result<(), FsError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(
        ext.as_str(),
        "jpg" | "jpeg" | "png" | "bmp" | "gif" | "tif" | "tiff"
    ) {
        return Err(FsError::new(
            "unsupported",
            format!("Không xoay được định dạng .{ext}"),
        ));
    }

    // Đọc toàn bộ ảnh vào bộ nhớ, xoay, rồi ghi đè. Làm vậy để không bao giờ
    // để lại tệp dở dang: ghi xong mới thay tệp cũ.
    let bytes = std::fs::read(path).map_err(FsError::from)?;
    let rotated = rotate_bytes(&bytes, &ext)?;

    let tmp = path.with_extension(format!("{ext}.rot"));
    std::fs::write(&tmp, &rotated).map_err(FsError::from)?;
    // Thay thế nguyên tử: nếu `rename` lỗi thì tệp gốc vẫn còn nguyên.
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        FsError::from(e)
    })?;
    Ok(())
}

/// Xoay 90° thuần trên byte ảnh, tách khỏi phần đọc/ghi để test được.
#[cfg(windows)]
fn rotate_bytes(bytes: &[u8], ext: &str) -> Result<Vec<u8>, FsError> {
    use windows::Win32::Graphics::Imaging::WICBitmapEncoderNoCache;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, IWICImagingFactory, WICBitmapTransformRotate90,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };

    // SAFETY: `None` hợp lệ cho tham số reserved của CoInitializeEx.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

    // SAFETY: mọi lời gọi đều kiểm tra kết quả; mọi con trỏ COM do Windows cấp.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| FsError::new("image_failed", format!("WIC: {e}")))?;

        let stream = factory
            .CreateStream()
            .map_err(|e| FsError::new("image_failed", format!("tạo stream: {e}")))?;
        stream
            .InitializeFromMemory(bytes)
            .map_err(|e| FsError::new("image_failed", format!("nạp ảnh: {e}")))?;

        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), Default::default())
            .map_err(|e| FsError::new("image_failed", format!("giải mã: {e}")))?;
        let frame = decoder
            .GetFrame(0)
            .map_err(|e| FsError::new("image_failed", format!("đọc khung: {e}")))?;
        let processor = factory
            .CreateBitmapFlipRotator()
            .map_err(|e| FsError::new("image_failed", format!("tạo bộ xoay: {e}")))?;
        processor
            .Initialize(&frame, WICBitmapTransformRotate90)
            .map_err(|e| FsError::new("image_failed", format!("xoay: {e}")))?;

        // Stream đích tạo bằng `SHCreateMemStream`: nó đã có bộ đệm sẵn, khác
        // `IWICStream` chưa khởi tạo (encoder từ chối stream đó).
        let out: windows::Win32::System::Com::IStream =
            windows::Win32::UI::Shell::SHCreateMemStream(None)
                .ok_or_else(|| FsError::new("image_failed", "không tạo được bộ đệm đích"))?;
        let encoder = factory
            .CreateEncoder(&guid_for(ext), std::ptr::null())
            .map_err(|e| FsError::new("image_failed", format!("tạo encoder: {e}")))?;
        encoder
            .Initialize(&out, WICBitmapEncoderNoCache)
            .map_err(|e| FsError::new("image_failed", format!("mở encoder: {e}")))?;
        // WIC trả khung qua con trỏ ra, không phải giá trị trả về.
        let mut frame_out: Option<windows::Win32::Graphics::Imaging::IWICBitmapFrameEncode> = None;
        let mut options_out: Option<windows::Win32::System::Com::StructuredStorage::IPropertyBag2> =
            None;
        encoder
            .CreateNewFrame(&mut frame_out, &mut options_out)
            .map_err(|e| FsError::new("image_failed", format!("khung đích: {e}")))?;
        let encoder_frame =
            frame_out.ok_or_else(|| FsError::new("image_failed", "không tạo được khung đích"))?;
        // Không có tuỳ chọn encoder nào: `None` là hợp lệ.
        encoder_frame
            .Initialize(None)
            .map_err(|e| FsError::new("image_failed", format!("khởi tạo khung: {e}")))?;
        // Phải khai báo kích thước khung TRƯỚC khi ghi: thiếu bước này encoder
        // báo "codec is in the wrong state" lúc Commit.
        let (mut w, mut h) = (0u32, 0u32);
        processor
            .GetSize(&mut w, &mut h)
            .map_err(|e| FsError::new("image_failed", format!("kích thước ảnh: {e}")))?;
        encoder_frame
            .SetSize(w, h)
            .map_err(|e| FsError::new("image_failed", format!("đặt kích thước: {e}")))?;
        encoder_frame
            .WriteSource(&processor, std::ptr::null_mut())
            .map_err(|e| FsError::new("image_failed", format!("ghi ảnh: {e}")))?;
        // Khung phải được commit TRƯỚC encoder; thiếu bước này encoder báo
        // "codec is in the wrong state".
        encoder_frame
            .Commit()
            .map_err(|e| FsError::new("image_failed", format!("hoàn tất khung: {e}")))?;
        encoder
            .Commit()
            .map_err(|e| FsError::new("image_failed", format!("hoàn tất: {e}")))?;

        // Đọc kích thước bằng cách seek tới cuối rồi về đầu.
        let mut size = 0u64;
        out.Seek(
            0,
            windows::Win32::System::Com::STREAM_SEEK_END,
            Some(&mut size),
        )
        .map_err(|e| FsError::new("image_failed", format!("kích thước: {e}")))?;
        out.Seek(0, windows::Win32::System::Com::STREAM_SEEK_SET, None)
            .map_err(|e| FsError::new("image_failed", format!("về đầu: {e}")))?;
        let mut buf = vec![0u8; size as usize];
        let mut read = 0u32;
        // `Read` trả HRESULT chứ không phải Result, nên kiểm tra bằng `ok()`.
        out.Read(buf.as_mut_ptr() as *mut _, size as u32, Some(&mut read))
            .ok()
            .map_err(|e| FsError::new("image_failed", format!("đọc lại: {e}")))?;
        buf.truncate(read as usize);
        Ok(buf)
    }
}

#[cfg(windows)]
fn guid_for(ext: &str) -> windows::core::GUID {
    use windows::Win32::Graphics::Imaging::{
        GUID_ContainerFormatBmp, GUID_ContainerFormatGif, GUID_ContainerFormatJpeg,
        GUID_ContainerFormatPng, GUID_ContainerFormatTiff,
    };
    match ext {
        "jpg" | "jpeg" => GUID_ContainerFormatJpeg,
        "gif" => GUID_ContainerFormatGif,
        "tif" | "tiff" => GUID_ContainerFormatTiff,
        "bmp" => GUID_ContainerFormatBmp,
        _ => GUID_ContainerFormatPng,
    }
}

#[cfg(not(windows))]
fn rotate_bytes(_bytes: &[u8], _ext: &str) -> Result<Vec<u8>, FsError> {
    Err(FsError::new("unsupported", "Xoay ảnh chỉ có trên Windows"))
}
