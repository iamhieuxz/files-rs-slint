//! Persisted user preferences.
//!
//! Without this the app forgets every choice on restart: the theme flips back to
//! light, the list returns to details view, side panels reappear. The user has
//! to redo the same clicks every launch, which reads as a bug.
//!
//! One small JSON file, written after each change. Only appearance and layout
//! live here — never the current folder or open tabs, which belong to the tab
//! model and would surprise a user who expects a clean start.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Everything worth remembering between sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Dark theme.
    pub dark: bool,
    /// 0 = details list, 1 = grid.
    pub view_mode: i32,
    pub show_sidebar: bool,
    pub show_preview: bool,
    pub show_hidden: bool,
    pub show_extensions: bool,
    pub folders_first: bool,
}

impl Default for Settings {
    fn default() -> Self {
        // Defaults match a first run: light theme, details list, everything on.
        Self {
            dark: false,
            view_mode: 0,
            show_sidebar: true,
            show_preview: true,
            show_hidden: false,
            show_extensions: true,
            folders_first: true,
        }
    }
}

impl Settings {
    /// Clamp values that could break the UI if the file were edited by hand.
    pub fn sanitized(mut self) -> Self {
        if self.view_mode != 0 && self.view_mode != 1 {
            self.view_mode = 0;
        }
        self
    }

    /// Where preferences live: beside the tag database.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("FilesRS").join("settings.json")
    }

    /// Read preferences, falling back to defaults.
    ///
    /// A missing or corrupt file must never block startup: the user gets a
    /// working app with default looks instead of an error screen.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str::<Self>(&text)
            .map(Self::sanitized)
            .unwrap_or_default()
    }

    /// Write preferences, creating the folder if needed.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let exe = std::env::current_exe().expect("đường dẫn exe");
        let dir = exe
            .parent()
            .expect("thư mục chứa exe")
            .join("test-scratch")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tạo thư mục tạm");
        dir
    }

    #[test]
    fn a_fresh_install_starts_light_with_panels_open() {
        let s = Settings::default();
        assert!(!s.dark, "lần đầu phải là nền sáng");
        assert_eq!(s.view_mode, 0);
        assert!(s.show_sidebar && s.show_preview);
        assert!(s.show_extensions);
    }

    #[test]
    fn dark_mode_survives_a_restart() {
        // Đây chính là lỗi người dùng báo: bật nền tối, tắt app, mở lại thì
        // quay về nền sáng.
        let dir = scratch("settings-dark");
        let path = dir.join("settings.json");
        let s = Settings { dark: true, ..Settings::default() };
        s.save(&path).expect("ghi được");

        let back = Settings::load(&path);
        assert!(back.dark, "phải nhớ nền tối");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_field_round_trips() {
        let dir = scratch("settings-all");
        let path = dir.join("settings.json");
        let s = Settings {
            dark: true,
            view_mode: 1,
            show_sidebar: false,
            show_preview: false,
            show_hidden: true,
            show_extensions: false,
            folders_first: false,
        };
        s.save(&path).expect("ghi được");
        assert_eq!(Settings::load(&path), s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_gives_defaults() {
        let s = Settings::load(Path::new(r"C:\khong\co\settings.json"));
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn a_corrupt_file_gives_defaults_instead_of_panicking() {
        let dir = scratch("settings-corrupt");
        let path = dir.join("settings.json");
        std::fs::write(&path, "không phải json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_missing_fields_fills_them_from_defaults() {
        // Người dùng nâng cấp từ bản cũ: thiếu khoá mới không được làm hỏng.
        let dir = scratch("settings-partial");
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{ "dark": true }"#).unwrap();
        let s = Settings::load(&path);
        assert!(s.dark, "giữ giá trị có trong file");
        assert_eq!(s.view_mode, 0, "khoá thiếu lấy mặc định");
        assert!(s.show_sidebar);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_out_of_range_view_mode_is_clamped() {
        let dir = scratch("settings-clamp");
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{ "view_mode": 7 }"#).unwrap();
        assert_eq!(Settings::load(&path).view_mode, 0, "về danh sách");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
