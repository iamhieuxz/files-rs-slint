//! Đăng ký ứng dụng làm trình duyệt tệp mặc định của Windows.
//!
//! Windows 10/11 không cho phép một chương trình tự đặt mình làm mặc định mà
//! không có sự đồng ý của người dùng. Cách hợp lệ duy nhất:
//!
//!   1. Đăng ký **ProgID** + **ứng dụng** trong `HKEY_CURRENT_USER\Software\Classes`.
//!   2. Mở trang Settings của Windows để người dùng tự chọn.
//!
//! Bước 1 là việc của module này. Bước 2 (`open_settings_page`) chỉ mở trang —
//! không có API nào đặt mặc định mà bỏ qua bước người dùng xác nhận, và mọi
//! thủ thuật lách đều bị Windows hoàn nguyên ở lần đăng nhập sau.
//!
//! Đăng ký nằm ở HKCU nên không cần quyền admin và gỡ được sạch sẽ.

use files_core::FsError;
use std::path::Path;

/// Tên ProgID dùng cho thư mục.
pub const FOLDER_PROGID: &str = "FilesRS.Folder";
/// Tên ProgID dùng cho ổ đĩa.
pub const DRIVE_PROGID: &str = "FilesRS.Drive";
/// Tên ứng dụng đăng ký trong OpenWithProgids.
pub const APP_NAME: &str = "FilesRS";

/// Giá trị cho khoá `DefaultIcon`: đường dẫn exe kèm chỉ số icon.
///
/// Registry đọc chuỗi này chứ không đọc đường dẫn, nên phải giữ đúng dạng
/// `"đường dẫn",chỉ số`.
pub fn icon_ref(exe: &str) -> String {
    format!("{exe},0")
}

/// Dòng lệnh Windows chạy khi người dùng mở một thư mục bằng app này.
///
/// Đường dẫn **phải** đặt trong ngoặc kép: `C:\Program Files\...` có dấu cách,
/// và Windows sẽ tách thành nhiều tham số nếu thiếu ngoặc. `%1` cũng phải có
/// ngoặc vì tên thư mục hoàn toàn có thể chứa dấu cách.
pub fn open_command(exe: &str) -> String {
    format!("\"{exe}\" \"%1\"")
}

/// Khoá gốc của một ProgID trong `HKCU`.
pub fn progid_key(progid: &str) -> String {
    format!(r"Software\Classes\{progid}")
}

/// Khoá đăng ký ứng dụng, dùng cho tên hiển thị trong Settings.
pub fn application_key() -> String {
    format!(r"Software\Classes\Applications\{APP_NAME}.exe")
}

/// Khoá chứa danh sách khả năng mở tệp.
pub fn capabilities_key() -> String {
    format!(r"Software\{APP_NAME}\Capabilities")
}

/// Đăng ký app làm một lựa chọn "mở bằng" cho thư mục và ổ đĩa.
///
/// Sau khi chạy, người dùng thấy "Files RS" trong menu chuột phải → Open with,
/// và có thể chọn nó làm mặc định trong Settings.
pub fn register(exe: &Path) -> Result<(), FsError> {
    let exe = std::path::absolute(exe).map_err(FsError::from)?;
    let exe_str = exe.to_string_lossy();
    let icon = icon_ref(&exe_str);
    let command = open_command(&exe_str);

    for (progid, kind) in [
        (FOLDER_PROGID, "Thư mục"),
        (DRIVE_PROGID, "Ổ đĩa"),
    ] {
        let base = progid_key(progid);
        set_string(&format!(r"{base}\DefaultIcon"), "", &icon)?;
        set_string(&format!(r"{base}\shell\open\command"), "", &command)?;
        set_string(&base, "", kind)?;
        // Cho phép app xuất hiện trong danh sách "Open with".
        set_string(&format!(r"{base}\shell"), "", "")?;
    }

    // Đăng ký ứng dụng để nó có tên hiển thị trong Settings.
    let app_key = application_key();
    set_string(&app_key, "FriendlyAppName", "Files RS")?;
    set_string(&format!(r"{app_key}\shell\open\command"), "", &command)?;
    // Capabilities: danh sách loại nội dung app nhận mở.
    let caps = capabilities_key();
    set_string(&caps, "ApplicationName", "Files RS")?;
    set_string(&caps, "ApplicationDescription", "Trình quản lý tệp cho Windows")?;
    set_string(&format!(r"{caps}\FileAssociations"), ".zip", "CompressedFolder")?;
    // Đăng ký khả năng để Windows hiện app trong "Choose a default".
    set_string(
        r"Software\RegisteredApplications",
        APP_NAME,
        &caps,
    )?;

    notify_shell();
    Ok(())
}

