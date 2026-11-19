//! Preview panel: the properties table plus an image/text preview.
//!
//! Files shows the same information in a separate Properties dialog; here it
//! lives in the pane so selecting a row always answers "what is this?" without
//! another window. Every field comes from the `Entry` the storage layer already
//! read, except the folder item count and the image dimensions, which need one
//! extra call and are therefore filled in only when they are cheap.

use crate::format;
use crate::{AppWindow, PropRow};
use files_core::{Entry, EntryKind};
use slint::{ModelRc, SharedString, VecModel};
use std::path::Path;

/// A folder with more entries than this is reported as "N+" instead of being
/// counted; counting a huge tree would block the UI thread.
use files_fs::FOLDER_COUNT_LIMIT;

fn set(ui: &AppWindow, kind: &str, title: &str) {
    ui.set_preview_kind(SharedString::from(kind));
    ui.set_preview_title(SharedString::from(title));
}

fn row(label: &str, value: String) -> PropRow {
    PropRow {
        label: SharedString::from(label),
        value: SharedString::from(value.as_str()),
    }
}

/// Build the properties table for one entry.
pub fn properties(entry: &Entry) -> Vec<PropRow> {
    let mut rows = Vec::new();
    rows.push(row("Tên", entry.name.clone()));
    rows.push(row("Loại", format::kind_label(entry)));

    if entry.is_dir() {
        rows.push(row("Kích thước", folder_size_text(entry)));
        rows.push(row("Số mục", folder_count_text(entry)));
    } else {
        rows.push(row("Kích thước", format::format_size(entry.size)));
    }

    // Với shortcut, đích là thông tin quan trọng nhất — người dùng cần biết
    // nó trỏ đi đâu trước khi mở.
    if entry
        .extension
        .as_deref()
        .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
    {
        let target = files_fs::shortcut_target(std::path::Path::new(&entry.path))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "không đọc được".to_string());
        rows.push(row("Trỏ tới", target));
    }

    rows.push(row(
        "Đường dẫn",
        Path::new(&entry.path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| entry.path.clone()),
    ));
    rows.push(row("Sửa lần cuối", format::format_time(entry.modified_ms)));
    rows.push(row("Tạo lúc", format::format_time(entry.created_ms)));
    rows.push(row(
        "Thuộc tính",
        match (entry.is_hidden, entry.is_readonly) {
            (true, true) => "Ẩn, Chỉ đọc".into(),
            (true, false) => "Ẩn".into(),
            (false, true) => "Chỉ đọc".into(),
            (false, false) => "Thường".into(),
        },
    ));
    rows
}

/// Folder size is not stored in the directory entry, so report it honestly
/// rather than showing a misleading `0 B`.
fn folder_size_text(_entry: &Entry) -> String {
    "—".into()
}

/// Số mục con được điền sau, từ luồng nền. Hiện "…" cho tới lúc có kết quả.
fn folder_count_text(_entry: &Entry) -> String {
    "…".into()
}

/// Định dạng số mục con khi luồng nền đã đếm xong.
pub fn folder_count_label(count: Option<usize>) -> String {
    match count {
        Some(n) if n > FOLDER_COUNT_LIMIT => format!("{FOLDER_COUNT_LIMIT}+"),
        Some(n) => n.to_string(),
        None => "—".into(),
    }
}

/// Loại dữ liệu xem trước cần cho `entry`.
pub fn kind_for(entry: &Entry) -> files_fs::PreviewKind {
    files_fs::PreviewKind::for_entry(
        entry.extension.as_deref(),
        entry.size,
        entry.kind == EntryKind::Directory,
    )
}

/// Cập nhật panel với thông tin đã có sẵn trong `Entry`.
///
/// Không chạm đĩa: ảnh, văn bản và số mục con do luồng nền điền sau qua
/// [`apply_data`]. Nhờ vậy chọn một dòng trong thư mục mạng không làm đứng cửa
/// sổ.
pub fn show(ui: &AppWindow, entry: &Entry) {
    ui.set_props(ModelRc::new(VecModel::from(properties(entry))));
    set(ui, "info", &entry.name);
}

