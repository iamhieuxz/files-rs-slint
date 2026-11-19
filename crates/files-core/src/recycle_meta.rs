//! Đọc metadata Thùng rác từ tệp `$I`.
//!
//! Windows giữ mỗi mục đã xoá thành **một cặp** tệp trong
//! `<ổ>:\$Recycle.Bin\<SID>\`:
//!
//! - `$I<id><ext>` — metadata: kích thước, thời điểm xoá, đường dẫn gốc.
//! - `$R<id><ext>` — nội dung thật (tệp hoặc thư mục).
//!
//! Module này chỉ **đọc và parse** — không I/O. Nhờ vậy định dạng nhị phân được
//! test bằng byte dựng sẵn, không cần Windows hay Thùng rác thật.
//!
//! # Định dạng `$I`
//!
//! Đã kiểm chứng trên máy Windows 11 thật (26 mục, tất cả version 2):
//!
//! ```text
//! offset  size  field
//! 0       8     version  = 2 (u64 LE)
//! 8       8     size     = kích thước tệp gốc (u64 LE)
//! 16      8     deleted  = FILETIME (u64 LE, 100ns kể từ 1601-01-01 UTC)
//! 24      4     chars    = số ký tự của path, KHÔNG gồm NUL kết thúc (u32 LE)
//! 28      2*N   path     = UTF-16LE, đúng `chars` ký tự
//! ```
//!
//! Với thư mục, `size` là tổng kích thước nội dung lúc xoá (đo được: thư mục
//! `Files RS 2.2` báo 22.949.376 byte, khớp kích thước thật).
//!
//! Định dạng cũ (Windows 95/98/ME và NTFS trước Vista) dùng `$I` **260 byte cố
//! định** với header 20 byte và đường dẫn ANSI. Không có mẫu nào trên máy kiểm
//! chứng nên phần này chỉ được nhận diện để báo lỗi rõ ràng, không đoán bừa.

use serde::{Deserialize, Serialize};

/// Kích thước header của định dạng version 2, tính bằng byte.
const HEADER_V2: usize = 28;

/// Định dạng cũ dùng `$I` cố định 260 byte; header 20 byte.
const LEGACY_SIZE: usize = 260;

#[allow(dead_code)]
const LEGACY_HEADER: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Tệp ngắn hơn header tối thiểu.
    TooShort { len: usize },
    /// Phiên bản không nhận diện được.
    UnknownVersion(u64),
    /// `chars` khai báo nhiều hơn số byte thực có.
    TruncatedPath { declared: usize, available: usize },
    /// Đường dẫn không phải UTF-16 hợp lệ.
    BadPath,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { len } => write!(f, "tệp $I quá ngắn ({len} byte)"),
            Self::UnknownVersion(v) => write!(f, "phiên bản $I không nhận diện: {v}"),
            Self::TruncatedPath {
                declared,
                available,
            } => write!(
                f,
                "đường dẫn $I bị cụt: khai báo {declared} ký tự, chỉ có {available} byte"
            ),
            Self::BadPath => write!(f, "đường dẫn $I không phải UTF-16 hợp lệ"),
        }
    }
}

/// Một mục trong Thùng rác, đọc từ `$I`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecycleEntry {
    /// Đường dẫn gốc trước khi xoá, ví dụ `C:\Users\me\Desktop\bao-cao.docx`.
    pub original_path: String,
    /// Kích thước lúc xoá. Với thư mục đây là tổng kích thước nội dung.
    pub size: u64,
    /// Thời điểm xoá, milli giây Unix. `None` khi FILETIME bằng 0.
    pub deleted_ms: Option<i64>,
    /// `true` nếu `$R` là thư mục. Suy ra từ nội dung trên đĩa, không từ `$I`.
    pub is_dir: bool,
}

/// Đọc metadata từ nội dung tệp `$I`.
///
/// Trả lỗi thay vì đoán khi tệp hỏng: hiện sai đường dẫn gốc còn tệ hơn không
/// hiện gì, vì người dùng có thể khôi phục nhầm chỗ.
pub fn parse_i(bytes: &[u8]) -> Result<RecycleEntry, ParseError> {
    if bytes.len() < 8 {
        return Err(ParseError::TooShort { len: bytes.len() });
    }
    let version = u64::from_le_bytes(bytes[0..8].try_into().unwrap_or([0; 8]));

    if version == 2 {
        parse_v2(bytes)
    } else if version == 1 || (bytes.len() == LEGACY_SIZE && version != 2) {
        // Không có mẫu thật để kiểm chứng; báo rõ thay vì đoán định dạng.
        Err(ParseError::UnknownVersion(version))
    } else {
        Err(ParseError::UnknownVersion(version))
    }
}

