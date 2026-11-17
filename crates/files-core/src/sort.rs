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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortSpec {
    pub by: SortBy,
    pub direction: SortDirection,
    /// Windows Explorer default: folders are grouped before files regardless of
    /// the active column. Files app exposes this as "Sort folders before files".
    pub folders_first: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self { by: SortBy::Name, direction: SortDirection::Ascending, folders_first: true }
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
            (Some((_, ac)), Some((_, bc))) => {
                if ac.is_ascii_digit() && bc.is_ascii_digit() {
                    // Compare the full numeric run by value, then by length so
                    // that equal values keep a stable, deterministic order.
                    let (an, a_len) = take_number(a, ai.peek().unwrap().0);
                    let (bn, b_len) = take_number(b, bi.peek().unwrap().0);
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
        if self.folders_first {
            let folder_ord = b.is_dir().cmp(&a.is_dir());
            if folder_ord != Ordering::Equal {
                return folder_ord;
            }
        }

        let ord = match self.by {
            SortBy::Name => natural_cmp(&a.name, &b.name),
            SortBy::Size => cmp_option(a.size, b.size),
            SortBy::DateModified => cmp_option(a.modified_ms, b.modified_ms),
            SortBy::DateCreated => cmp_option(a.created_ms, b.created_ms),
            SortBy::Extension => cmp_option(a.ext_lower(), b.ext_lower())
                .then_with(|| natural_cmp(&a.name, &b.name)),
            SortBy::Kind => {
                let rank = |e: &Entry| match e.kind {
                    crate::entry::EntryKind::Directory => 0u8,
                    crate::entry::EntryKind::File => 1,
                    crate::entry::EntryKind::Symlink => 2,
                    crate::entry::EntryKind::Other => 3,
                };
                rank(a).cmp(&rank(b)).then_with(|| natural_cmp(&a.name, &b.name))
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