/// Điền kết quả đọc nền vào panel.
pub fn apply_data(ui: &AppWindow, entry: &Entry, data: files_fs::PreviewData) {
    match data {
        files_fs::PreviewData::FolderCount(count) => {
            let mut rows = properties(entry);
            for row in rows.iter_mut() {
                if row.label == "Số mục" {
                    row.value = SharedString::from(folder_count_label(count).as_str());
                }
            }
            ui.set_props(ModelRc::new(VecModel::from(rows)));
            set(ui, "info", &entry.name);
        }
        files_fs::PreviewData::Image(icon) => {
            ui.set_preview_image(crate::view::to_slint_image(&icon));
            set(ui, "image", &entry.name);
        }
        files_fs::PreviewData::Text(text) => {
            ui.set_preview_text(SharedString::from(text.as_str()));
            set(ui, "text", &entry.name);
        }
        files_fs::PreviewData::None => {}
    }
}

pub fn clear(ui: &AppWindow) {
    set(ui, "none", "");
    ui.set_props(ModelRc::new(VecModel::from(Vec::<PropRow>::new())));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_entry() -> Entry {
        Entry {
            name: "notes.txt".into(),
            path: r"C:\Users\me\Documents\notes.txt".into(),
            kind: EntryKind::File,
            size: Some(2048),
            modified_ms: Some(1_700_000_000_000),
            created_ms: Some(1_600_000_000_000),
            extension: Some("txt".into()),
            is_hidden: false,
            is_readonly: false,
        }
    }

    #[test]
    fn a_file_reports_size_but_not_an_item_count() {
        let rows = properties(&file_entry());
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert!(labels.contains(&"Kích thước"));
        assert!(
            !labels.contains(&"Số mục"),
            "only folders have an item count"
        );
        let size = rows.iter().find(|r| r.label == "Kích thước").unwrap();
        assert_eq!(size.value.as_str(), "2.0 KB");
    }

    #[test]
    fn a_file_reports_its_parent_folder_not_itself() {
        let rows = properties(&file_entry());
        let path = rows.iter().find(|r| r.label == "Đường dẫn").unwrap();
        assert_eq!(path.value.as_str(), r"C:\Users\me\Documents");
    }

    #[test]
    fn attributes_combine_hidden_and_readonly() {
        let mut e = file_entry();
        e.is_hidden = true;
        e.is_readonly = true;
        let rows = properties(&e);
        let attr = rows.iter().find(|r| r.label == "Thuộc tính").unwrap();
        assert_eq!(attr.value.as_str(), "Ẩn, Chỉ đọc");
    }

    #[test]
    fn an_unknown_timestamp_renders_empty() {
        let mut e = file_entry();
        e.modified_ms = None;
        e.created_ms = None;
        let rows = properties(&e);
        assert_eq!(
            rows.iter()
                .find(|r| r.label == "Sửa lần cuối")
                .unwrap()
                .value
                .as_str(),
            ""
        );
        assert_eq!(
            rows.iter()
                .find(|r| r.label == "Tạo lúc")
                .unwrap()
                .value
                .as_str(),
            ""
        );
    }

    #[test]
    fn a_folder_never_claims_a_zero_byte_size() {
        let e = Entry {
            name: "Documents".into(),
            path: r"C:\Users\me\Documents".into(),
            kind: EntryKind::Directory,
            size: None,
            modified_ms: None,
            created_ms: None,
            extension: None,
            is_hidden: false,
            is_readonly: false,
        };
        let rows = properties(&e);
        let size = rows.iter().find(|r| r.label == "Kích thước").unwrap();
        assert_eq!(
            size.value.as_str(),
            "—",
            "unknown is shown as a dash, not 0 B"
        );
        assert!(rows.iter().any(|r| r.label == "Số mục"));
    }
}
