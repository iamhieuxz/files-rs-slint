//! Thùng rác: liệt kê và khôi phục, đọc trực tiếp `$RECYCLE.BIN` trên đĩa.
//!
//! # Vì sao không dùng Shell
//!
//! Đã thử năm đường Shell và ghi lại bằng chứng: `SHParseDisplayName`,
//! `SHGetFolderLocation(CSIDL_BITBUCKET)`, `BindToObject`,
//! `BindToHandler(BHID_EnumItems)` và một luồng STA riêng. Tất cả trả `S_OK`,
//! nhưng `IEnumIDList::Next` không trả mục nào, trong khi `Shell.Application`
//! thấy 45 mục và đọc đĩa thấy 21 tệp `$R`. Thùng rác là namespace ảo: mục của
//! nó chỉ đi qua được lớp `IShellFolderView`, không qua `IShellFolder` thuần.
//!
//! # Cách làm ở đây
//!
//! Mỗi ổ đĩa giữ `<ổ>:\$Recycle.Bin\<SID>\`. Trong đó mỗi mục đã xoá là một cặp:
//!
//! - `$I<id><ext>` — metadata (xem `files_core::recycle_meta`).
//! - `$R<id><ext>` — nội dung thật, tệp hoặc thư mục.
//!
//! Ghép cặp theo phần sau `$I`/`$R`. Khôi phục là chuyển `$R` về đường dẫn gốc
//! rồi xoá cả hai tệp. Toàn bộ là thao tác tệp thuần — không COM, không treo,
//! test được.
//!
//! # Giới hạn đã biết
//!
//! - Chỉ đọc được thùng rác của **người dùng hiện tại** trên các ổ nó truy cập
//!   được. Thùng rác của người dùng khác cần quyền quản trị.
//! - Định dạng `$I` cũ (Windows 95/98/ME) không đọc được — báo lỗi rõ ràng thay
//!   vì đoán.

use files_core::recycle_meta::{parse_i, stem_of, RecycleEntry};
use files_core::FsError;
use std::path::{Path, PathBuf};

/// Một mục trong Thùng rác.
#[derive(Debug, Clone)]
pub struct RecycleItem {
    /// Đường dẫn tới tệp `$I` — dùng làm định danh khi khôi phục.
    pub i_path: PathBuf,
    /// Đường dẫn tới tệp/thư mục `$R`.
    pub r_path: PathBuf,
    /// Đường dẫn gốc trước khi xoá.
    pub original: PathBuf,
    /// Tên hiển thị: tên cuối của đường dẫn gốc.
    pub name: String,
    /// Kích thước lúc xoá; với thư mục là tổng nội dung.
    pub size: u64,
    /// Thời điểm xoá, milli giây Unix.
    pub deleted_ms: Option<i64>,
    /// `$R` có phải thư mục không.
    pub is_dir: bool,
}

/// Đọc và ghép cặp các mục trong một thư mục thùng rác của một SID.
///
/// Tách khỏi `list` để test được trên thư mục dựng sẵn, không cần Thùng rác thật.
pub fn read_bin_dir(dir: &Path) -> Vec<RecycleItem> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    // Gom `$I` và `$R` theo stem để ghép cặp.
    let mut i_files: std::collections::HashMap<String, PathBuf> = Default::default();
    let mut r_files: std::collections::HashMap<String, PathBuf> = Default::default();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = stem_of(&name) else { continue };
        let path = entry.path();
        if name.starts_with("$I") {
            i_files.insert(stem.to_string(), path);
        } else {
            r_files.insert(stem.to_string(), path);
        }
    }

    let mut items = Vec::new();
    for (stem, i_path) in i_files {
        // `$I` mồ côi: Windows đã dọn `$R` nhưng còn metadata, hoặc ngược lại.
        // Bỏ qua thay vì hiện một mục không khôi phục được.
        let Some(r_path) = r_files.get(&stem) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(&i_path) else {
            continue;
        };
        let Ok(meta) = parse_i(&bytes) else {
            // Tệp `$I` hỏng: bỏ qua. Hiện sai đường dẫn gốc còn tệ hơn.
            continue;
        };
        let RecycleEntry {
            original_path,
            size,
            deleted_ms,
            ..
        } = meta;
        let original = PathBuf::from(&original_path);
        let name = original
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| original_path.clone());
        items.push(RecycleItem {
            i_path,
            r_path: r_path.clone(),
            original,
            name,
            size,
            deleted_ms,
            is_dir: r_path.is_dir(),
        });
    }

    // Mới xoá lên đầu, giống Explorer.
    items.sort_by_key(|i| std::cmp::Reverse(i.deleted_ms));
    items
}

