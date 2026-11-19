//! Coloured tags for files and folders.
//!
//! Mirrors the Files app: a fixed set of named tags, each with a colour, and any
//! number of items carrying any number of tags. The colour of the first tag is
//! what the list shows, which is what makes a tagged folder read as "the blue
//! one" at a glance.
//!
//! Storage is one JSON file: the tag definitions plus a path → tag-uids map.
//! Paths are the identity, because that is all the listing gives us; a renamed
//! item loses its tag, exactly like Files' path-based fallback.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One tag the user can attach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub uid: String,
    pub name: String,
    /// `#RRGGBB`, matching the Files palette.
    pub color: String,
}

/// The four defaults shipped with Files, kept identical so a user moving
/// between the two apps sees the same names and colours.
pub fn default_tags() -> Vec<Tag> {
    vec![
        Tag {
            uid: "f7e0e137-2eb5-4fa4-a50d-ddd65df17c34".into(),
            name: "Home".into(),
            color: "#0072BD".into(),
        },
        Tag {
            uid: "c84a8131-c4de-47d9-9440-26e859d14b3d".into(),
            name: "Work".into(),
            color: "#D95319".into(),
        },
        Tag {
            uid: "d4b8d4bd-ceaf-4e58-ac61-a185fcf96c5d".into(),
            name: "Photos".into(),
            color: "#EDB120".into(),
        },
        Tag {
            uid: "79376daf-c44a-4fe4-aa3b-8b30baea453e".into(),
            name: "Important".into(),
            color: "#77AC30".into(),
        },
    ]
}

/// Parse `#RRGGBB` into a packed `0xRRGGBB` colour.
///
/// Returns `None` for anything malformed rather than guessing, so a corrupted
/// settings file shows the default colour instead of a random one.
pub fn parse_hex(hex: &str) -> Option<u32> {
    let s = hex.trim().trim_start_matches('#');
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

/// The whole tag database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagStore {
    /// Tag definitions, in the order they appear in menus.
    pub tags: Vec<Tag>,
    /// Path → tag uids, most recently applied first.
    pub assignments: BTreeMap<String, Vec<String>>,
}

impl Default for TagStore {
    fn default() -> Self {
        Self {
            tags: default_tags(),
            assignments: BTreeMap::new(),
        }
    }
}

