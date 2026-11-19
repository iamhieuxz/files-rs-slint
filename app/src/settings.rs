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
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Thứ tự mặc định của các cột trong details view.
pub const COLUMN_IDS: [&str; 6] = ["name", "tag", "modified", "kind", "git", "size"];

pub fn default_column_order() -> Vec<String> {
    COLUMN_IDS.iter().map(|s| s.to_string()).collect()
}

pub fn column_default_width(id: &str) -> u32 {
    match id {
        "name" => 240,
        "tag" => 100,
        "modified" => 140,
        "kind" => 100,
        "size" => 90,
        _ => 16,
    }
}

/// Giới hạn kéo cột. Cột git là chữ cái trạng thái nên cố định.
pub fn column_bounds(id: &str) -> (u32, u32) {
    match id {
        "name" => (120, 600),
        "tag" => (70, 220),
        "modified" => (100, 240),
        "kind" => (70, 200),
        "size" => (70, 200),
        _ => (16, 16),
    }
}

/// Sửa thứ tự cột bị hỏng trong settings: bỏ id lạ/trùng, thêm cột thiếu.
pub fn sanitize_columns(
    order: Vec<String>,
    widths: BTreeMap<String, u32>,
) -> (Vec<String>, BTreeMap<String, u32>) {
    let mut clean = Vec::with_capacity(COLUMN_IDS.len());
    for id in order {
        if COLUMN_IDS.contains(&id.as_str()) && !clean.contains(&id) {
            clean.push(id);
        }
    }
    for id in COLUMN_IDS {
        if !clean.iter().any(|c| c == id) {
            clean.push(id.to_string());
        }
    }

    let mut clean_widths = BTreeMap::new();
    for id in COLUMN_IDS {
        let (min, max) = column_bounds(id);
        let width = widths
            .get(id)
            .copied()
            .unwrap_or_else(|| column_default_width(id))
            .clamp(min, max);
        clean_widths.insert(id.to_string(), width);
    }
    (clean, clean_widths)
}

/// Dời một cột sang trái/phải một bậc. Trả về true nếu thứ tự đổi.
pub fn move_column(order: &mut [String], id: &str, direction: i32) -> bool {
    let Some(index) = order.iter().position(|c| c == id) else {
        return false;
    };
    let target = index as i32 + direction;
    if target < 0 || target as usize >= order.len() {
        return false;
    }
    order.swap(index, target as usize);
    true
}

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
    /// Thư mục người dùng ghim vào sidebar, theo thứ tự thêm.
    pub pinned_folders: Vec<String>,
    /// Thứ tự cột details view, kéo được như Excel.
    pub column_order: Vec<String>,
    /// Bề rộng từng cột theo id.
    pub column_widths: BTreeMap<String, u32>,
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
            pinned_folders: Vec::new(),
            column_order: default_column_order(),
            column_widths: COLUMN_IDS
                .iter()
                .map(|id| (id.to_string(), column_default_width(id)))
                .collect(),
        }
    }
}

impl Settings {
    /// Clamp values that could break the UI if the file were edited by hand.
    pub fn sanitized(mut self) -> Self {
        if self.view_mode != 0 && self.view_mode != 1 {
            self.view_mode = 0;
        }
        let (order, widths) = sanitize_columns(self.column_order, self.column_widths);
        self.column_order = order;
        self.column_widths = widths;
        self
    }

    /// Keep shared data from `self` (the file) while taking appearance choices
    /// from the window that is saving.
    ///
    /// Pins belong to the file because every window can add them; overwriting
    /// them with a stale snapshot silently removes another window's work.
    pub fn with_ui_from(mut self, current: &Settings) -> Self {
        self.dark = current.dark;
        self.view_mode = current.view_mode;
        self.show_sidebar = current.show_sidebar;
        self.show_preview = current.show_preview;
        self.show_hidden = current.show_hidden;
        self.show_extensions = current.show_extensions;
        self.folders_first = current.folders_first;
        self.column_order = current.column_order.clone();
        self.column_widths = current.column_widths.clone();
        self
    }

    /// Keep the file's shared preferences while taking a fresh pin list from
    /// the window that just changed pins.
    pub fn with_pins_from(mut self, current: &Settings) -> Self {
        self.pinned_folders = current.pinned_folders.clone();
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
        // File rỗng không phải dữ liệu hỏng: `state::settings_path()` tạo sẵn
        // file 0 byte để kiểm tra quyền ghi. Đưa nó vào `.corrupt` sẽ xoá mất
        // đường dẫn thật và sinh file rác ở mỗi lần khởi động.
        if text.trim().is_empty() {
            return Self::default();
        }
        match serde_json::from_str::<Self>(&text) {
            Ok(s) => s.sanitized(),
            Err(_) => {
                // Giữ bản hỏng để người dùng cứu được, trước khi lần lưu sau
                // ghi đè bằng mặc định.
                crate::persist::quarantine(path);
                Self::default()
            }
        }
    }