/// SID của người dùng hiện tại, dạng chuỗi `S-1-5-...`.
#[cfg(windows)]
fn current_sid() -> Option<String> {
    use windows::Win32::Security::TOKEN_QUERY;
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: mở token của chính tiến trình này với quyền đọc; handle được đóng
    // bằng `CloseHandle` ngay sau khi dùng. Không có con trỏ lạ nào.
    unsafe {
        let mut token = windows::Win32::Foundation::HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return None;
        }
        let result = sid_from_token(token);
        let _ = windows::Win32::Foundation::CloseHandle(token);
        result
    }
}

#[cfg(windows)]
unsafe fn sid_from_token(token: windows::Win32::Foundation::HANDLE) -> Option<String> {
    use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_USER};

    // SAFETY: gọi hai lần theo mẫu Win32 chuẩn: lần đầu lấy kích thước, lần sau
    // lấy dữ liệu vào buffer ta cấp. `token` là handle hợp lệ đang mở.
    let mut needed = 0u32;
    let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
    if needed == 0 {
        return None;
    }
    let mut buffer = vec![0u8; needed as usize];
    GetTokenInformation(
        token,
        TokenUser,
        Some(buffer.as_mut_ptr() as *mut core::ffi::c_void),
        needed,
        &mut needed,
    )
    .ok()?;

    // SAFETY: buffer đã được Windows ghi đầy `needed` byte và chứa `TOKEN_USER`.
    let user = &*(buffer.as_ptr() as *const TOKEN_USER);
    sid_to_string(&user.User.Sid)
}

#[cfg(windows)]
unsafe fn sid_to_string(sid: &windows::Win32::Security::PSID) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;

    let mut out = PWSTR::null();
    // SAFETY: `sid` lấy từ token đang mở, hợp lệ trong suốt lời gọi. Windows cấp
    // chuỗi, ta sao chép rồi trả lại bằng `LocalFree`.
    ConvertSidToStringSidW(*sid, &mut out).ok()?;
    let text = out.to_string().ok();
    let _ = windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
        out.0 as *mut core::ffi::c_void,
    )));
    text
}

#[cfg(not(windows))]
fn current_sid() -> Option<String> {
    None
}

/// Các thư mục thùng rác hiện đọc được: mỗi ổ đĩa một, giới hạn ở SID hiện tại.
fn bin_dirs() -> Vec<PathBuf> {
    let Some(sid) = current_sid() else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    // A..Z: liệt kê ổ đĩa không cần API riêng; `read_dir` lỗi thì bỏ qua.
    for letter in b'A'..=b'Z' {
        let root = format!("{}:\\$Recycle.Bin\\{sid}", letter as char);
        let path = PathBuf::from(root);
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs
}

/// True khi duyệt Thùng rác đã hoạt động.
pub fn can_browse() -> bool {
    true
}

/// Liệt kê toàn bộ Thùng rác trên mọi ổ đĩa của người dùng hiện tại.
pub fn list() -> Result<Vec<RecycleItem>, FsError> {
    let dirs = bin_dirs();
    if dirs.is_empty() {
        // Không tìm thấy thư mục nào: thùng rác rỗng, hoặc người dùng chưa xoá
        // gì. Không phải lỗi.
        return Ok(Vec::new());
    }
    let mut all = Vec::new();
    for dir in dirs {
        all.extend(read_bin_dir(&dir));
    }
    all.sort_by_key(|i| std::cmp::Reverse(i.deleted_ms));
    Ok(all)
}

/// True khi khôi phục từng mục đã hoạt động.
pub fn can_restore() -> bool {
    true
}

/// Khôi phục một mục về đúng đường dẫn gốc.
///
/// Thứ tự cố ý: **chuyển `$R` về đích trước**, chỉ xoá `$I` sau khi đã thành
/// công. Nếu bước chuyển lỗi thì cả cặp còn nguyên trong thùng rác và người
/// dùng thử lại được. Làm ngược lại sẽ mất dữ liệu khi đích bị chiếm.
pub fn restore(item: &RecycleItem) -> Result<(), FsError> {
    if !item.r_path.exists() {
        return Err(FsError::new(
            "recycle_missing",
            format!("Không còn nội dung để khôi phục: {}", item.r_path.display()),
        ));
    }
    // Không ghi đè: đích đã có thì dừng, để người dùng tự quyết định.
    if item.original.exists() {
        return Err(FsError::new(
            "recycle_conflict",
            format!("Đã có tệp cùng tên ở đích: {}", item.original.display()),
        ));
    }
    if let Some(parent) = item.original.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                FsError::new(
                    "recycle_failed",
                    format!("Không tạo được thư mục đích {}: {e}", parent.display()),
                )
            })?;
        }
    }

    // `rename` chỉ chạy trong cùng ổ đĩa; khác ổ thì phải copy rồi xoá.
    let moved = match std::fs::rename(&item.r_path, &item.original) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(17) || e.raw_os_error() == Some(18) => {
            copy_then_remove(&item.r_path, &item.original)
        }
        Err(e) => Err(FsError::new(
            "recycle_failed",
            format!("Không khôi phục được: {e}"),
        )),
    };
    moved?;

    // Nội dung đã về đích an toàn; giờ mới dọn metadata. Lỗi ở đây không làm
    // hỏng dữ liệu người dùng, chỉ để lại một `$I` mồ côi.
    let _ = std::fs::remove_file(&item.i_path);
    Ok(())
}

