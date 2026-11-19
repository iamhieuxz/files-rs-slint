//! "Open with" handlers for a file, using the same Shell data Explorer uses.
//!
//! Instead of guessing registry keys, this asks the Shell for the handlers
//! registered for the file's extension and lets the user invoke one. When
//! listing fails, the caller can fall back to the system Open With dialog.

use files_core::FsError;
use std::path::Path;

/// Một ứng dụng có thể mở loại tệp này.
pub struct OpenWithApp {
    name: String,
    #[cfg(windows)]
    handler: windows::Win32::UI::Shell::IAssocHandler,
}

impl OpenWithApp {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Mở `path` bằng ứng dụng này.
    pub fn invoke(&self, path: &Path) -> Result<(), FsError> {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows::core::PCWSTR;
            use windows::Win32::System::Com::{
                CoInitializeEx, IDataObject, COINIT_APARTMENTTHREADED,
            };
            use windows::Win32::UI::Shell::{
                BHID_DataObject, IShellItem, SHCreateItemFromParsingName,
            };

            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            // SAFETY: COM được khởi tạo trên luồng UI; buffer NUL-terminated
            // sống qua các lời gọi; item/data là interface do Shell cấp.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)
                    .map_err(|e| {
                        FsError::new(
                            "open_with_failed",
                            format!("Không tạo được shell item: {e}"),
                        )
                    })?;
                let data: IDataObject =
                    item.BindToHandler(None, &BHID_DataObject).map_err(|e| {
                        FsError::new(
                            "open_with_failed",
                            format!("Không tạo được dữ liệu shell: {e}"),
                        )
                    })?;
                self.handler.Invoke(&data).map_err(|e| {
                    FsError::new("open_with_failed", format!("Ứng dụng không mở được: {e}"))
                })
            }
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            Err(FsError::new("unsupported", "Open with chỉ có trên Windows"))
        }
    }
}

/// Các ứng dụng đăng ký mở loại tệp của `path`.
#[cfg(windows)]
pub fn open_with_apps(path: &Path) -> Result<Vec<OpenWithApp>, FsError> {
    use std::collections::HashSet;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{IAssocHandler, SHAssocEnumHandlers, ASSOC_FILTER_NONE};

    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()));
    let Some(extension) = extension else {
        return Ok(Vec::new());
    };
    let wide: Vec<u16> = extension.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe fn take_pwstr(value: PWSTR) -> String {
        let text = value.to_string().unwrap_or_default();
        CoTaskMemFree(Some(value.0 as *const _));
        text
    }

    // SAFETY: COM được khởi tạo trên luồng gọi; `wide` NUL-terminated và sống
    // qua lời gọi; các interface do Shell cấp và được thả bởi Rust.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let enumerator =
            SHAssocEnumHandlers(PCWSTR(wide.as_ptr()), ASSOC_FILTER_NONE).map_err(|e| {
                FsError::new("open_with_failed", format!("Không lấy được handler: {e}"))
            })?;
        let mut apps: Vec<OpenWithApp> = Vec::new();
        let mut seen = HashSet::new();
        loop {
            let mut batch: [Option<IAssocHandler>; 1] = [None];
            if enumerator.Next(&mut batch, None).is_err() {
                break;
            }
            let Some(handler) = batch[0].take() else {
                break;
            };
            let name = handler
                .GetUIName()
                .ok()
                .map(|name| take_pwstr(name))
                .filter(|name| !name.is_empty())
                .or_else(|| {
                    handler
                        .GetName()
                        .ok()
                        .map(|name| take_pwstr(name))
                        .filter(|name| !name.is_empty())
                });
            let Some(name) = name else { continue };
            if !seen.insert(name.to_lowercase()) {
                continue;
            }
            apps.push(OpenWithApp { name, handler });
            if apps.len() >= 16 {
                break;
            }
        }
        Ok(apps)
    }
}

#[cfg(not(windows))]
pub fn open_with_apps(_path: &Path) -> Result<Vec<OpenWithApp>, FsError> {
    Ok(Vec::new())
}

/// Hộp thoại "Open with" chuẩn của Windows, dùng khi không liệt kê được app.
#[cfg(windows)]
pub fn open_with_dialog(path: &Path) -> Result<(), FsError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        SHOpenWithDialog, OAIF_ALLOW_REGISTRATION, OAIF_EXEC, OPENASINFO,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let info = OPENASINFO {
        pcszFile: PCWSTR(wide.as_ptr()),
        pcszClass: PCWSTR::null(),
        oaifInFlags: OAIF_ALLOW_REGISTRATION | OAIF_EXEC,
    };
    // SAFETY: `wide` và `info` sống qua lời gọi; hwnd cha là null (cửa sổ top-level).
    unsafe { SHOpenWithDialog(None, &info) }.map_err(|e| {
        FsError::new(
            "open_with_failed",
            format!("Không mở được hộp thoại Open with: {e}"),
        )
    })
}

#[cfg(not(windows))]
pub fn open_with_dialog(_path: &Path) -> Result<(), FsError> {
    Err(FsError::new("unsupported", "Open with chỉ có trên Windows"))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_text_file_has_at_least_one_open_with_handler() {
        // Windows luôn đăng ký Notepad cho .txt; nếu Shell không trả gì thì
        // tính năng Open with đã hỏng ở tầng hệ thống.
        let apps = open_with_apps(Path::new("probe.txt")).expect("liệt kê được handler");
        assert!(!apps.is_empty(), "phải có ít nhất một ứng dụng mở .txt");
        assert!(apps.iter().any(|a| !a.name().is_empty()));
    }
}
