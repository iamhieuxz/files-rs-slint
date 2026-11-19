//! Dữ liệu nặng cho panel xem trước.
//!
//! Panel xem trước cần ba thứ đắt tiền: số mục của một thư mục, ảnh đã giải mã,
//! và đoạn đầu của tệp văn bản. Làm việc đó trên UI thread khiến một thư mục
//! mạng hoặc một ảnh lớn làm đứng cửa sổ, nên toàn bộ phần đọc/giải mã nằm ở
//! đây và chạy trên luồng nền.
//!
//! Module này chỉ trả dữ liệu thuần; quyết định hiển thị vẫn ở tầng UI.

use std::io::Read;
use std::path::Path;

/// Đọc tối đa bao nhiêu byte văn bản cho phần xem trước.
pub const TEXT_LIMIT: u64 = 16 * 1024;

/// Thư mục có nhiều hơn ngưỡng này thì báo "N+" thay vì đếm tiếp.
pub const FOLDER_COUNT_LIMIT: usize = 5000;

/// Loại dữ liệu cần cho một mục đang chọn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    /// Thư mục: chỉ cần số mục con.
    Folder,
    /// Tệp ảnh: giải mã thành bitmap.
    Image,
    /// Tệp văn bản: đọc phần đầu.
    Text,
    /// Loại không xem trước được (nhị phân, kích thước lớn…).
    None,
}

impl PreviewKind {
    /// Suy ra loại xem trước từ phần mở rộng và kích thước tệp.
    ///
    /// `size` là kích thước đã biết từ listing, nên quyết định này không cần
    /// chạm đĩa.
    pub fn for_entry(extension: Option<&str>, size: Option<u64>, is_dir: bool) -> Self {
        if is_dir {
            return Self::Folder;
        }
        let ext = extension.unwrap_or("").to_ascii_lowercase();
        if IMAGE_EXTS.contains(&ext.as_str()) {
            return Self::Image;
        }
        // Tệp lớn không đáng đọc chỉ để hiện vài dòng đầu.
        if TEXT_EXTS.contains(&ext.as_str()) && size.unwrap_or(0) <= 4 * TEXT_LIMIT {
            return Self::Text;
        }
        Self::None
    }
}

/// Phần mở rộng được giải mã thành ảnh xem trước.
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "ico"];

/// Phần mở rộng được hiển thị dạng văn bản.
const TEXT_EXTS: &[&str] = &[
    "txt", "md", "rs", "toml", "json", "log", "csv", "ini", "cfg", "yaml", "yml", "xml", "html",
    "css", "js", "ts", "py", "bat", "ps1", "cmd", "sh",
];

/// Kết quả đọc nền cho panel xem trước.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewData {
    /// Thư mục: số mục con, `None` khi không đọc được.
    FolderCount(Option<usize>),
    /// Ảnh đã giải mã.
    Image(crate::shellicon::RgbaIcon),
    /// Phần đầu tệp văn bản, đã chuyển sang chuỗi mất mát byte.
    Text(String),
    /// Không có gì để hiển thị.
    None,
}

/// Đọc dữ liệu xem trước cho `path`.
///
/// Không bao giờ panic và không bao giờ trả lỗi: panel xem trước là trang trí,
/// nên tệp hỏng chỉ đơn giản là không có gì để hiện.
pub fn preview_data(path: &Path, kind: PreviewKind) -> PreviewData {
    match kind {
        PreviewKind::Folder => PreviewData::FolderCount(folder_count(path)),
        PreviewKind::Image => match crate::shellicon::thumbnail(path, 240) {
            Ok(Some(icon)) => PreviewData::Image(icon),
            _ => PreviewData::None,
        },
        PreviewKind::Text => read_text_head(path)
            .map(PreviewData::Text)
            .unwrap_or(PreviewData::None),
        PreviewKind::None => PreviewData::None,
    }
}

/// Số mục con trực tiếp, kẹp ở `FOLDER_COUNT_LIMIT`.
fn folder_count(path: &Path) -> Option<usize> {
    let iter = std::fs::read_dir(path).ok()?;
    let mut n = 0usize;
    for _ in iter.take(FOLDER_COUNT_LIMIT + 1) {
        n += 1;
    }
    Some(n)
}

/// Đọc tối đa `TEXT_LIMIT` byte đầu của một tệp văn bản.
fn read_text_head(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.take(TEXT_LIMIT).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("files-rs-preview-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_folder_reports_its_child_count() {
        let dir = scratch("count");
        std::fs::write(dir.join("a.txt"), b"x").unwrap();
        std::fs::write(dir.join("b.txt"), b"x").unwrap();

        assert_eq!(
            preview_data(&dir, PreviewKind::Folder),
            PreviewData::FolderCount(Some(2))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_reports_none_instead_of_panicking() {
        let missing = PathBuf::from(r"C:\khong\co\that");
        assert_eq!(
            preview_data(&missing, PreviewKind::Folder),
            PreviewData::FolderCount(None)
        );
    }

    #[test]
    fn text_is_read_only_up_to_the_limit() {
        let dir = scratch("text");
        let file = dir.join("big.txt");
        std::fs::write(&file, vec![b'a'; (TEXT_LIMIT as usize) * 3]).unwrap();

        match preview_data(&file, PreviewKind::Text) {
            PreviewData::Text(text) => {
                assert_eq!(text.len(), TEXT_LIMIT as usize, "chỉ đọc phần đầu");
            }
            other => panic!("cần Text, nhận {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_image_reports_none_instead_of_an_error() {
        let dir = scratch("bad-image");
        let file = dir.join("hong.png");
        std::fs::write(&file, b"khong phai anh").unwrap();

        assert_eq!(preview_data(&file, PreviewKind::Image), PreviewData::None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_kind_follows_extension_and_size() {
        assert_eq!(
            PreviewKind::for_entry(None, None, true),
            PreviewKind::Folder
        );
        assert_eq!(
            PreviewKind::for_entry(Some("PNG"), None, false),
            PreviewKind::Image
        );
        assert_eq!(
            PreviewKind::for_entry(Some("md"), Some(10), false),
            PreviewKind::Text
        );
        assert_eq!(
            PreviewKind::for_entry(Some("md"), Some(10 * TEXT_LIMIT), false),
            PreviewKind::None,
            "tệp lớn không đáng đọc chỉ để xem trước"
        );
        assert_eq!(
            PreviewKind::for_entry(Some("exe"), Some(1), false),
            PreviewKind::None
        );
    }
}