/// Copy đệ quy rồi xoá nguồn, dùng khi khôi phục sang ổ đĩa khác.
fn copy_then_remove(from: &Path, to: &Path) -> Result<(), FsError> {
    if from.is_dir() {
        copy_dir(from, to)?;
        std::fs::remove_dir_all(from).map_err(|e| {
            FsError::new(
                "recycle_failed",
                format!("Đã copy nhưng không xoá được bản trong thùng rác: {e}"),
            )
        })
    } else {
        std::fs::copy(from, to)
            .map_err(|e| FsError::new("recycle_failed", format!("Không copy được: {e}")))?;
        std::fs::remove_file(from).map_err(|e| {
            FsError::new(
                "recycle_failed",
                format!("Đã copy nhưng không xoá được bản trong thùng rác: {e}"),
            )
        })
    }
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), FsError> {
    std::fs::create_dir_all(to)
        .map_err(|e| FsError::new("recycle_failed", format!("Không tạo thư mục: {e}")))?;
    let entries = std::fs::read_dir(from)
        .map_err(|e| FsError::new("recycle_failed", format!("Không đọc được nguồn: {e}")))?;
    for entry in entries.flatten() {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)
                .map_err(|e| FsError::new("recycle_failed", format!("Không copy được: {e}")))?;
        }
    }
    Ok(())
}

