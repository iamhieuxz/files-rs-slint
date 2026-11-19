//! Sort and filter rules.
//!
//! The original expresses these as `SortOption` / `SortDirection` / `GroupOption`
//! enums bound to the layout page. Here they are plain data so the same rules can
//! be tested without a UI and reused by any frontend.

use crate::entry::Entry;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortBy {
    Name,
    Size,
    DateModified,
    DateCreated,
    Extension,
    Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    fn flip(self, ord: Ordering) -> Ordering {
        match self {
            SortDirection::Ascending => ord,
            SortDirection::Descending => ord.reverse(),
        }
    }
}

/// Cách đối xử giữa thư mục và tệp khi sắp xếp.
///
/// Files gốc có ba action riêng: `SortFoldersFirstAction`,
/// `SortFilesFirstAction`, `SortFilesAndFoldersTogetherAction`. Trước đây bản
/// Rust chỉ có một cờ `folders_first`, nên hai hành vi còn lại không chọn được.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderOrder {
    /// Thư mục trước tệp, bất kể cột đang sắp xếp. Mặc định của Explorer.
    #[default]
    FoldersFirst,
    /// Tệp trước thư mục.
    FilesFirst,
    /// Không tách nhóm: sắp xếp thuần theo cột đang chọn.
    Together,
}

impl FolderOrder {
    /// Thứ hạng dùng làm khoá phụ; `None` nghĩa là không tách nhóm.
    fn rank(self, is_dir: bool) -> Option<u8> {
        match self {
            Self::FoldersFirst => Some(if is_dir { 0 } else { 1 }),
            Self::FilesFirst => Some(if is_dir { 1 } else { 0 }),
            Self::Together => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::FoldersFirst => "Thư mục trước",
            Self::FilesFirst => "Tệp trước",
            Self::Together => "Sắp chung",
        }
    }

    /// Chu kỳ ba trạng thái, dùng cho nút bấm lặp.
    pub fn cycle(self) -> Self {
        match self {
            Self::FoldersFirst => Self::FilesFirst,
            Self::FilesFirst => Self::Together,
            Self::Together => Self::FoldersFirst,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortSpec {
    pub by: SortBy,
    pub direction: SortDirection,
    /// Cách tách nhóm thư mục và tệp.
    pub folder_order: FolderOrder,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            by: SortBy::Name,
            direction: SortDirection::Ascending,
            folder_order: FolderOrder::FoldersFirst,
        }
    }
}

/// Case-insensitive natural comparison: `file2` sorts before `file10`.
///
/// The original uses a Win32 `StrCmpLogicalW` wrapper (`NaturalStringComparer`)
/// plus a locale collator. This is a portable approximation that matches
/// Explorer's ordering for ASCII names, which covers the common case; locale
/// collation is a documented gap.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut ai, mut bi) = (a.char_indices().peekable(), b.char_indices().peekable());

    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some((a_at, ac)), Some((b_at, bc))) => {
                if ac.is_ascii_digit() && bc.is_ascii_digit() {
                    // Compare the full numeric run by value, then by length so
                    // that equal values keep a stable, deterministic order.
                    let (an, a_len) = take_number(a, a_at);
                    let (bn, b_len) = take_number(b, b_at);
                    let ord = an.cmp(&bn).then(a_len.cmp(&b_len));
                    if ord != Ordering::Equal {
                        return ord;
                    }
                    advance(&mut ai, a, a_len);
                    advance(&mut bi, b, b_len);
                } else {
                    // Case-insensitive primary key; byte order breaks ties so
                    // `A` and `a` never compare equal and swap positions.
                    let ord = ac.to_ascii_lowercase().cmp(&bc.to_ascii_lowercase());
                    if ord != Ordering::Equal {
                        return ord;
                    }
                    if ac != bc {
                        return ac.cmp(&bc);
                    }
                    ai.next();
                    bi.next();
                }
            }
        }
    }
}

fn take_number(s: &str, start: usize) -> (u128, usize) {
    let mut value: u128 = 0;
    let mut len = 0usize;
    for ch in s[start..].chars() {
        match ch.to_digit(10) {
            Some(d) => {
                // Saturate rather than overflow on absurd digit runs.
                value = value.saturating_mul(10).saturating_add(d as u128);
                len += ch.len_utf8();
            }
            None => break,
        }
    }
    (value, len)
}

fn advance<'a>(it: &mut std::iter::Peekable<std::str::CharIndices<'a>>, s: &str, len: usize) {
    let target = it.peek().map(|(i, _)| *i + len).unwrap_or(s.len());
    while it.peek().map(|(i, _)| *i < target).unwrap_or(false) {
        it.next();
    }
}

fn cmp_option<T: Ord>(a: Option<T>, b: Option<T>) -> Ordering {
    // Missing values sort last in ascending order, matching Explorer.
    match (a, b) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

impl SortSpec {
    pub fn compare(&self, a: &Entry, b: &Entry) -> Ordering {
        // Nhóm thư mục/tệp là khoá **phụ**: nó quyết định trước cột đang sắp
        // xếp, nhưng đảo chiều cùng lúc với cột để hành vi khớp Explorer.
        if let (Some(ra), Some(rb)) = (
            self.folder_order.rank(a.is_dir()),
            self.folder_order.rank(b.is_dir()),
        ) {
            let group = ra.cmp(&rb);
            if group != Ordering::Equal {
                return self.direction.flip(group);
            }
        }

        let ord = match self.by {
            SortBy::Name => natural_cmp(&a.name, &b.name),
            SortBy::Size => cmp_option(a.size, b.size),
            SortBy::DateModified => cmp_option(a.modified_ms, b.modified_ms),
            SortBy::DateCreated => cmp_option(a.created_ms, b.created_ms),
            SortBy::Extension => {
                cmp_option(a.ext_lower(), b.ext_lower()).then_with(|| natural_cmp(&a.name, &b.name))
            }
            SortBy::Kind => {
                let rank = |e: &Entry| match e.kind {
                    crate::entry::EntryKind::Directory => 0u8,
                    crate::entry::EntryKind::File => 1,
                    crate::entry::EntryKind::Symlink => 2,
                    crate::entry::EntryKind::Other => 3,
                };
                rank(a)
                    .cmp(&rank(b))
                    .then_with(|| natural_cmp(&a.name, &b.name))
            }
        };

        // Stable tiebreaker so paging and re-renders never reorder equal rows.
        self.direction
            .flip(ord)
            .then_with(|| natural_cmp(&a.name, &b.name))
    }

    pub fn sort(&self, entries: &mut [Entry]) {
        entries.sort_by(|a, b| self.compare(a, b));
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListOptions {
    /// Include entries whose `is_hidden` flag is set.
    pub show_hidden: bool,
    /// Include entries whose name starts with `.`.
    pub show_dotfiles: bool,
}

pub fn filter(entries: Vec<Entry>, opts: &ListOptions) -> Vec<Entry> {
    entries
        .into_iter()
        .filter(|e| {
            if !opts.show_hidden && e.is_hidden {
                return false;
            }
            if !opts.show_dotfiles && e.name.starts_with('.') {
                return false;
            }
            true
        })
        .collect()
}