    /// Write preferences, creating the folder if needed.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        crate::persist::write_atomic(path, &text)
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
    fn saving_ui_keeps_pins_added_by_another_window() {
        // Hai cửa sổ dùng chung một file. Cửa sổ A ghim thêm thư mục; cửa sổ B
        // đổi theme rồi lưu — không được ghi đè danh sách ghim bằng bản cũ.
        let disk = Settings {
            pinned_folders: vec![r"C:\A".into()],
            ..Settings::default()
        };
        let current = Settings {
            dark: true,
            ..Settings::default()
        };

        let merged = disk.with_ui_from(&current);

        assert!(
            merged.dark,
            "lựa chọn giao diện của cửa sổ đang lưu phải thắng"
        );
        assert_eq!(merged.pinned_folders, vec![r"C:\A".to_string()]);
    }

    #[test]
    fn saving_pins_keeps_shared_preferences_from_disk() {
        let disk = Settings {
            dark: true,
            view_mode: 1,
            ..Settings::default()
        };
        let current = Settings {
            pinned_folders: vec![r"C:\Moi".into()],
            ..Settings::default()
        };

        let merged = disk.with_pins_from(&current);

        assert_eq!(merged.pinned_folders, vec![r"C:\Moi".to_string()]);
        assert!(merged.dark, "theme của cửa sổ khác không bị ghi đè");
        assert_eq!(merged.view_mode, 1);
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
        let s = Settings {
            dark: true,
            ..Settings::default()
        };
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
            pinned_folders: vec![r"C:\Users\x\Documents".into()],
            column_order: default_column_order(),
            column_widths: Settings::default().column_widths,
        };
        s.save(&path).expect("ghi được");
        assert_eq!(Settings::load(&path), s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn column_order_can_be_moved_but_not_past_the_edges() {
        let mut order = default_column_order();
        assert!(move_column(&mut order, "name", 1));
        assert_eq!(order[0], "tag");
        assert_eq!(order[1], "name");
        assert!(!move_column(&mut order, "tag", -1), "đã ở đầu");
        assert!(!move_column(&mut order, "missing", 1));
        assert!(!move_column(&mut order, "size", 1), "đã ở cuối");
    }

    #[test]
    fn a_broken_column_layout_is_repaired_and_clamped() {
        let (order, widths) = sanitize_columns(
            vec!["size".into(), "bogus".into(), "name".into(), "size".into()],
            BTreeMap::from([
                ("name".to_string(), 5),
                ("size".to_string(), 999),
                ("bogus".to_string(), 50),
            ]),
        );
        assert_eq!(order[..2], ["size", "name"]);
        assert_eq!(order.len(), COLUMN_IDS.len());
        assert!(!order.iter().any(|c| c == "bogus"));
        assert_eq!(widths["name"], 120, "kẹp về min");
        assert_eq!(widths["size"], 200, "kẹp về max");
        assert_eq!(widths["tag"], 100, "cột thiếu lấy mặc định");
        assert!(!widths.contains_key("bogus"));
    }

    #[test]
    fn pinned_folders_survive_a_restart() {
        // Ghim nam trong settings.json nen phai doc lai duoc.
        let dir = scratch("settings-pins");
        let path = dir.join("settings.json");
        let s = Settings {
            pinned_folders: vec![r"C:\A".into(), r"D:\B".into()],
            ..Settings::default()
        };
        s.save(&path).expect("ghi được");

        let back = Settings::load(&path);
        assert_eq!(
            back.pinned_folders,
            vec![r"C:\A".to_string(), r"D:\B".to_string()]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fresh_install_has_no_pins() {
        assert!(Settings::default().pinned_folders.is_empty());
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
    fn an_empty_file_is_not_treated_as_corrupt() {
        // `state::settings_path()` tạo sẵn file 0 byte để kiểm tra quyền ghi.
        // Coi nó là hỏng sẽ đổi tên đường dẫn thật thành `.corrupt` ngay lần
        // khởi động đầu tiên.
        let dir = scratch("settings-empty");
        let path = dir.join("settings.json");
        std::fs::File::create(&path).unwrap();

        assert_eq!(Settings::load(&path), Settings::default());
        assert!(path.exists(), "file rỗng phải được giữ nguyên tại chỗ");
        assert!(
            !dir.join("settings.json.corrupt").exists(),
            "không được sinh file .corrupt từ file rỗng"
        );
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
