//! Integration tests against a real temporary directory.
//!
//! These prove the storage layer produces the values the tests in
//! `files-core` assume, on an actual NTFS/filesystem.

use files_core::sort::{ListOptions, SortBy, SortSpec};
use files_core::EntryKind;

fn scratch(name: &str) -> std::path::PathBuf {
    // `CARGO_TARGET_TMPDIR` is provided by cargo for integration tests and lives
    // inside the build directory, so the tests do not depend on write access to
    // the system temp directory.
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let dir = root.join(format!("scratch-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn reads_entries_with_metadata() {
    let dir = scratch("basic");
    std::fs::create_dir(dir.join("subfolder")).unwrap();
    std::fs::write(dir.join("alpha.txt"), b"hello").unwrap();
    std::fs::write(dir.join("beta.log"), b"12345").unwrap();

    let listing = files_fs::read_dir(&dir, SortSpec::default(), ListOptions::default()).unwrap();

    assert_eq!(listing.entries.len(), 3);
    // Folders first, then natural name order.
    assert_eq!(listing.entries[0].name, "subfolder");
    assert_eq!(listing.entries[0].kind, EntryKind::Directory);
    assert!(
        listing.entries[0].size.is_none(),
        "a folder reports no size"
    );
    assert_eq!(listing.entries[1].name, "alpha.txt");
    assert_eq!(listing.entries[1].size, Some(5));
    assert_eq!(listing.entries[1].extension.as_deref(), Some("txt"));

    assert!(listing.partial_errors.is_empty());
    assert!(
        listing.location.path.ends_with("scratch-basic"),
        "location path should be the directory we read, got {}",
        listing.location.path
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reports_hidden_entries_and_its_count() {
    let dir = scratch("hidden");
    std::fs::write(dir.join(".dotfile"), b"x").unwrap();
    std::fs::write(dir.join("plain.txt"), b"x").unwrap();

    let hidden = files_fs::read_dir(&dir, SortSpec::default(), ListOptions::default()).unwrap();
    assert_eq!(hidden.entries.len(), 1);
    assert_eq!(
        hidden.hidden_count, 1,
        "the filtered entry is still counted"
    );

    let all = files_fs::read_dir(
        &dir,
        SortSpec::default(),
        ListOptions {
            show_hidden: true,
            show_dotfiles: true,
        },
    )
    .unwrap();
    assert_eq!(all.entries.len(), 2);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_path_is_not_found_not_a_panic() {
    let err = files_fs::read_dir(
        std::path::Path::new(r"C:\definitely\not\here\at\all"),
        SortSpec::default(),
        ListOptions::default(),
    )
    .unwrap_err();
    assert_eq!(err.code, "not_found");
}

#[test]
fn a_file_is_rejected_as_a_directory() {
    let dir = scratch("notadir");
    let file = dir.join("thing.txt");
    std::fs::write(&file, b"x").unwrap();

    let err = files_fs::read_dir(&file, SortSpec::default(), ListOptions::default()).unwrap_err();
    assert_eq!(err.code, "not_a_directory");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sorting_by_size_changes_the_order() {
    let dir = scratch("sizesort");
    std::fs::write(dir.join("big.bin"), vec![0u8; 4096]).unwrap();
    std::fs::write(dir.join("tiny.txt"), b"1").unwrap();

    let listing = files_fs::read_dir(
        &dir,
        SortSpec {
            by: SortBy::Size,
            folders_first: false,
            ..Default::default()
        },
        ListOptions::default(),
    )
    .unwrap();

    assert_eq!(listing.entries[0].name, "tiny.txt");
    assert_eq!(listing.entries[1].name, "big.bin");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn drives_are_enumerable() {
    let drives = files_fs::list_drives();
    // Any Windows host has at least one volume mounted.
    assert!(!drives.is_empty(), "expected at least one drive");
    for d in &drives {
        assert!(!d.path.is_empty());
        assert!(d.free_bytes <= d.total_bytes);
        assert!((0.0..=1.0).contains(&d.used_ratio()));
    }
}

#[test]
fn watching_a_directory_is_possible() {
    let dir = scratch("watch");
    let handle = files_fs::watch_directory(&dir).expect("watch should start");
    // A change must be observed within a generous window: notify on Windows is
    // asynchronous and the first event can lag.
    std::fs::write(dir.join("new.txt"), b"x").unwrap();
    let mut seen = false;
    for _ in 0..20 {
        if handle
            .recv_timeout(std::time::Duration::from_millis(250))
            .is_some()
        {
            seen = true;
            break;
        }
    }
    assert!(seen, "expected a change notification");
    let _ = std::fs::remove_dir_all(&dir);
}
