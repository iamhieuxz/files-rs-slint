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
use slint::{Image, ModelRc, SharedString, VecModel};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "ico"];
const TEXT_EXTS: &[&str] = &[
    "txt", "md", "rs", "toml", "json", "log", "csv", "ini", "cfg", "yaml", "yml", "xml", "html",
    "css", "js", "ts", "py", "bat", "ps1", "cmd", "sh",
];
const TEXT_LIMIT: u64 = 16 * 1024;
/// A folder with more entries than this is reported as "N+" instead of being
/// counted; counting a huge tree would block the UI thread.
const FOLDER_COUNT_LIMIT: usize = 5000;

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

fn folder_count_text(entry: &Entry) -> String {
    match std::fs::read_dir(&entry.path) {
        Ok(iter) => {
            let mut n = 0usize;
            for _ in iter.take(FOLDER_COUNT_LIMIT + 1) {
                n += 1;
            }
            if n > FOLDER_COUNT_LIMIT {
                format!("{FOLDER_COUNT_LIMIT}+")
            } else {
                n.to_string()
            }
        }
        Err(_) => "—".into(),
    }
}

/// Update the panel for the selected entry.
pub fn show(ui: &AppWindow, entry: &Entry) {
    ui.set_props(ModelRc::new(VecModel::from(properties(entry))));
    set(ui, "info", &entry.name);

    if entry.kind == EntryKind::Directory {
        return;
    }

    let ext = entry
        .extension
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let path = Path::new(&entry.path);

    if IMAGE_EXTS.contains(&ext.as_str()) {
        if let Ok(img) = Image::load_from_path(path) {
            ui.set_preview_image(img);
            set(ui, "image", &entry.name);
        }
        return;
    }

    if TEXT_EXTS.contains(&ext.as_str()) && entry.size.unwrap_or(0) <= 4 * TEXT_LIMIT {
        let mut buf = Vec::new();
        let read = File::open(path).and_then(|f| f.take(TEXT_LIMIT).read_to_end(&mut buf));
        if read.is_ok() {
            ui.set_preview_text(SharedString::from(String::from_utf8_lossy(&buf).as_ref()));
            set(ui, "text", &entry.name);
        }
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