/// Gỡ toàn bộ đăng ký. An toàn khi chưa từng đăng ký.
pub fn unregister() -> Result<(), FsError> {
    for key in [
        r"Software\Classes\FilesRS.Folder",
        r"Software\Classes\FilesRS.Drive",
        r"Software\Classes\Applications\FilesRS.exe",
        r"Software\FilesRS",
    ] {
        delete_tree(key);
    }
    delete_value(r"Software\RegisteredApplications", APP_NAME);
    notify_shell();
    Ok(())
}

/// Mở trang Settings nơi người dùng chọn app mặc định cho tệp/thư mục.
///
/// Windows không cho đặt mặc định bằng code; đây là bước duy nhất còn lại và
/// nó cần người dùng bấm.
pub fn open_settings_page() -> Result<(), FsError> {
    std::process::Command::new("cmd.exe")
        .args(["/C", "start", "ms-settings:defaultapps"])
        .spawn()
        .map(|_| ())
        .map_err(|e| FsError::new("spawn_failed", format!("Không mở được Settings: {e}")))
}

// ---------------------------------------------------------------- registry

#[cfg(windows)]
fn set_string(key: &str, value: &str, data: &str) -> Result<(), FsError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    let wide_key: Vec<u16> = std::ffi::OsStr::new(key)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let wide_value: Vec<u16> = std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let wide_data: Vec<u16> = std::ffi::OsStr::new(data)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut hkey = HKEY::default();
    // SAFETY: buffers are NUL-terminated and outlive the calls; `hkey` is closed
    // on every path.
    unsafe {
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(wide_key.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        );
        if rc != ERROR_SUCCESS {
            return Err(FsError::new(
                "registry_failed",
                format!("RegCreateKeyExW {key}: {rc:?}"),
            ));
        }
        let bytes = std::slice::from_raw_parts(
            wide_data.as_ptr() as *const u8,
            wide_data.len() * 2,
        );
        let rc = RegSetValueExW(
            hkey,
            PCWSTR(wide_value.as_ptr()),
            None,
            REG_SZ,
            Some(bytes),
        );
        // Đóng handle là dọn dẹp: lỗi ở đây không làm hỏng dữ liệu đã ghi,
        // nên chỉ ghi log, không trả lỗi.
        let close_rc = RegCloseKey(hkey);
        if close_rc != ERROR_SUCCESS {
            // Rò handle là lỗi thật nhưng không làm hỏng dữ liệu vừa ghi;
            // ghi log để còn thấy chứ không nuốt im lặng.
            crate::log::warn(&format!("RegCloseKey {key}: {close_rc:?}"));
        }
        if rc != ERROR_SUCCESS {
            return Err(FsError::new(
                "registry_failed",
                format!("RegSetValueExW {key}\\{value}: {rc:?}"),
            ));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn set_string(_key: &str, _value: &str, _data: &str) -> Result<(), FsError> {
    Err(FsError::new("unsupported", "Registry chỉ có trên Windows"))
}

#[cfg(windows)]
fn delete_tree(key: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegDeleteTreeW, HKEY_CURRENT_USER,
    };

    let wide: Vec<u16> = std::ffi::OsStr::new(key)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: NUL-terminated key path; the buffer outlives the call.
    let rc = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(wide.as_ptr())) };
    // Khoá không tồn tại là chuyện bình thường khi gỡ lần đầu; mọi mã lỗi khác
    // nghĩa là gỡ chưa sạch và người dùng sẽ còn thấy app trong "Open with".
    if rc != ERROR_SUCCESS && rc != ERROR_FILE_NOT_FOUND {
        crate::log::warn(&format!("RegDeleteTreeW {key}: {rc:?}"));
    }
}