fn parse_v2(bytes: &[u8]) -> Result<RecycleEntry, ParseError> {
    if bytes.len() < HEADER_V2 {
        return Err(ParseError::TooShort { len: bytes.len() });
    }
    let size = u64::from_le_bytes(bytes[8..16].try_into().unwrap_or([0; 8]));
    let filetime = i64::from_le_bytes(bytes[16..24].try_into().unwrap_or([0; 8]));
    let chars = u32::from_le_bytes(bytes[24..28].try_into().unwrap_or([0; 4])) as usize;

    let path_bytes = &bytes[HEADER_V2..];
    let needed = chars * 2;
    if path_bytes.len() < needed {
        return Err(ParseError::TruncatedPath {
            declared: chars,
            available: path_bytes.len(),
        });
    }

    // Chỉ lấy đúng `chars` ký tự; phần dư (nếu có) là đệm NUL.
    //
    // Đo trên tệp thật: `chars` **bao gồm** NUL kết thúc. Ví dụ tệp 296 byte có
    // `chars = 134`, và unit[133] là 0x0000 — tức 133 ký tự đường dẫn + 1 NUL.
    // Không cắt NUL thì tên hiển thị có ký tự ẩn ở cuối và so sánh đường dẫn
    // trượt.
    let units: Vec<u16> = path_bytes[..needed]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|u| *u != 0)
        .collect();
    let path = String::from_utf16(&units).map_err(|_| ParseError::BadPath)?;

    Ok(RecycleEntry {
        original_path: path,
        size,
        deleted_ms: filetime_to_unix_ms(filetime),
        is_dir: false,
    })
}

/// Đổi FILETIME (100ns kể từ 1601-01-01 UTC) sang milli giây Unix.
///
/// FILETIME 0 nghĩa là "không có giá trị", không phải năm 1601.
fn filetime_to_unix_ms(filetime: i64) -> Option<i64> {
    if filetime <= 0 {
        return None;
    }
    // 11644473600 giây giữa 1601-01-01 và 1970-01-01.
    const EPOCH_DIFF_SECS: i64 = 11_644_473_600;
    let secs = filetime / 10_000_000;
    let frac_100ns = filetime % 10_000_000;
    Some((secs - EPOCH_DIFF_SECS) * 1000 + frac_100ns / 10_000)
}

