//! Ghi và đọc file JSON của người dùng sao cho không mất dữ liệu.
//!
//! `settings.json` và `tags.json` là dữ liệu người dùng tạo ra. Ghi thẳng bằng
//! `fs::write` có thể để lại file cụt nếu tiến trình bị dừng giữa chừng, và một
//! file hỏng bị đọc thành "mặc định" rồi bị lần lưu sau ghi đè mất luôn bản cũ.

use std::path::{Path, PathBuf};

/// Ghi `text` vào `path` qua file tạm cùng thư mục rồi đổi tên.
///
/// Đổi tên trong cùng volume là một bước nguyên tử: người đọc thấy bản cũ hoặc
/// bản mới, không bao giờ thấy bản ghi dở.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    if let Err(e) = std::fs::write(&tmp, text) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Giữ lại file không đọc được dưới tên `.corrupt` để không bị ghi đè.
///
/// Trả về đường dẫn bản sao lưu nếu đổi tên được.
pub fn quarantine(path: &Path) -> Option<PathBuf> {
    let mut name = path.as_os_str().to_owned();
    name.push(".corrupt");
    let backup = PathBuf::from(name);
    std::fs::rename(path, &backup).ok().map(|_| backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("files-rs-persist-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_atomic_write_replaces_the_file_and_leaves_no_temp() {
        let dir = scratch("atomic");
        let f = dir.join("a.json");
        std::fs::write(&f, "cu").unwrap();

        write_atomic(&f, "moi").unwrap();

        assert_eq!(std::fs::read_to_string(&f).unwrap(), "moi");
        assert!(!dir.join("a.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_write_keeps_the_old_file() {
        let dir = scratch("failed");
        let f = dir.join("a.json");
        std::fs::write(&f, "cu").unwrap();
        // Một thư mục chiếm chỗ file tạm khiến bước ghi chắc chắn lỗi.
        std::fs::create_dir(dir.join("a.json.tmp")).unwrap();

        assert!(write_atomic(&f, "moi").is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "cu");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quarantine_moves_the_bad_file_aside() {
        let dir = scratch("quarantine");
        let f = dir.join("a.json");
        std::fs::write(&f, "{hong").unwrap();

        let backup = quarantine(&f).expect("phải đổi tên được");

        assert!(!f.exists());
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "{hong");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
