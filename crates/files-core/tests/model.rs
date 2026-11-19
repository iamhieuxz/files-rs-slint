//! Behavioural tests for the data model.
//!
//! These run without a filesystem, a window, or WebView2: the rules that decide
//! what the user sees are verified here.

use files_core::entry::{Entry, EntryKind};
use files_core::path::{breadcrumbs, normalize};
use files_core::sort::{filter, natural_cmp, ListOptions, SortBy, SortDirection, SortSpec};
use std::cmp::Ordering;
use std::path::Path;

fn entry(name: &str, kind: EntryKind, size: Option<u64>) -> Entry {
    Entry {
        name: name.to_string(),
        path: format!("C:\\tmp\\{name}"),
        kind,
        size,
        modified_ms: None,
        created_ms: None,
        extension: name.rsplit_once('.').map(|(_, e)| e.to_string()),
        is_hidden: false,
        is_readonly: false,
    }
}

#[test]
fn natural_order_puts_file2_before_file10() {
    // The whole reason for a natural comparator: a plain string sort would put
    // "file10" before "file2", which looks broken to a user.
    assert_eq!(natural_cmp("file2.txt", "file10.txt"), Ordering::Less);
    assert_eq!(natural_cmp("file10.txt", "file2.txt"), Ordering::Greater);
    assert_eq!(natural_cmp("img9", "img9"), Ordering::Equal);
}

#[test]
fn natural_order_is_case_insensitive_but_not_ambiguous() {
    assert_eq!(
        natural_cmp("Apple", "apple"),
        Ordering::Less,
        "case breaks the tie"
    );
    assert_eq!(natural_cmp("alpha", "BETA"), Ordering::Less);
}

#[test]
fn natural_order_handles_prefixes() {
    assert_eq!(natural_cmp("a", "ab"), Ordering::Less);
    assert_eq!(natural_cmp("ab", "a"), Ordering::Greater);
    assert_eq!(natural_cmp("", "a"), Ordering::Less);
}

#[test]
fn folders_sort_before_files_by_default() {
    let mut rows = vec![
        entry("zzz.txt", EntryKind::File, Some(1)),
        entry("aaa", EntryKind::Directory, None),
    ];
    let spec = SortSpec::default();
    spec.sort(&mut rows);
    assert_eq!(rows[0].name, "aaa", "folder must lead");
    assert_eq!(rows[1].name, "zzz.txt");
}

#[test]
fn folders_first_can_be_disabled() {
    let mut rows = vec![
        entry("aaa.txt", EntryKind::File, Some(1)),
        entry("zzz", EntryKind::Directory, None),
    ];
    let spec = SortSpec {
        folder_order: files_core::sort::FolderOrder::Together,
        ..Default::default()
    };
    spec.sort(&mut rows);
    assert_eq!(rows[0].name, "aaa.txt", "pure name order when disabled");
}

#[test]
fn size_sort_puts_directories_last_and_orders_ascending() {
    let mut rows = vec![
        entry("big.bin", EntryKind::File, Some(900)),
        entry("dir", EntryKind::Directory, None),
        entry("small.txt", EntryKind::File, Some(5)),
    ];
    // folders_first off so the size column is the only ordering.
    let spec = SortSpec {
        by: SortBy::Size,
        folder_order: files_core::sort::FolderOrder::Together,
        ..Default::default()
    };
    spec.sort(&mut rows);
    assert_eq!(rows[0].name, "small.txt");
    assert_eq!(rows[1].name, "big.bin");
    assert_eq!(rows[2].name, "dir", "unknown size sorts last");
}

#[test]
fn descending_reverses_the_primary_key() {
    let mut rows = vec![
        entry("a.txt", EntryKind::File, Some(1)),
        entry("b.txt", EntryKind::File, Some(2)),
    ];
    let spec = SortSpec {
        direction: SortDirection::Descending,
        folder_order: files_core::sort::FolderOrder::Together,
        ..Default::default()
    };
    spec.sort(&mut rows);
    assert_eq!(rows[0].name, "b.txt");
}

#[test]
fn sort_is_deterministic_for_equal_keys() {
    // Two entries with identical size must still order the same way every run,
    // otherwise the list reshuffles on every refresh.
    let make = || {
        vec![
            entry("b.txt", EntryKind::File, Some(7)),
            entry("a.txt", EntryKind::File, Some(7)),
        ]
    };
    let spec = SortSpec {
        by: SortBy::Size,
        folder_order: files_core::sort::FolderOrder::Together,
        ..Default::default()
    };
    let mut first = make();
    let mut second = make();
    spec.sort(&mut first);
    spec.sort(&mut second);
    let names = |v: &[Entry]| v.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&first), names(&second));
}

#[test]
fn hidden_filter_drops_flagged_entries() {
    let mut hidden = entry("secret.txt", EntryKind::File, Some(1));
    hidden.is_hidden = true;
    let rows = vec![entry("visible.txt", EntryKind::File, Some(1)), hidden];

    let shown = filter(rows.clone(), &ListOptions::default());
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].name, "visible.txt");

    let all = filter(
        rows,
        &ListOptions {
            show_hidden: true,
            show_dotfiles: true,
        },
    );
    assert_eq!(all.len(), 2);
}

#[test]
fn dotfiles_are_filtered_independently_of_the_hidden_flag() {
    let rows = vec![
        entry(".gitignore", EntryKind::File, Some(1)),
        entry("main.rs", EntryKind::File, Some(1)),
    ];
    let shown = filter(
        rows.clone(),
        &ListOptions {
            show_hidden: true,
            show_dotfiles: false,
        },
    );
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].name, "main.rs");
}

#[test]
fn normalize_resolves_dot_segments_without_touching_the_disk() {
    assert_eq!(normalize(r"C:\a\b\..\c"), Path::new(r"C:\a\c"));
    assert_eq!(normalize(r"C:\a\.\b"), Path::new(r"C:\a\b"));
    // Must not climb above the drive root.
    assert_eq!(normalize(r"C:\..\..\x"), Path::new(r"C:\x"));
}

#[test]
fn breadcrumbs_split_a_drive_path() {
    let crumbs = breadcrumbs(Path::new(r"C:\Users\me\Documents"));
    let labels: Vec<&str> = crumbs.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, vec!["C:", "Users", "me", "Documents"]);
    // Every crumb must be a navigable absolute path.
    assert_eq!(crumbs[0].path, r"C:\");
    assert_eq!(crumbs[3].path, r"C:\Users\me\Documents");
}

#[test]
fn breadcrumbs_of_a_root_are_a_single_crumb() {
    let crumbs = breadcrumbs(Path::new(r"C:\"));
    assert_eq!(crumbs.len(), 1);
    assert_eq!(crumbs[0].path, r"C:\");
}
