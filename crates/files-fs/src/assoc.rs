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

/// Đăng ký app làm một lựa chọn "mở bằng" cho thư mục và ổ đĩa.
///
/// Sau khi chạy, người dùng thấy "Files RS" trong menu chuột phải → Open with,
/// và có thể chọn nó làm mặc định trong Settings.
pub fn register(exe: &Path) -> Result<(), FsError> {
    let exe = std::path::absolute(exe).map_err(FsError::from)?;
    let exe_str = exe.to_string_lossy();
    let icon = format!("{exe_str},0");

    for (progid, kind) in [
        (FOLDER_PROGID, "Thư mục"),
        (DRIVE_PROGID, "Ổ đĩa"),
    ] {
        let base = format!(r"Software\Classes\{progid}");
        set_string(&format!(r"{base}\DefaultIcon"), "", &icon)?;
        set_string(&format!(r"{base}\shell\open\command"), "", &format!("\"{exe_str}\" \"%1\""))?;
        set_string(&base, "", kind)?;
        // Cho phép app xuất hiện trong danh sách "Open with".
        set_string(&format!(r"{base}\shell"), "", "")?;
    }

    // Đăng ký ứng dụng để nó có tên hiển thị trong Settings.
    let app_key = format!(r"Software\Classes\Applications\{APP_NAME}.exe");
    set_string(&app_key, "FriendlyAppName", "Files RS")?;
    set_string(
        &format!(r"{app_key}\shell\open\command"),
        "",
        &format!("\"{exe_str}\" \"%1\""),
    )?;
    // Capabilities: danh sách loại nội dung app nhận mở.
    let caps = format!(r"Software\FilesRS\Capabilities");
    set_string(&caps, "ApplicationName", "Files RS")?;
    set_string(&caps, "ApplicationDescription", "Trình quản lý tệp cho Windows")?;
    set_string(&format!(r"{caps}\FileAssociations"), ".zip", "CompressedFolder")?;
    // Đăng ký khả năng để Windows hiện app trong "Choose a default".
    set_string(
        r"Software\RegisteredApplications",
        APP_NAME,
        r"Software\FilesRS\Capabilities",
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
        let _ = RegCloseKey(hkey);
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
    use windows::Win32::System::Registry::{
        RegDeleteTreeW, HKEY_CURRENT_USER,
    };

    let wide: Vec<u16> = std::ffi::OsStr::new(key)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: NUL-terminated key path; failure (missing key) is ignored.
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(wide.as_ptr()));
    }
}

#[cfg(not(windows))]
fn delete_tree(_key: &str) {}

#[cfg(windows)]
fn delete_value(key: &str, value: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
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
    // SAFETY: both paths are NUL-terminated; a missing value is not an error.
    unsafe {
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(wide_key.as_ptr()), PCWSTR(wide_value.as_ptr()));
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