impl TagStore {
    /// Where the real app keeps its tags.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("FilesRS").join("tags.json")
    }

    /// Read the store, falling back to the defaults.
    ///
    /// A missing file is normal on first run; a corrupt one is not worth
    /// blocking startup over, so it also falls back — but the caller logs it.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        // Như `Settings::load`: file 0 byte do bước kiểm tra quyền ghi tạo ra,
        // không phải JSON hỏng, nên không được đổi tên thành `.corrupt`.
        if text.trim().is_empty() {
            return Self::default();
        }
        serde_json::from_str(&text).unwrap_or_else(|_| {
            crate::persist::quarantine(path);
            Self::default()
        })
    }

    /// Write the store, creating the folder if needed.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        crate::persist::write_atomic(path, &text)
    }

    /// Find a tag definition by uid.
    pub fn tag(&self, uid: &str) -> Option<&Tag> {
        self.tags.iter().find(|t| t.uid == uid)
    }

    /// Tags attached to `path`, in stored order.
    pub fn tags_for(&self, path: &Path) -> Vec<&Tag> {
        let key = path.to_string_lossy();
        self.assignments
            .get(key.as_ref())
            .map(|uids| uids.iter().filter_map(|u| self.tag(u)).collect())
            .unwrap_or_default()
    }

    /// Colour of the first tag on `path`, or `None` when untagged.
    ///
    /// The first tag wins because that is what the icon shows; a second tag is
    /// still recorded and visible in the menu.
    pub fn color_for(&self, path: &Path) -> Option<u32> {
        self.tags_for(path)
            .first()
            .and_then(|t| parse_hex(&t.color))
    }

    /// Attach `uid` to `path`. Applying the same tag twice is a no-op.
    pub fn attach(&mut self, path: &Path, uid: &str) {
        if self.tag(uid).is_none() {
            return;
        }
        let entry = self
            .assignments
            .entry(path.to_string_lossy().into_owned())
            .or_default();
        if !entry.iter().any(|u| u == uid) {
            // Newest first so the freshly chosen tag drives the icon colour.
            entry.insert(0, uid.to_string());
        }
    }

    /// Remove one tag from `path`, dropping the path entry when it empties.
    pub fn detach(&mut self, path: &Path, uid: &str) {
        let key = path.to_string_lossy().into_owned();
        if let Some(list) = self.assignments.get_mut(&key) {
            list.retain(|u| u != uid);
            if list.is_empty() {
                self.assignments.remove(&key);
            }
        }
    }

    /// Remove every tag from `path`.
    pub fn clear(&mut self, path: &Path) {
        self.assignments
            .remove(&path.to_string_lossy().into_owned());
    }

    /// Drop assignments whose entry vanished from disk, so the file does not
    /// grow forever and a recycled path cannot inherit a stranger's colour.
    ///
    /// Only paths under `root` are checked: scanning the whole drive on every
    /// refresh would be worse than the leak.
    pub fn prune_missing(&mut self, root: &Path) {
        let prefix = root.to_string_lossy().into_owned();
        self.assignments.retain(|path, _| {
            if !path.starts_with(&prefix) {
                return true;
            }
            std::path::Path::new(path).exists()
        });
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
    fn the_defaults_match_the_files_app() {
        let tags = default_tags();
        assert_eq!(tags.len(), 4);
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["Home", "Work", "Photos", "Important"]);
        // Màu lấy đúng từ FileTagsSettingsService.cs của repo gốc.
        assert_eq!(parse_hex(&tags[0].color), Some(0x0072BD));
        assert_eq!(parse_hex(&tags[1].color), Some(0xD95319));
        assert_eq!(parse_hex(&tags[2].color), Some(0xEDB120));
        assert_eq!(parse_hex(&tags[3].color), Some(0x77AC30));
    }

    #[test]
    fn hex_parsing_rejects_rubbish() {
        assert_eq!(parse_hex("#0072BD"), Some(0x0072BD));
        assert_eq!(parse_hex("0072bd"), Some(0x0072BD));
        assert_eq!(parse_hex("#0072B"), None, "thiếu ký tự");
        assert_eq!(parse_hex("#0072BDE"), None, "thừa ký tự");
        assert_eq!(parse_hex("#GGGGGG"), None, "không phải hex");
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn attaching_a_tag_gives_the_path_that_colour() {
        let mut store = TagStore::default();
        let p = Path::new(r"C:\x\report.docx");
        assert_eq!(store.color_for(p), None, "chưa gắn thì không có màu");

        store.attach(p, &store.tags[0].uid.clone());
        assert_eq!(store.color_for(p), Some(0x0072BD));
    }

    #[test]
    fn attaching_the_same_tag_twice_does_not_duplicate() {
        let mut store = TagStore::default();
        let p = Path::new(r"C:\x\a.txt");
        let uid = store.tags[1].uid.clone();
        store.attach(p, &uid);
        store.attach(p, &uid);
        assert_eq!(store.tags_for(p).len(), 1);
    }

    #[test]
    fn the_newest_tag_decides_the_icon_colour() {
        let mut store = TagStore::default();
        let p = Path::new(r"C:\x\b.txt");
        let (blue, orange) = (store.tags[0].uid.clone(), store.tags[1].uid.clone());
        store.attach(p, &blue);
        store.attach(p, &orange);
        // Gắn Work sau Home thì icon phải thành màu Work.
        assert_eq!(store.color_for(p), Some(0xD95319));
        // Nhưng cả hai vẫn được ghi nhớ.
        assert_eq!(store.tags_for(p).len(), 2);
    }

    #[test]
    fn detaching_the_last_tag_removes_the_entry() {
        // Để lại key rỗng sẽ phình file và làm `prune` phải quét vô ích.
        let mut store = TagStore::default();
        let p = Path::new(r"C:\x\c.txt");
        let uid = store.tags[2].uid.clone();
        store.attach(p, &uid);
        store.detach(p, &uid);
        assert!(store.assignments.is_empty());
        assert_eq!(store.color_for(p), None);
    }

    #[test]
    fn clearing_removes_every_tag_at_once() {
        let mut store = TagStore::default();
        let p = Path::new(r"C:\x\d.txt");
        let (a, b) = (store.tags[0].uid.clone(), store.tags[3].uid.clone());
        store.attach(p, &a);
        store.attach(p, &b);
        store.clear(p);
        assert!(store.tags_for(p).is_empty());
    }

    #[test]
    fn an_unknown_tag_uid_is_ignored() {
        // Menu cũ có thể gửi uid của thẻ đã bị xoá.
        let mut store = TagStore::default();
        store.attach(Path::new(r"C:\x\e.txt"), "khong-ton-tai");
        assert!(store.assignments.is_empty());
    }

    #[test]
    fn a_tag_that_vanished_from_definitions_is_skipped() {
        // File cấu hình bị sửa tay: uid còn nhưng định nghĩa mất.
        let mut store = TagStore::default();
        let p = Path::new(r"C:\x\f.txt");
        store.attach(p, &store.tags[0].uid.clone());
        store.tags.clear();
        assert_eq!(store.color_for(p), None);
        assert!(store.tags_for(p).is_empty());
    }

    #[test]
    fn saving_and_loading_round_trips() {
        let dir = scratch("tags-roundtrip");
        let path = dir.join("tags.json");
        let mut store = TagStore::default();
        let p = Path::new(r"C:\x\round trip.txt");
        store.attach(p, &store.tags[0].uid.clone());
        store.save(&path).expect("ghi được");

        let back = TagStore::load(&path);
        assert_eq!(back.tags.len(), 4);
        assert_eq!(back.color_for(p), Some(0x0072BD));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_writers_do_not_drop_each_others_assignments() {
        // Mô phỏng hai cửa sổ dùng chung tags.json: mỗi bên đọc bản mới nhất
        // trước khi sửa, nên thẻ của bên kia không bị xoá.
        let dir = scratch("tags-two-writers");
        let path = dir.join("tags.json");
        let blue = default_tags()[0].uid.clone();
        let orange = default_tags()[1].uid.clone();
        let a = Path::new(r"C:\x\cua-so-a.txt");
        let b = Path::new(r"C:\x\cua-so-b.txt");

        let mut first = TagStore::load(&path);
        first.attach(a, &blue);
        first.save(&path).expect("ghi được");

        let mut second = TagStore::load(&path);
        second.attach(b, &orange);
        second.save(&path).expect("ghi được");

        let final_store = TagStore::load(&path);
        assert_eq!(final_store.color_for(a), Some(0x0072BD), "thẻ của A còn");
        assert_eq!(final_store.color_for(b), Some(0xD95319), "thẻ của B còn");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_loads_the_defaults() {
        let store = TagStore::load(Path::new(r"C:\khong\co\tags.json"));
        assert_eq!(store.tags.len(), 4);
        assert!(store.assignments.is_empty());
    }

    #[test]
    fn a_corrupt_file_loads_the_defaults_instead_of_panicking() {
        let dir = scratch("tags-corrupt");
        let path = dir.join("tags.json");
        std::fs::write(&path, "{ đây không phải json").unwrap();
        let store = TagStore::load(&path);
        assert_eq!(store.tags.len(), 4, "phải quay về mặc định");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_file_keeps_the_path_in_place() {
        // Như settings: file 0 byte do bước kiểm tra quyền ghi tạo ra không
        // phải dữ liệu hỏng, nên không được đổi tên thành `.corrupt`.
        let dir = scratch("tags-empty");
        let path = dir.join("tags.json");
        std::fs::File::create(&path).unwrap();

        let store = TagStore::load(&path);

        assert_eq!(store.tags.len(), 4);
        assert!(path.exists(), "file rỗng phải được giữ nguyên tại chỗ");
        assert!(!dir.join("tags.json.corrupt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pruning_drops_entries_that_no_longer_exist() {
        let dir = scratch("tags-prune");
        // Tránh tên thiết bị dành riêng của Windows (CON, PRN, NUL…): "con.txt"
        // trỏ vào thiết bị console chứ không phải một tệp.
        let alive = dir.join("con-song.txt");
        std::fs::write(&alive, b"x").unwrap();
        let gone = dir.join("da-mat.txt");

        let mut store = TagStore::default();
        let uid = store.tags[0].uid.clone();
        store.attach(&alive, &uid);
        store.attach(&gone, &uid);

        store.prune_missing(&dir);
        assert_eq!(store.tags_for(&alive).len(), 1, "tệp còn thì giữ thẻ");
        assert!(store.tags_for(&gone).is_empty(), "tệp mất thì bỏ thẻ");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pruning_leaves_paths_outside_the_root_alone() {
        // Chỉ dọn trong thư mục đang xem; quét cả ổ đĩa mỗi lần làm mới thì
        // quá đắt so với lợi ích.
        let dir = scratch("tags-prune-scope");
        let outside = Path::new(r"D:\noi-khac\tep.txt");
        let mut store = TagStore::default();
        store.attach(outside, &store.tags[0].uid.clone());
        store.prune_missing(&dir);
        assert_eq!(
            store.tags_for(outside).len(),
            1,
            "ngoài phạm vi thì giữ nguyên"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
