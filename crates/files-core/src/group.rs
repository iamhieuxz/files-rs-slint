//! Nhóm mục trong danh sách theo một tiêu chí.
//!
//! Files gốc có `GroupAction` với các lựa chọn None/Name/DateModified/DateCreated/
//! Size/FileType/FileTag. Ở đây nhóm là **dữ liệu thuần**: nhận danh sách `Entry`
//! đã sắp xếp, trả về các nhóm kèm nhãn. Nhờ vậy cùng một logic dùng được cho
//! danh sách chi tiết, lưới, và test không cần UI.
//!
//! Thứ tự nhóm theo đúng thứ tự xuất hiện trong danh sách đã sắp xếp, nên nhóm
//! không phá vỡ thứ tự mà người dùng vừa chọn ở cột.

use crate::entry::Entry;
use serde::{Deserialize, Serialize};

/// Tiêu chí nhóm.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupBy {
    /// Không nhóm: một danh sách phẳng.
    #[default]
    None,
    /// Theo chữ cái đầu của tên; chữ số và ký tự khác gom vào "#".
    Name,
    /// Theo ngày sửa, kèm đơn vị.
    DateModified,
    /// Theo ngày tạo.
    DateCreated,
    /// Theo khoảng kích thước.
    Size,
    /// Theo loại tệp (phần mở rộng, hoặc "Thư mục").
    Kind,
    /// Theo thẻ màu đầu tiên đang gắn.
    Tag,
    /// Theo thư mục gốc của tệp. Chỉ có nghĩa trong Thùng rác, nơi mỗi mục nhớ
    /// đường dẫn trước khi bị xoá.
    OriginalFolder,
    /// Theo ngày xoá. Chỉ có nghĩa trong Thùng rác.
    DeletedDate,
}

impl GroupBy {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Không nhóm",
            Self::Name => "Tên",
            Self::DateModified => "Ngày sửa",
            Self::DateCreated => "Ngày tạo",
            Self::Size => "Kích thước",
            Self::Kind => "Loại",
            Self::Tag => "Thẻ màu",
            Self::OriginalFolder => "Thư mục gốc",
            Self::DeletedDate => "Ngày xoá",
        }
    }

    /// Danh sách để dựng menu chọn.
    pub fn all() -> &'static [Self] {
        &[
            Self::None,
            Self::Name,
            Self::DateModified,
            Self::DateCreated,
            Self::Size,
            Self::Kind,
            Self::Tag,
        ]
    }

    /// Tiêu chí chỉ có nghĩa trong Thùng rác.
    ///
    /// Menu thường không nên hiện hai mục này vì ngoài Thùng rác không có dữ
    /// liệu để nhóm theo.
    pub fn recycle_only(self) -> bool {
        matches!(self, Self::OriginalFolder | Self::DeletedDate)
    }
}

/// Đơn vị gom ngày.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DateUnit {
    Year,
    #[default]
    Month,
    Day,
}

/// Một nhóm trong danh sách.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Nhãn hiển thị ở header nhóm.
    pub label: String,
    /// Chỉ số (trong `entries`) của các mục thuộc nhóm này.
    pub indices: Vec<usize>,
}

/// Chia `entries` thành các nhóm theo `by`.
///
/// Trả về một nhóm duy nhất với nhãn rỗng khi `by == None`, để nơi gọi không
/// phải xử lý hai hình dạng dữ liệu khác nhau.
pub fn group(
    entries: &[Entry],
    by: GroupBy,
    unit: DateUnit,
    tag_of: impl Fn(&Entry) -> Option<String>,
) -> Vec<Group> {
    if by == GroupBy::None {
        return vec![Group {
            label: String::new(),
            indices: (0..entries.len()).collect(),
        }];
    }

    // Giữ thứ tự xuất hiện: nhóm đầu tiên gặp được đứng trước.
    let mut order: Vec<String> = Vec::new();
    let mut buckets: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();

    for (index, entry) in entries.iter().enumerate() {
        let key = key_for(entry, by, unit, &tag_of);
        if !buckets.contains_key(&key) {
            order.push(key.clone());
        }
        buckets.entry(key).or_default().push(index);
    }

    order
        .into_iter()
        .map(|label| Group {
            indices: buckets.remove(&label).unwrap_or_default(),
            label,
        })
        .collect()
}

