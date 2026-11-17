//! Row and sidebar presentation: turning model values into the strings and
//! glyph names the UI renders.
//!
//! Formatting lives here rather than in `main.rs` so the mapping is testable:
//! "which icon does a `.pdf` get" is a rule, not a rendering detail.

use files_core::{Entry, EntryKind};

/// Fluent icon name + tint for one entry.
///
/// The colour groups match Files' `ThemedIcon` conventions: folders amber,
/// documents blue, code purple, media orange, archives pink. Executables and
/// installers get their own group so an app is recognisable at a glance.
pub fn icon_for(entry: &Entry) -> (&'static str, u32) {
    if entry.is_dir() {
        return ("folder", 0xd8a838);
    }
    let ext = entry.extension.as_deref().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        // ---- ảnh ----
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "svg" | "tif" | "tiff"
        | "heic" | "avif" | "raw" | "psd" => ("image", 0x34a853),
        // ---- video ----
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" | "flv" | "m4v" | "mpg" | "mpeg" => {
            ("video", 0xfb923c)
        }
        // ---- âm thanh ----
        "mp3" | "wav" | "flac" | "m4a" | "ogg" | "aac" | "wma" | "opus" => {
            ("music_note", 0xd9534f)
        }
        // ---- nén ----
        "zip" | "7z" | "rar" | "tar" | "gz" | "bz2" | "xz" | "cab" => {
            ("folder_zip", 0xf472b6)
        }
        // ---- mã nguồn ----
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "c" | "cpp" | "h" | "hpp" | "cs" | "go"
        | "java" | "kt" | "rb" | "php" | "swift" | "lua" | "json" | "toml" | "yaml" | "yml"
        | "xml" | "html" | "css" | "scss" | "sql" | "sh" | "ps1" | "bat" | "cmd" => {
            ("code", 0xc084fc)
        }
        // ---- tài liệu ----
        "doc" | "docx" | "rtf" | "odt" | "pages" => ("document", 0x4a90d9),
        "xls" | "xlsx" | "csv" | "ods" | "numbers" => ("table", 0x2aa36b),
        "ppt" | "pptx" | "odp" | "key" => ("slide_layout", 0xe0703d),
        "pdf" => ("document", 0xd9534f),
        "txt" | "md" | "log" => ("document", 0x8a8f98),
        // ---- ứng dụng và trình cài đặt ----
        // Một tệp .exe là một ứng dụng: dùng icon "apps" thay vì icon tệp chung,
        // để hàng trăm tệp trong Program Files không trông giống nhau.
        "exe" | "com" | "appx" | "msix" => ("apps", 0x2f86d4),
        "msi" | "msp" => ("box", 0x8b6bd9),
        "dll" | "sys" | "drv" => ("puzzle_piece", 0x8a8f98),
        "lnk" | "url" => ("link", 0x4a90d9),
        // ---- dữ liệu và cấu hình ----
        "db" | "sqlite" | "sqlite3" | "mdb" | "accdb" => ("database", 0x2aa36b),
        "ini" | "cfg" | "conf" | "reg" => ("settings", 0x8a8f98),
        "ttf" | "otf" | "woff" | "woff2" => ("text_bullet_list", 0x8b6bd9),
        "iso" | "vhd" | "vhdx" | "img" => ("hard_drive", 0x6b7280),
        _ => ("document", 0x8a8f98),
    }
}

/// Human label for the "Loại" column.
pub fn kind_label(entry: &Entry) -> String {
    match entry.kind {
        EntryKind::Directory => "Thư mục".into(),
        EntryKind::Symlink => "Liên kết".into(),
        EntryKind::Other => "Khác".into(),
        EntryKind::File => match entry.extension.as_ref() {
            Some(ext) => format!("Tệp {}", ext.to_uppercase()),
            None => "Tệp".into(),
        },
    }
}

