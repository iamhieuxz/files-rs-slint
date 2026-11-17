use crate::AppWindow;
use files_core::{Entry, EntryKind};
use slint::{Image, SharedString};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "ico"];
const TEXT_EXTS: &[&str] = &[
    "txt", "md", "rs", "toml", "json", "log", "csv", "ini", "cfg", "yaml", "yml", "xml", "html",
    "css", "js", "ts", "py", "bat", "ps1", "cmd", "sh",
];
const TEXT_LIMIT: u64 = 16 * 1024;

fn set(ui: &AppWindow, kind: &str, title: &str, meta: &str) {
    ui.set_preview_kind(SharedString::from(kind));
    ui.set_preview_title(SharedString::from(title));
    ui.set_preview_meta(SharedString::from(meta));
}

fn describe(entry: &Entry) -> String {
    let size = match entry.size {
        Some(b) if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / 1024.0 / 1024.0),
        Some(b) if b >= 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        Some(b) => format!("{b} B"),
        None => "—".into(),
    };
    format!("{} · {}", entry.path, size)
}

/// Cập nhật panel preview theo tệp được chọn.
/// Ảnh: nạp trực tiếp bằng Slint. Văn bản: đọc tối đa 16 KB. Còn lại: chỉ hiện thông tin.
pub fn show(ui: &AppWindow, entry: &Entry) {
    if entry.kind == EntryKind::Directory {
        set(ui, "info", &entry.name, &format!("Thư mục · {}", entry.path));
        return;
    }

    let ext = entry.extension.as_deref().unwrap_or("").to_ascii_lowercase();
    let path = Path::new(&entry.path);

    if IMAGE_EXTS.contains(&ext.as_str()) {
        match Image::load_from_path(path) {
            Ok(img) => {
                eprintln!("[preview] loaded image: {} ({}x{})", entry.path, img.size().width, img.size().height);
                ui.set_preview_image(img);
                set(ui, "image", &entry.name, &describe(entry));
            }
            Err(e) => {
                eprintln!("[preview] FAILED image: {} -> {e:?}", entry.path);
                set(ui, "info", &entry.name, &format!("Không đọc được ảnh · {}", describe(entry)));
            }
        }
        return;
    }

    if TEXT_EXTS.contains(&ext.as_str()) && entry.size.unwrap_or(0) <= 4 * TEXT_LIMIT {
        let mut buf = Vec::new();
        let read = File::open(path).and_then(|f| f.take(TEXT_LIMIT).read_to_end(&mut buf));
        match read {
            Ok(_) => {
                ui.set_preview_text(SharedString::from(String::from_utf8_lossy(&buf).as_ref()));
                set(ui, "text", &entry.name, &describe(entry));
            }
            Err(e) => set(ui, "info", &entry.name, &format!("Không đọc được tệp: {e}")),
        }
        return;
    }

    set(ui, "info", &entry.name, &describe(entry));
}

pub fn clear(ui: &AppWindow) {
    set(ui, "none", "", "");
}