/// Khoá nhóm của một mục.
fn key_for(
    entry: &Entry,
    by: GroupBy,
    unit: DateUnit,
    tag_of: &impl Fn(&Entry) -> Option<String>,
) -> String {
    match by {
        GroupBy::None => String::new(),
        GroupBy::Name => {
            let first = entry
                .name
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default();
            if first.chars().next().is_some_and(|c| c.is_alphabetic()) {
                first
            } else {
                "#".to_string()
            }
        }
        GroupBy::DateModified => date_label(entry.modified_ms, unit),
        GroupBy::DateCreated => date_label(entry.created_ms, unit),
        GroupBy::Size => size_label(entry),
        GroupBy::Kind => kind_label(entry),
        GroupBy::Tag => tag_of(entry).unwrap_or_else(|| "Chưa gắn thẻ".to_string()),
        // Hai tiêu chí dưới chỉ dùng trong Thùng rác. Nơi gọi (`view::grouped_rows`)
        // dựng sẵn bản sao `Entry` có `path` là thư mục gốc và `modified_ms` là
        // thời điểm xoá, nên ở đây chỉ cần đọc đúng trường đó.
        GroupBy::OriginalFolder => {
            if entry.path.is_empty() {
                "Không rõ".to_string()
            } else {
                entry.path.clone()
            }
        }
        GroupBy::DeletedDate => date_label(entry.modified_ms, unit),
    }
}

/// Nhãn ngày theo đơn vị. Mốc thời gian tính bằng milli giây Unix, giờ UTC.
fn date_label(ms: Option<i64>, unit: DateUnit) -> String {
    let Some(ms) = ms else {
        return "Không rõ".to_string();
    };
    let secs = ms.div_euclid(1000);
    // Chuyển sang ngày/tháng/năm bằng số học thuần để không kéo thêm crate vào
    // `files-core`; đủ đúng cho nhãn nhóm.
    let days = secs.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    match unit {
        DateUnit::Year => format!("{year}"),
        DateUnit::Month => format!("{month:02}/{year}"),
        DateUnit::Day => format!("{day:02}/{month:02}/{year}"),
    }
}

/// Đổi số ngày kể từ 1970-01-01 thành (năm, tháng, ngày).
///
/// Thuật toán của Howard Hinnant, dùng cho lịch Gregorius; tránh phụ thuộc
/// `chrono` trong crate dữ liệu thuần.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Nhãn kích thước theo khoảng, khớp cách Explorer gom.
fn size_label(entry: &Entry) -> String {
    if entry.is_dir() {
        return "Thư mục".to_string();
    }
    let Some(size) = entry.size else {
        return "Không rõ".to_string();
    };
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    match size {
        0 => "Trống".to_string(),
        1..KB => "Nhỏ (< 1 KB)".to_string(),
        1024..=102_399 => "1–100 KB".to_string(),
        102_400..=1_048_575 => "100 KB – 1 MB".to_string(),
        1_048_576..=10_485_759 => "1–10 MB".to_string(),
        10_485_760..=104_857_599 => "10–100 MB".to_string(),
        104_857_600..=1_073_741_823 => "100 MB – 1 GB".to_string(),
        _ => {
            // Trên 1 GB: ghi theo GB cho dễ đọc.
            let gb = size / GB;
            format!("≥ {gb} GB")
        }
    }
}