/// 1.4 KB / 27.3 MB / 1.20 GB, empty for directories.
pub fn format_size(bytes: Option<u64>) -> String {
    match bytes {
        None => String::new(),
        Some(b) if b < 1024 => format!("{b} B"),
        Some(b) if b < 1024 * 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        Some(b) if b < 1024 * 1024 * 1024 => format!("{:.1} MB", b as f64 / 1024.0 / 1024.0),
        Some(b) => format!("{:.2} GB", b as f64 / 1024.0 / 1024.0 / 1024.0),
    }
}

/// `dd/MM/yyyy HH:mm` in local time; empty when the timestamp is unknown.
pub fn format_time(ms: Option<i64>) -> String {
    match ms.and_then(chrono::DateTime::from_timestamp_millis) {
        Some(dt) => dt
            .with_timezone(&chrono::Local)
            .format("%d/%m/%Y %H:%M")
            .to_string(),
        None => String::new(),
    }
}

/// A message worth showing to the user for `err`.
///
/// `io::Error`'s own text ("Access is denied. (os error 5)") says what Windows
/// refused but not what the person can do about it. The common cases get an
/// actionable sentence; everything else keeps the original detail so a real bug
/// is still reportable.
pub fn explain_error(code: &str, message: &str) -> String {
    match code {
        "access_denied" => format!(
            "Không có quyền ghi vào thư mục này. Cách xử lý: chọn thư mục khác, \
             hoặc chuột phải tệp .exe của ứng dụng → Run as administrator. ({message})"
        ),
        "not_found" => format!(
            "Không tìm thấy tệp hoặc thư mục — có thể nó vừa bị di chuyển hoặc xoá. ({message})"
        ),
        "invalid_name" => format!(
            "Tên không hợp lệ: không được để trống và không chứa các ký tự \\ / : * ? \" < > |. ({message})"
        ),
        _ => message.to_string(),
    }
}

/// Name shown in the list: the raw name, or without its extension when the user
/// turned extensions off. A folder is never stripped, and a name that is *only*
/// an extension (`.gitignore`) is left alone so it does not become empty.
pub fn display_name(name: &str, is_dir: bool, show_extensions: bool) -> String {
    if show_extensions || is_dir {
        return name.to_string();
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => stem.to_string(),
        _ => name.to_string(),
    }
}

/// Colour for a pinned folder in the sidebar, matched on the folder name.
pub fn folder_glyph(name: &str) -> (&'static str, u32) {
    match name {
        "Home" => ("home", 0xe8a33d),
        "Desktop" => ("desktop", 0x2f86d4),
        "Downloads" => ("arrow_download", 0x2aa36b),
        "Documents" => ("document", 0x4a90d9),
        "Pictures" => ("image", 0x3aa6a6),
        "Music" => ("music_note", 0xd9534f),
        "Videos" => ("video", 0x8b6bd9),
        _ => ("folder", 0xd8a838),
    }
}

/// Natural width of each breadcrumb segment.
///
/// Slint cannot size a `Text` from its own content inside a layout, so the
/// address bar asks for the widths up front: every segment gets a per-character
/// estimate, clamped so a long folder name cannot swallow the bar.
pub const CRUMB_PAD: f32 = 8.0;
const CRUMB_CHAR: f32 = 7.4;
const CRUMB_MIN: f32 = 28.0;
const CRUMB_MAX: f32 = 220.0;

pub fn layout_crumbs(labels: &[String]) -> Vec<f32> {
    labels
        .iter()
        .map(|label| {
            let text = label.chars().count() as f32 * CRUMB_CHAR;
            (text + CRUMB_PAD * 2.0).clamp(CRUMB_MIN, CRUMB_MAX)
        })
        .collect()
}

/// The address bar as one line: `C:  ›  Users  ›  iamhieuxz`.
///
/// Slint cannot let children of a layout size themselves, so the breadcrumb is
/// rendered as a single `Text` rather than one clickable chip per segment. The
/// bar stays clickable as a whole to switch into path editing.
pub fn crumb_line(labels: &[String]) -> String {
    labels.join("  ›  ")
}

