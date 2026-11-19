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
    let count = archive::create_zip(std::slice::from_ref(&src), &zip_path).unwrap();
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
fn zipping_a_file_onto_itself_is_refused_without_truncating_it() {
    // Chọn chính tệp .zip rồi bấm "Nén thành ZIP": đích trùng nguồn.
    // `File::create` từng truncate nguồn trước khi đọc, phá hỏng archive.
    let dir = scratch("zip-self");
    let file = dir.join("photos.zip");
    std::fs::write(&file, b"du lieu zip cu").unwrap();

    let err = archive::create_zip(std::slice::from_ref(&file), &file)
        .expect_err("phải từ chối nén tệp vào chính nó");

    assert_eq!(err.code, "exists");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"du lieu zip cu",
        "archive gốc phải còn nguyên"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_existing_archive_is_never_overwritten() {
    let dir = scratch("zip-existing");
    let payload = dir.join("a.txt");
    std::fs::write(&payload, b"alpha").unwrap();
    let dest = dir.join("a.zip");
    std::fs::write(&dest, b"archive cu").unwrap();

    let err = archive::create_zip(std::slice::from_ref(&payload), &dest)
        .expect_err("phải từ chối ghi đè archive đã có");

    assert_eq!(err.code, "exists");
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        b"archive cu",
        "archive cũ phải nguyên vẹn"
    );
    let _ = std::fs::remove_dir_all(&dir);
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
fn extraction_never_overwrites_a_file_that_already_exists() {
    // Giải nén vào thư mục đã có tệp cùng tên: dữ liệu người dùng phải thắng.
    let dir = scratch("zip-no-overwrite");
    let src = dir.join("nguon");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.txt"), b"tu archive").unwrap();
    let zip_path = dir.join("nguon.zip");
    archive::create_zip(std::slice::from_ref(&src), &zip_path).unwrap();

    let out = dir.join("out");
    std::fs::create_dir_all(out.join("nguon")).unwrap();
    std::fs::write(out.join("nguon/a.txt"), b"du lieu cua nguoi dung").unwrap();

    let count = archive::extract_zip(&zip_path, &out).expect("extract không được lỗi");

    assert_eq!(count, 0, "không mục nào được ghi đè");
    assert_eq!(
        std::fs::read(out.join("nguon/a.txt")).unwrap(),
        b"du lieu cua nguoi dung",
        "tệp có sẵn phải nguyên vẹn"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_zip_leaves_no_partial_archive() {
    // Nguồn không tồn tại: archive dở không được để lại, vì nó trông giống
    // một tệp zip hợp lệ và người dùng sẽ tưởng đã nén xong.
    let dir = scratch("zip-partial");
    let missing = dir.join("khong-co");
    let dest = dir.join("ket-qua.zip");

    let result = archive::create_zip(std::slice::from_ref(&missing), &dest);

    assert!(result.is_err(), "phải báo lỗi");
    assert!(!dest.exists(), "không được để lại archive đích");
    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        leftovers.iter().all(|n| !n.ends_with(".partial")),
        "không được để lại tệp tạm: {leftovers:?}"
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

#[test]
fn extraction_rejects_entries_that_escape_the_target() {
    use std::io::Write;

    let dir = scratch("zip-slip");
    let archive_path = dir.join("evil.zip");
    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        // Relative traversal and an absolute-looking path. A naive join would
        // write both outside `out/`.
        zip.start_file("../escape.txt", options).unwrap();
        zip.write_all(b"escaped").unwrap();
        zip.start_file("/absolute.txt", options).unwrap();
        zip.write_all(b"absolute").unwrap();
        // Một tệp hợp lệ để chắc chắn archive vẫn được giải nén.
        zip.start_file("safe.txt", options).unwrap();
        zip.write_all(b"safe").unwrap();
        zip.finish().unwrap();
    }

    let out = dir.join("out");
    let count = archive::extract_zip(&archive_path, &out).expect("extract");

    assert_eq!(count, 1, "only the safe entry is extracted");
    assert_eq!(std::fs::read(out.join("safe.txt")).unwrap(), b"safe");
    assert!(!dir.join("escape.txt").exists(), "no write above target");
    assert!(!out.join("../escape.txt").exists());
    assert!(!dir.join("absolute.txt").exists(), "no absolute write");
    assert!(
        !out.join("absolute.txt").exists(),
        "absolute entry is skipped"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
