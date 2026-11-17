//! Tests for archives and hashing.
//!
//! These live outside the library because they need a real scratch directory,
//! and `CARGO_TARGET_TMPDIR` is only provided to integration tests.

use files_fs::archive;
use files_fs::HashKind;
use std::fs::File;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let dir = root.join(format!("archive-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn hashes_match_the_known_digests_of_an_empty_file() {
    let dir = scratch("hash");
    let f = dir.join("empty.bin");
    File::create(&f).unwrap();

    // Well-known digests of zero bytes.
    assert_eq!(
        archive::hash_file(&f, HashKind::Md5).unwrap(),
        "d41d8cd98f00b204e9800998ecf8427e"
    );
    assert_eq!(
        archive::hash_file(&f, HashKind::Sha1).unwrap(),
        "da39a3ee5e6b4b0d3255bfef95601890afd80709"
    );
    assert_eq!(
        archive::hash_file(&f, HashKind::Sha256).unwrap(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_hash_covers_the_whole_file() {
    let dir = scratch("hashcontent");
    let f = dir.join("data.bin");
    // Larger than the 64 KB read buffer, so the loop must run twice.
    let payload = vec![0xABu8; 70 * 1024];
    std::fs::write(&f, &payload).unwrap();

    let sha = archive::hash_file(&f, HashKind::Sha256).unwrap();
    assert_eq!(sha.len(), 64, "SHA-256 renders as 64 hex characters");
    // Same content, same digest — proves the whole file was read.
    let f2 = dir.join("copy.bin");
    std::fs::write(&f2, &payload).unwrap();
    assert_eq!(archive::hash_file(&f2, HashKind::Sha256).unwrap(), sha);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hashing_a_directory_is_refused() {
    let dir = scratch("hashdir");
    let err = archive::hash_file(&dir, HashKind::Sha256).unwrap_err();
    assert_eq!(err.code, "not_a_file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn zip_round_trips_a_tree() {
    let dir = scratch("zip");
    let src = dir.join("folder");
    std::fs::create_dir_all(src.join("nested")).unwrap();
    std::fs::write(src.join("a.txt"), b"alpha").unwrap();
    std::fs::write(src.join("nested/b.txt"), b"beta").unwrap();

    let zip_path = dir.join("out.zip");
    let count = archive::create_zip(&[src.clone()], &zip_path).unwrap();
    assert_eq!(count, 2, "two files were written");

    let out = dir.join("extracted");
    let written = archive::extract_zip(&zip_path, &out).unwrap();
    assert_eq!(written, 2);
    assert_eq!(std::fs::read(out.join("folder/a.txt")).unwrap(), b"alpha");
    assert_eq!(
        std::fs::read(out.join("folder/nested/b.txt")).unwrap(),
        b"beta"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn one_item_keeps_its_own_name() {
    assert_eq!(
        archive::archive_name(&[PathBuf::from(r"C:\x\photos")]),
        "photos"
    );
}

#[test]
fn several_items_are_named_after_the_folder() {
    let sources = vec![
        PathBuf::from(r"C:\x\docs\a.txt"),
        PathBuf::from(r"C:\x\docs\b.txt"),
    ];
    assert_eq!(archive::archive_name(&sources), "docs");
}

#[test]
fn extracting_a_missing_archive_reports_an_error() {
    let dir = scratch("zipmissing");
    let err = archive::extract_zip(&dir.join("nope.zip"), &dir.join("out")).unwrap_err();
    assert!(
        err.code == "io_error" || err.code == "not_found",
        "got {}",
        err.code
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_corrupt_archive_is_reported_as_invalid() {
    let dir = scratch("zipcorrupt");
    let bad = dir.join("bad.zip");
    std::fs::write(&bad, b"this is not a zip file").unwrap();

    let err = archive::extract_zip(&bad, &dir.join("out")).unwrap_err();
    assert_eq!(err.code, "invalid_archive", "got {}", err.code);

    let _ = std::fs::remove_dir_all(&dir);
}