/// Split items into rows of at most `per_row`.
///
/// `GridLayout` in Slint 1.19 does not wrap: everything lands in one row. The
/// grid view therefore groups the items here and renders one `HorizontalLayout`
/// per row.
pub fn chunk_rows<T: Clone>(items: &[T], per_row: usize) -> Vec<Vec<T>> {
    if per_row == 0 {
        return Vec::new();
    }
    items.chunks(per_row).map(|c| c.to_vec()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, ext: Option<&str>) -> Entry {
        Entry {
            name: name.into(),
            path: format!("C:\\tmp\\{name}"),
            kind: EntryKind::File,
            size: Some(1),
            modified_ms: None,
            created_ms: None,
            extension: ext.map(|e| e.to_string()),
            is_hidden: false,
            is_readonly: false,
        }
    }

    #[test]
    fn sizes_switch_units_at_the_right_boundaries() {
        assert_eq!(format_size(Some(0)), "0 B");
        assert_eq!(format_size(Some(1023)), "1023 B");
        assert_eq!(format_size(Some(1024)), "1.0 KB");
        assert_eq!(format_size(Some(1024 * 1024)), "1.0 MB");
        assert_eq!(format_size(Some(1024 * 1024 * 1024)), "1.00 GB");
        assert_eq!(format_size(None), "", "directories show no size");
    }

    #[test]
    fn images_and_videos_get_distinct_icons() {
        assert_eq!(icon_for(&file("a.png", Some("png"))).0, "image");
        assert_eq!(icon_for(&file("a.mp4", Some("mp4"))).0, "video");
        assert_eq!(icon_for(&file("a.rs", Some("rs"))).0, "code");
        assert_eq!(icon_for(&file("a.zip", Some("zip"))).0, "folder_zip");
        assert_eq!(icon_for(&file("a.unknownext", Some("unknownext"))).0, "document");
    }

    #[test]
    fn applications_are_distinguishable_from_plain_files() {
        // An .exe is an app, not an anonymous document: Program Files is full
        // of them and they must not all look identical.
        assert_eq!(icon_for(&file("app.exe", Some("exe"))).0, "apps");
        assert_eq!(icon_for(&file("setup.msi", Some("msi"))).0, "box");
        assert_eq!(icon_for(&file("lib.dll", Some("dll"))).0, "puzzle_piece");
        assert_eq!(icon_for(&file("shortcut.lnk", Some("lnk"))).0, "link");
        // And they are visually distinct from each other.
        assert_ne!(
            icon_for(&file("app.exe", Some("exe"))).0,
            icon_for(&file("setup.msi", Some("msi"))).0
        );
    }

    #[test]
    fn modern_media_and_office_extensions_are_covered() {
        assert_eq!(icon_for(&file("a.heic", Some("heic"))).0, "image");
        assert_eq!(icon_for(&file("a.mkv", Some("mkv"))).0, "video");
        assert_eq!(icon_for(&file("a.flac", Some("flac"))).0, "music_note");
        assert_eq!(icon_for(&file("a.docx", Some("docx"))).0, "document");
        assert_eq!(icon_for(&file("a.pptx", Some("pptx"))).0, "slide_layout");
        assert_eq!(icon_for(&file("a.sqlite", Some("sqlite"))).0, "database");
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        assert_eq!(icon_for(&file("PHOTO.JPG", Some("JPG"))).0, "image");
        assert_eq!(icon_for(&file("APP.EXE", Some("EXE"))).0, "apps");
    }

    #[test]
    fn kind_label_upper_cases_the_extension() {
        assert_eq!(kind_label(&file("a.rs", Some("rs"))), "Tệp RS");
        assert_eq!(kind_label(&file("noext", None)), "Tệp");
    }

    #[test]
    fn an_unknown_timestamp_renders_empty_not_a_sentinel_date() {
        assert_eq!(format_time(None), "");
        // 1970-01-01 in local time is a real value and must still render.
        assert!(!format_time(Some(0)).is_empty());
    }

    #[test]
    fn pinned_folders_have_their_own_colours() {
        assert_eq!(folder_glyph("Downloads").0, "arrow_download");
        assert_eq!(folder_glyph("Something Else").0, "folder");
    }

    #[test]
    fn access_denied_explains_what_to_do() {
        let text = explain_error("access_denied", "Access is denied. (os error 5)");
        assert!(text.contains("quyền"), "phải nói về quyền: {text}");
        assert!(text.contains("Access is denied"), "giữ chi tiết gốc để còn báo lỗi: {text}");
    }

    #[test]
    fn access_denied_tells_the_user_what_to_do() {
        // "(os error 5)" không nói được gì; câu này phải chỉ ra hành động.
        let text = explain_error("access_denied", "Access is denied. (os error 5)");
        assert!(text.contains("Run as administrator"), "thiếu gợi ý quyền: {text}");
        assert!(text.contains("thư mục khác"), "thiếu lựa chọn thay thế");
    }

    #[test]
    fn unknown_codes_keep_the_original_message() {
        // Đừng nuốt mất chi tiết lạ: đó có thể là manh mối của một lỗi thật.
        assert_eq!(explain_error("io_error", "đĩa hỏng"), "đĩa hỏng");
    }

    #[test]
    fn hiding_extensions_strips_only_real_files() {
        assert_eq!(display_name("notes.txt", false, false), "notes");
        assert_eq!(display_name("notes.txt", false, true), "notes.txt");
        // A folder named like a file keeps its dots: `v1.2` is a real folder name.
        assert_eq!(display_name("v1.2", true, false), "v1.2");
    }

    #[test]
    fn hiding_extensions_never_empties_a_name() {
        // `.gitignore` has no stem; stripping would leave an empty row.
        assert_eq!(display_name(".gitignore", false, false), ".gitignore");
        // A trailing dot is not an extension either.
        assert_eq!(display_name("weird.", false, false), "weird.");
    }

    #[test]
    fn every_crumb_gets_a_clickable_width() {
        let labels: Vec<String> = ["C:", "Users", "iamhieuxz", "Documents"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let widths = layout_crumbs(&labels);
        assert_eq!(widths.len(), 4);
        // Wider names get more room, and the order is preserved.
        assert!(widths[0] < widths[1], "`C:` is narrower than `Users`");
        assert!(widths[1] < widths[3], "`Users` is narrower than `Documents`");
    }

    #[test]
    fn a_very_long_name_is_clamped() {
        let long = "a".repeat(500);
        let widths = layout_crumbs(&[long]);
        assert_eq!(widths[0], CRUMB_MAX, "long names must not swallow the bar");
    }

    #[test]
    fn a_short_name_keeps_a_clickable_minimum() {
        let widths = layout_crumbs(&["C:".to_string()]);
        assert!(widths[0] >= CRUMB_MIN);
    }

    #[test]
    fn the_crumb_line_separates_segments() {
        let labels: Vec<String> = ["C:", "Users", "me"].iter().map(|s| s.to_string()).collect();
        assert_eq!(crumb_line(&labels), "C:  ›  Users  ›  me");
    }

    #[test]
    fn an_empty_crumb_list_renders_nothing() {
        assert_eq!(crumb_line(&[]), "");
    }

    #[test]
    fn grid_rows_hold_at_most_per_row_items() {
        let items: Vec<u32> = (0..7).collect();
        let rows = chunk_rows(&items, 3);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].len(), 3);
        assert_eq!(rows[2].len(), 1, "the last row is short");
        // Every item appears exactly once, in order.
        let flat: Vec<u32> = rows.into_iter().flatten().collect();
        assert_eq!(flat, items);
    }

    #[test]
    fn an_empty_grid_has_no_rows() {
        let items: Vec<u32> = Vec::new();
        assert!(chunk_rows(&items, 5).is_empty());
    }

    #[test]
    fn a_zero_column_grid_does_not_panic() {
        let items = vec![1, 2, 3];
        assert!(chunk_rows(&items, 0).is_empty());
    }
}