#[cfg(not(windows))]
fn delete_tree(_key: &str) {}

#[cfg(windows)]
fn delete_value(key: &str, value: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegDeleteKeyValueW, HKEY_CURRENT_USER,
    };

    let wide_key: Vec<u16> = std::ffi::OsStr::new(key)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let wide_value: Vec<u16> = std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: both paths are NUL-terminated; the buffers outlive the call.
    let rc = unsafe {
        RegDeleteKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(wide_key.as_ptr()),
            PCWSTR(wide_value.as_ptr()),
        )
    };
    // Giá trị không tồn tại là bình thường; lỗi khác thì phải thấy.
    if rc != ERROR_SUCCESS && rc != ERROR_FILE_NOT_FOUND {
        crate::log::warn(&format!("RegDeleteKeyValueW {key}\\{value}: {rc:?}"));
    }
}

#[cfg(not(windows))]
fn delete_value(_key: &str, _value: &str) {}

/// Báo Shell biết danh sách "Open with" đã đổi, để không phải đăng nhập lại.
#[cfg(windows)]
fn notify_shell() {
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    // SAFETY: documented call with no pointers.
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

#[cfg(not(windows))]
fn notify_shell() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_command_quotes_the_exe_path() {
        // "C:\Program Files" có dấu cách: thiếu ngoặc là Windows tách sai.
        let cmd = open_command(r"C:\Program Files\Files RS\files-rs.exe");
        assert_eq!(cmd, r#""C:\Program Files\Files RS\files-rs.exe" "%1""#);
    }

    #[test]
    fn the_open_command_quotes_the_placeholder() {
        // Tên thư mục cũng có thể chứa dấu cách, nên %1 phải nằm trong ngoặc.
        let cmd = open_command(r"C:\a.exe");
        assert!(cmd.ends_with(r#""%1""#), "phải kết thúc bằng \"%1\": {cmd}");
    }

    #[test]
    fn a_path_without_spaces_is_still_quoted() {
        // Đặt ngoặc luôn luôn cho đơn giản và an toàn hơn là đoán.
        assert!(open_command(r"C:\x\a.exe").starts_with('"'));
    }

    #[test]
    fn the_icon_reference_carries_the_index() {
        // Registry cần "đường dẫn,chỉ số"; thiếu ,0 là Windows không thấy icon.
        assert_eq!(icon_ref(r"C:\x\a.exe"), r"C:\x\a.exe,0");
    }

    #[test]
    fn progid_keys_live_under_hkcu_classes() {
        let key = progid_key(FOLDER_PROGID);
        assert_eq!(key, r"Software\Classes\FilesRS.Folder");
        assert!(key.starts_with(r"Software\Classes\"), "phải nằm dưới Classes");
        assert_eq!(progid_key(DRIVE_PROGID), r"Software\Classes\FilesRS.Drive");
    }

    #[test]
    fn every_registry_key_is_distinct() {
        // Hai ProgID trùng khoá thì thư mục và ổ đĩa sẽ đè lên nhau.
        let keys = [
            progid_key(FOLDER_PROGID),
            progid_key(DRIVE_PROGID),
            application_key(),
            capabilities_key(),
            format!(r"Software\{APP_NAME}"),
        ];
        let unique: std::collections::HashSet<&String> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len(), "có khoá trùng: {keys:?}");
    }

    #[test]
    fn the_application_key_names_the_executable() {
        // Windows tra khoá này theo tên file thực thi.
        assert!(application_key().ends_with("FilesRS.exe"));
    }

    #[test]
    fn capabilities_key_matches_the_registered_applications_value() {
        // RegisteredApplications trỏ tới đúng khoá capabilities; lệch là
        // Windows không tìm thấy danh sách khả năng.
        assert_eq!(capabilities_key(), r"Software\FilesRS\Capabilities");
    }
}