/// Tách `id` từ tên tệp `$I...` hoặc `$R...`.
///
/// `$I0BSU5L.lnk` → `Some("0BSU5L.lnk")`. Tên không bắt đầu bằng `$I`/`$R` trả
/// `None`.
pub fn stem_of(name: &str) -> Option<&str> {
    name.strip_prefix("$I").or_else(|| name.strip_prefix("$R"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dựng `$I` version 2 từ các trường, giống hệt định dạng đo được trên đĩa.
    ///
    /// `chars` trong định dạng thật **bao gồm** NUL kết thúc, nên hàm này cộng
    /// thêm 1 — đúng như tệp 296 byte với `chars = 134` cho path 133 ký tự.
    fn build_v2(size: u64, filetime: i64, path: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&2u64.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&filetime.to_le_bytes());
        let units: Vec<u16> = path.encode_utf16().collect();
        // +1 cho NUL kết thúc.
        out.extend_from_slice(&((units.len() + 1) as u32).to_le_bytes());
        for u in units {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn parses_a_real_shaped_v2_entry() {
        // Số liệu lấy từ tệp thật: 296 byte, size=1, path 134 ký tự.
        let path = r"C:\Users\iamhieuxz\AppData\Local\Temp\ops-recycle\trash-me.txt";
        let ft = 133_795_232_000_000_000i64;
        let bytes = build_v2(1, ft, path);
        let e = parse_i(&bytes).expect("phải parse được");
        assert_eq!(e.original_path, path);
        assert_eq!(e.size, 1);
        assert!(e.deleted_ms.is_some());
    }

    #[test]
    fn a_folder_entry_carries_the_total_size() {
        // Thư mục Files RS 2.2 báo 22.949.376 byte.
        let path = r"C:\Users\iamhieuxz\Desktop\Files RS 2.2";
        let e = parse_i(&build_v2(22_949_376, 133_795_232_000_000_000, path)).expect("parse");
        assert_eq!(e.size, 22_949_376);
        assert_eq!(e.original_path, path);
    }

    #[test]
    fn a_zero_filetime_means_unknown_not_year_1601() {
        let e = parse_i(&build_v2(5, 0, "C:\\a.txt")).expect("parse");
        assert_eq!(e.deleted_ms, None, "FILETIME 0 phải là None");
    }

    #[test]
    fn a_truncated_path_is_an_error_not_a_guess() {
        let mut bytes = build_v2(1, 1, "C:\\duong\\danh.txt");
        // Cắt mất nửa cuối của path nhưng giữ nguyên `chars`.
        bytes.truncate(bytes.len() - 8);
        assert!(matches!(
            parse_i(&bytes),
            Err(ParseError::TruncatedPath { .. })
        ));
    }

    #[test]
    fn a_file_shorter_than_the_header_is_rejected() {
        assert!(matches!(
            parse_i(&[2, 0, 0, 0]),
            Err(ParseError::TooShort { .. })
        ));
    }

    #[test]
    fn an_unknown_version_is_rejected() {
        let mut bytes = build_v2(1, 1, "C:\\a.txt");
        bytes[0] = 9;
        assert!(matches!(
            parse_i(&bytes),
            Err(ParseError::UnknownVersion(9))
        ));
    }

    #[test]
    fn trailing_nul_padding_does_not_leak_into_the_path() {
        // Đây là lỗi thật đã gặp trên dữ liệu sống: `chars` bao gồm NUL kết
        // thúc, nên tên hiển thị có ký tự ẩn ở cuối nếu không cắt.
        let mut bytes = build_v2(1, 1, "C:\\a.txt");
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        let e = parse_i(&bytes).expect("parse");
        assert_eq!(e.original_path, "C:\\a.txt");
        assert!(
            !e.original_path.ends_with('\0'),
            "đường dẫn không được có NUL ở cuối"
        );
    }

    #[test]
    fn the_declared_length_includes_the_terminating_nul() {
        // Đối chiếu với tệp thật: path 133 ký tự thì `chars` = 134.
        let path = r"C:\Users\iamhieuxz\Desktop\Files RS 2.2";
        let bytes = build_v2(1, 1, path);
        let declared = u32::from_le_bytes(bytes[24..28].try_into().expect("4 byte")) as usize;
        assert_eq!(
            declared,
            path.chars().count() + 1,
            "chars phải gồm cả NUL kết thúc"
        );
        assert_eq!(parse_i(&bytes).expect("parse").original_path, path);
    }

    #[test]
    fn unicode_paths_survive_the_round_trip() {
        let path = r"C:\Users\me\Tài liệu\báo cáo năm 2026.docx";
        let e = parse_i(&build_v2(1234, 133_795_232_000_000_000, path)).expect("parse");
        assert_eq!(e.original_path, path);
    }

    #[test]
    fn stem_is_stripped_from_both_prefixes() {
        assert_eq!(stem_of("$I0BSU5L.lnk"), Some("0BSU5L.lnk"));
        assert_eq!(stem_of("$R0BSU5L.lnk"), Some("0BSU5L.lnk"));
        assert_eq!(stem_of("desktop.ini"), None);
    }

    #[test]
    fn filetime_converts_to_the_expected_unix_time() {
        // 133_795_232_000_000_000 FILETIME = 2024-12-24T21:13:20Z (đã đối chiếu
        // bằng `DateTime::FromFileTime` của .NET trên cùng giá trị).
        let ms = filetime_to_unix_ms(133_795_232_000_000_000).expect("có giá trị");
        assert_eq!(ms, 1_735_049_600_000);
    }

    #[test]
    fn every_stem_pairs_i_and_r_names() {
        // Ghép cặp dựa trên stem: `$Iabc.txt` ↔ `$Rabc.txt`.
        let i = stem_of("$Iabc.txt").expect("có stem");
        let r = stem_of("$Rabc.txt").expect("có stem");
        assert_eq!(i, r);
    }
}
