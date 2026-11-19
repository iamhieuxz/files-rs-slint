//! On-disk thumbnail cache.
//!
//! Thumbnails are expensive to decode: a folder of photos otherwise pays WIC
//! every time it is opened. Entries are keyed by path, size and the source's
//! modification time + length, so a changed file is decoded again instead of
//! showing a stale picture.
//!
//! The cache is best-effort: an unwritable cache directory must never stop a
//! thumbnail from rendering. It is also bounded, so it cannot grow forever.

use crate::shellicon::{thumbnail, RgbaIcon};
use files_core::FsError;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

/// Keep the cache small enough to be a cache, not a second copy of the photos.
const CACHE_LIMIT_BYTES: u64 = 64 * 1024 * 1024;
/// Trimming scans the directory; doing it on every write would make a folder of
/// 500 photos do 500 scans. Every 32 writes is enough to keep it bounded.
const TRIM_EVERY: u64 = 32;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
static WRITES: AtomicU64 = AtomicU64::new(0);

/// Thumbnail for `path`, served from the disk cache when possible.
pub fn thumbnail_cached(path: &Path, size: u32) -> Result<Option<RgbaIcon>, FsError> {
    thumbnail_cached_in(&cache_dir(), path, size)
}

/// Same as [`thumbnail_cached`] with an explicit cache directory.
///
/// Used by tests; also makes the cache location visible at the call site.
pub fn thumbnail_cached_in(
    cache_dir: &Path,
    path: &Path,
    size: u32,
) -> Result<Option<RgbaIcon>, FsError> {
    let Some(key) = cache_key(path, size) else {
        // Không đọc được metadata (tệp biến mất): cứ thử giải mã, kết quả
        // thường là None và không đụng tới cache.
        return thumbnail(path, size);
    };
    let file = cache_dir.join(format!("{key}.rgba"));
    if let Some(icon) = read_cache(&file) {
        return Ok(Some(icon));
    }

    let icon = thumbnail(path, size)?;
    if let Some(ref icon) = icon {
        match write_cache(&file, icon) {
            Ok(()) => maybe_trim(cache_dir),
            Err(e) => crate::log::warn(&format!("không ghi được cache thumbnail: {e}")),
        }
    }
    Ok(icon)
}

/// Where cached thumbnails live.
pub fn cache_dir() -> PathBuf {
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        let dir = PathBuf::from(base).join("FilesRS").join("thumbcache");
        if std::fs::create_dir_all(&dir).is_ok() {
            return dir;
        }
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        let fallback = dir.join("files-rs-thumbcache");
        // Không tạo được thư mục thì vẫn trả đường dẫn; lần ghi sau sẽ báo lỗi
        // có ngữ cảnh. Ghi log ở đây để biết nguyên nhân gốc.
        if let Err(e) = std::fs::create_dir_all(&fallback) {
            crate::log::warn(&format!("không tạo được thư mục cache dự phòng: {e}"));
        }
        return fallback;
    }
    PathBuf::from("files-rs-thumbcache")
}

/// Cache entry name for one source file.
///
/// Returns `None` when the source cannot be stat'ed; a cached picture without
/// a live source would never be requested by the listing anyway.
fn cache_key(path: &Path, size: u32) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(modified.to_le_bytes());
    hasher.update(meta.len().to_le_bytes());
    hasher.update(size.to_le_bytes());
    Some(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn read_cache(path: &Path) -> Option<RgbaIcon> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 8 {
        return None;
    }
    let width = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    let height = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return None;
    }
    let pixels_len = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if bytes.len() != 8 + pixels_len {
        return None;
    }
    Some(RgbaIcon {
        width,
        height,
        pixels: bytes[8..].to_vec(),
    })
}

fn write_cache(path: &Path, icon: &RgbaIcon) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "thumb".into());
    let tmp = path.with_file_name(format!(".{name}.{}.{sequence}.tmp", std::process::id()));
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(&icon.width.to_le_bytes())?;
    file.write_all(&icon.height.to_le_bytes())?;
    file.write_all(&icon.pixels)?;
    drop(file);
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

fn maybe_trim(dir: &Path) {
    if WRITES.fetch_add(1, Ordering::Relaxed) % TRIM_EVERY == TRIM_EVERY - 1 {
        enforce_limit(dir, CACHE_LIMIT_BYTES);
    }
}

/// Delete the oldest cache files until the directory fits in `limit` bytes.
fn enforce_limit(dir: &Path, limit: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut total = 0u64;
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        total = total.saturating_add(meta.len());
        files.push((entry.path(), meta.len(), meta.modified().ok()));
    }
    if total <= limit {
        return;
    }
    files.sort_by_key(|(_, _, modified)| *modified);
    for (path, len, _) in files {
        if total <= limit {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let exe = std::env::current_exe().expect("exe");
        let dir = exe
            .parent()
            .expect("thư mục")
            .join("test-scratch")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tạo thư mục tạm");
        dir
    }

    fn icon() -> RgbaIcon {
        RgbaIcon {
            width: 2,
            height: 1,
            pixels: vec![1, 2, 3, 4, 5, 6, 7, 8],
        }
    }

    #[test]
    fn a_cached_bitmap_round_trips() {
        let dir = scratch("thumbcache-roundtrip");
        let file = dir.join("a.rgba");
        write_cache(&file, &icon()).expect("ghi được");
        let back = read_cache(&file).expect("đọc lại được");
        assert_eq!(back.width, 2);
        assert_eq!(back.height, 1);
        assert_eq!(back.pixels, icon().pixels);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cache_write_into_a_blocked_folder_reports_an_error() {
        // Một tệp nằm đúng chỗ thư mục cha cần có: ghi cache phải trả lỗi,
        // không được nuốt im lặng.
        let dir = scratch("thumbcache-blocked");
        let blocker = dir.join("chan");
        std::fs::write(&blocker, b"khong phai thu muc").unwrap();
        let file = blocker.join("a.rgba");
        assert!(write_cache(&file, &icon()).is_err(), "phải báo lỗi");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_truncated_cache_entry_is_ignored() {
        let dir = scratch("thumbcache-truncated");
        let file = dir.join("a.rgba");
        std::fs::write(&file, [0u8; 4]).unwrap();
        assert!(read_cache(&file).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bitmap_with_an_impossible_size_is_ignored() {
        let dir = scratch("thumbcache-bad-size");
        let file = dir.join("a.rgba");
        std::fs::write(&file, [0xffu8; 8]).unwrap();
        assert!(
            read_cache(&file).is_none(),
            "kích thước rác không được panic"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trimming_keeps_the_cache_under_the_limit() {
        let dir = scratch("thumbcache-limit");
        for n in 0..3 {
            std::fs::write(dir.join(format!("{n}.rgba")), vec![0u8; 100]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        enforce_limit(&dir, 250);
        let total: u64 = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();
        assert!(total <= 250, "cache vẫn vượt giới hạn: {total}");
        let count = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(count, 2, "chỉ xoá đủ số tệp cũ");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