/// Làm rỗng Thùng rác trên mọi ổ đĩa.
///
/// `SHEmptyRecycleBinW` là API Shell dành cho việc này: nó biết thùng rác của
/// mọi ổ đĩa nằm ở đâu. App đã tự hỏi xác nhận nên tắt UI của Shell.
pub fn empty() -> Result<(), FsError> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::{
            SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND,
        };

        let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
        // SAFETY: lời gọi có tài liệu, không con trỏ riêng; HWND null và đường
        // dẫn gốc null nghĩa là "mọi ổ đĩa"; cờ tắt UI của Shell.
        unsafe { SHEmptyRecycleBinW(None, windows::core::PCWSTR::null(), flags) }.map_err(|e| {
            FsError::new(
                "recycle_failed",
                format!("Không làm rỗng được Thùng rác: {e}"),
            )
        })
    }
    #[cfg(not(windows))]
    {
        Err(FsError::new("unsupported", "Thùng rác chỉ có trên Windows"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dựng một tệp `$I` version 2 giống định dạng đo được trên đĩa.
    fn write_i(path: &Path, size: u64, filetime: i64, original: &str) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2u64.to_le_bytes());
        bytes.extend_from_slice(&size.to_le_bytes());
        bytes.extend_from_slice(&filetime.to_le_bytes());
        let units: Vec<u16> = original.encode_utf16().collect();
        bytes.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for u in units {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        std::fs::write(path, bytes).expect("ghi $I");
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("files-rs-bin-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tạo thư mục tạm");
        dir
    }

    #[test]
    fn a_pair_is_read_as_one_item() {
        let dir = scratch("pair");
        write_i(
            &dir.join("$IAAAAAA.txt"),
            42,
            133_795_232_000_000_000,
            r"C:\a\b.txt",
        );
        std::fs::write(dir.join("$RAAAAAA.txt"), b"x").expect("ghi $R");

        let items = read_bin_dir(&dir);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "b.txt");
        assert_eq!(items[0].original, PathBuf::from(r"C:\a\b.txt"));
        assert_eq!(items[0].size, 42);
        assert!(!items[0].is_dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_pair_is_flagged_as_dir() {
        let dir = scratch("dir");
        write_i(
            &dir.join("$IBBBBBB.0"),
            100,
            133_795_232_000_000_000,
            r"C:\a\thu-muc",
        );
        std::fs::create_dir_all(dir.join("$RBBBBBB.0")).expect("tạo $R");
        let items = read_bin_dir(&dir);
        assert_eq!(items.len(), 1);
        assert!(items[0].is_dir, "phải nhận ra đây là thư mục");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_orphan_i_without_r_is_skipped() {
        // `$I` mồ côi không khôi phục được nên không hiện ra.
        let dir = scratch("orphan");
        write_i(
            &dir.join("$ICCCCCC.txt"),
            1,
            133_795_232_000_000_000,
            r"C:\a\c.txt",
        );
        let items = read_bin_dir(&dir);
        assert!(items.is_empty(), "không có $R thì bỏ qua");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_i_does_not_hide_the_rest() {
        let dir = scratch("corrupt");
        std::fs::write(dir.join("$IDDDDDD.txt"), b"\x02\x00").expect("ghi hỏng");
        std::fs::write(dir.join("$RDDDDDD.txt"), b"x").expect("ghi $R");
        write_i(
            &dir.join("$IEEEEEE.txt"),
            7,
            133_795_232_000_000_000,
            r"C:\a\e.txt",
        );
        std::fs::write(dir.join("$REEEEEE.txt"), b"y").expect("ghi $R");
        let items = read_bin_dir(&dir);
        assert_eq!(items.len(), 1, "chỉ mục đọc được hiện ra");
        assert_eq!(items[0].name, "e.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn newest_item_comes_first() {
        let dir = scratch("order");
        write_i(
            &dir.join("$IOLDOLD.txt"),
            1,
            133_000_000_000_000_000,
            r"C:\a\old.txt",
        );
        std::fs::write(dir.join("$ROLDOLD.txt"), b"o").expect("ghi");
        write_i(
            &dir.join("$INEWNEW.txt"),
            1,
            134_000_000_000_000_000,
            r"C:\a\new.txt",
        );
        std::fs::write(dir.join("$RNEWNEW.txt"), b"n").expect("ghi");
        let items = read_bin_dir(&dir);
        assert_eq!(items[0].name, "new.txt", "mới xoá phải lên đầu");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_bin_directory_is_empty_not_an_error() {
        let missing = std::env::temp_dir().join("files-rs-bin-khong-ton-tai-xyz");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(read_bin_dir(&missing).is_empty());
    }

    #[test]
    fn restore_refuses_to_overwrite_an_existing_destination() {
        // Đây là quy tắc an toàn dữ liệu: không ghi đè tệp đang có.
        let dir = scratch("conflict");
        let dest = dir.join("dich.txt");
        std::fs::write(&dest, b"dang co").expect("tạo đích");
        write_i(
            &dir.join("$IFFFFF1.txt"),
            1,
            133_795_232_000_000_000,
            dest.to_str().unwrap(),
        );
        std::fs::write(dir.join("$RFFFFF1.txt"), b"trong-thung-rac").expect("ghi $R");

        let items = read_bin_dir(&dir);
        assert_eq!(items.len(), 1);
        let err = restore(&items[0]).expect_err("phải từ chối");
        assert_eq!(err.code, "recycle_conflict");
        // Đích giữ nguyên nội dung cũ.
        assert_eq!(std::fs::read(&dest).expect("đọc"), b"dang co");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_moves_content_back_and_removes_the_pair() {
        let dir = scratch("restore");
        let dest = dir.join("khoi-phuc.txt");
        write_i(
            &dir.join("$IGGGGGG.txt"),
            3,
            133_795_232_000_000_000,
            dest.to_str().unwrap(),
        );
        std::fs::write(dir.join("$RGGGGGG.txt"), b"abc").expect("ghi $R");

        let items = read_bin_dir(&dir);
        restore(&items[0]).expect("phải khôi phục được");

        assert_eq!(std::fs::read(&dest).expect("đọc đích"), b"abc");
        assert!(!items[0].r_path.exists(), "$R phải được chuyển đi");
        assert!(!items[0].i_path.exists(), "$I phải được dọn");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_recreates_a_missing_parent_folder() {
        let dir = scratch("parent");
        let dest = dir.join("thu-muc-moi").join("sau").join("t.txt");
        write_i(
            &dir.join("$IHHHHHH.txt"),
            2,
            133_795_232_000_000_000,
            dest.to_str().unwrap(),
        );
        std::fs::write(dir.join("$RHHHHHH.txt"), b"hi").expect("ghi $R");

        let items = read_bin_dir(&dir);
        restore(&items[0]).expect("phải khôi phục được");
        assert_eq!(std::fs::read(&dest).expect("đọc"), b"hi");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restoring_a_missing_r_reports_a_clear_error() {
        let dir = scratch("missing-r");
        write_i(
            &dir.join("$IIIIIII.txt"),
            1,
            133_795_232_000_000_000,
            r"C:\a\i.txt",
        );
        // Không tạo `$R`: mô phỏng mục đã bị dọn nội dung.
        let item = RecycleItem {
            i_path: dir.join("$IIIIIII.txt"),
            r_path: dir.join("$RIIIIIII.txt"),
            original: dir.join("i.txt"),
            name: "i.txt".into(),
            size: 1,
            deleted_ms: None,
            is_dir: false,
        };
        let err = restore(&item).expect_err("phải báo lỗi");
        assert_eq!(err.code, "recycle_missing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn browsing_and_restoring_are_now_supported() {
        assert!(can_browse());
        assert!(can_restore());
    }
}