/// Nhãn loại: "Thư mục" hoặc phần mở rộng viết hoa.
fn kind_label(entry: &Entry) -> String {
    if entry.is_dir() {
        return "Thư mục".to_string();
    }
    match entry.ext_lower() {
        Some(ext) if !ext.is_empty() => ext.to_uppercase(),
        _ => "Không phần mở rộng".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::EntryKind;

    fn entry(name: &str, size: Option<u64>, modified: Option<i64>) -> Entry {
        Entry {
            name: name.into(),
            path: format!("C:\\{name}"),
            kind: EntryKind::File,
            size,
            modified_ms: modified,
            created_ms: modified,
            extension: std::path::Path::new(name)
                .extension()
                .map(|e| e.to_string_lossy().into_owned()),
            is_hidden: false,
            is_readonly: false,
        }
    }

    #[test]
    fn none_returns_one_flat_group() {
        let items = vec![entry("a.txt", Some(1), None), entry("b.txt", Some(2), None)];
        let groups = group(&items, GroupBy::None, DateUnit::Month, |_| None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label, "");
        assert_eq!(groups[0].indices, vec![0, 1]);
    }

    #[test]
    fn name_groups_by_first_letter_and_buckets_the_rest() {
        let items = vec![
            entry("apple.txt", None, None),
            entry("Banana.txt", None, None),
            entry("1number.txt", None, None),
            entry("another.txt", None, None),
        ];
        let groups = group(&items, GroupBy::Name, DateUnit::Month, |_| None);
        let labels: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, vec!["A", "B", "#"]);
        assert_eq!(groups[0].indices, vec![0, 3], "cùng chữ cái gom một nhóm");
    }

    #[test]
    fn groups_keep_the_order_they_first_appear() {
        // Nhóm không được tự sắp xếp lại: thứ tự do cột đang sắp quyết định.
        let items = vec![
            entry("zebra.txt", None, None),
            entry("apple.txt", None, None),
        ];
        let groups = group(&items, GroupBy::Name, DateUnit::Month, |_| None);
        assert_eq!(groups[0].label, "Z");
        assert_eq!(groups[1].label, "A");
    }

    #[test]
    fn size_groups_cover_the_documented_ranges() {
        let items = vec![
            entry("empty.txt", Some(0), None),
            entry("tiny.txt", Some(500), None),
            entry("kb.txt", Some(5 * 1024), None),
            entry("mb.txt", Some(5 * 1024 * 1024), None),
        ];
        let groups = group(&items, GroupBy::Size, DateUnit::Month, |_| None);
        let labels: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, vec!["Trống", "Nhỏ (< 1 KB)", "1–100 KB", "1–10 MB"]);
    }

    #[test]
    fn a_folder_groups_as_folder_regardless_of_size() {
        let mut dir = entry("Documents", Some(999_999), None);
        dir.kind = EntryKind::Directory;
        let groups = group(&[dir], GroupBy::Size, DateUnit::Month, |_| None);
        assert_eq!(groups[0].label, "Thư mục");
    }

    #[test]
    fn kind_groups_by_extension_uppercased() {
        let items = vec![entry("a.txt", None, None), entry("b.TXT", None, None)];
        let groups = group(&items, GroupBy::Kind, DateUnit::Month, |_| None);
        assert_eq!(groups.len(), 1, "phần mở rộng khác hoa thường là cùng nhóm");
        assert_eq!(groups[0].label, "TXT");
    }

    #[test]
    fn a_file_without_extension_has_its_own_group() {
        let groups = group(
            &[entry("README", None, None)],
            GroupBy::Kind,
            DateUnit::Month,
            |_| None,
        );
        assert_eq!(groups[0].label, "Không phần mở rộng");
    }

    #[test]
    fn tag_group_uses_the_callback_and_falls_back() {
        let items = vec![entry("a.txt", None, None), entry("b.txt", None, None)];
        let groups = group(&items, GroupBy::Tag, DateUnit::Month, |e| {
            if e.name.starts_with('a') {
                Some("Home".into())
            } else {
                None
            }
        });
        let labels: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, vec!["Home", "Chưa gắn thẻ"]);
    }

    #[test]
    fn a_missing_date_groups_as_unknown() {
        let groups = group(
            &[entry("a.txt", None, None)],
            GroupBy::DateModified,
            DateUnit::Month,
            |_| None,
        );
        assert_eq!(groups[0].label, "Không rõ");
    }

    #[test]
    fn date_labels_respect_the_unit() {
        // 2026-11-19 ≈ 1_769_000_000_000 ms.
        let ms = 1_769_000_000_000i64;
        let year = group(
            &[entry("a.txt", None, Some(ms))],
            GroupBy::DateModified,
            DateUnit::Year,
            |_| None,
        );
        assert_eq!(year[0].label, "2026");
        let month = group(
            &[entry("a.txt", None, Some(ms))],
            GroupBy::DateModified,
            DateUnit::Month,
            |_| None,
        );
        assert!(month[0].label.ends_with("/2026"));
    }

    #[test]
    fn recycle_only_criteria_are_flagged() {
        // Menu thường không được hiện hai tiêu chí này: ngoài Thùng rác không có
        // dữ liệu để nhóm theo, chọn vào sẽ ra một nhóm duy nhất vô nghĩa.
        assert!(GroupBy::OriginalFolder.recycle_only());
        assert!(GroupBy::DeletedDate.recycle_only());
        assert!(!GroupBy::Name.recycle_only());
        assert!(!GroupBy::Tag.recycle_only());
        assert!(!GroupBy::None.recycle_only());
    }

    #[test]
    fn deleted_date_groups_by_the_modified_field() {
        // Trong tab Thùng rác, `modified_ms` giữ thời điểm xoá.
        let items = [
            entry("a.txt", None, Some(1_769_000_000_000)),
            entry("b.txt", None, Some(1_769_000_000_000)),
            entry("c.txt", None, None),
        ];
        let groups = group(&items, GroupBy::DeletedDate, DateUnit::Year, |_| None);
        let labels: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, vec!["2026", "Không rõ"]);
        assert_eq!(groups[0].indices, vec![0, 1]);
    }

    #[test]
    fn original_folder_groups_by_the_path_field() {
        // Nơi gọi thay `Entry.path` bằng thư mục cha của đường dẫn gốc trước khi
        // gọi `group`, nên ở đây chỉ cần `path` được dùng làm khoá.
        let mut a = entry("a.txt", None, None);
        a.path = r"C:\Users\me\Desktop".into();
        let mut b = entry("b.txt", None, None);
        b.path = r"C:\Users\me\Desktop".into();
        let mut c = entry("c.txt", None, None);
        c.path = r"C:\Users\me\Documents".into();

        let groups = group(&[a, b, c], GroupBy::OriginalFolder, DateUnit::Month, |_| {
            None
        });
        let labels: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![r"C:\Users\me\Desktop", r"C:\Users\me\Documents"]
        );
        assert_eq!(groups[0].indices, vec![0, 1]);
    }

    #[test]
    fn an_empty_original_path_gets_a_placeholder_label() {
        // `$I` hỏng có thể để lại đường dẫn rỗng; không được hiện header trống.
        let mut e = entry("a.txt", None, None);
        e.path = String::new();
        let groups = group(&[e], GroupBy::OriginalFolder, DateUnit::Month, |_| None);
        assert_eq!(groups[0].label, "Không rõ");
    }

    #[test]
    fn every_index_appears_exactly_once() {
        // Mất hoặc lặp chỉ số sẽ làm một hàng hiện hai lần hoặc biến mất.
        let items: Vec<Entry> = (0..50)
            .map(|i| {
                entry(
                    &format!("tep-{i}.txt"),
                    Some(i * 100),
                    Some(1_769_000_000_000),
                )
            })
            .collect();
        for by in GroupBy::all() {
            let groups = group(&items, *by, DateUnit::Month, |_| None);
            let total: usize = groups.iter().map(|g| g.indices.len()).sum();
            assert_eq!(total, items.len(), "nhóm {by:?} làm mất mục");
            let mut all: Vec<usize> = groups.iter().flat_map(|g| g.indices.clone()).collect();
            all.sort_unstable();
            all.dedup();
            assert_eq!(all.len(), items.len(), "nhóm {by:?} bị lặp chỉ số");
        }
    }
}
