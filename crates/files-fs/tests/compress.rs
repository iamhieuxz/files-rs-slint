//! End-to-end check of the compress path the UI uses.
//!
//! The UI button calls `create_zip` with the selected paths and the current
//! folder; this test drives the same function with the same shape of input so a
//! regression in the app wiring shows up here without a window.

use files_fs::archive;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let dir = root.join(format!("compress-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn compressing_a_folder_creates_a_zip_next_to_it() {
    let dir = scratch("folder");
    let folder = dir.join("folder");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.txt"), b"alpha").unwrap();
    std::fs::write(folder.join("b.txt"), b"beta").unwrap();

    // Same steps as `compress_selection`: name from the selection, dest in the
    // current folder.
    let sources = vec![folder.clone()];
    let name = archive::archive_name(&sources);
    let dest = dir.join(format!("{name}.zip"));

    let written = archive::create_zip(&sources, &dest).expect("zip should succeed");
    assert_eq!(written, 2, "both files were added");
    assert!(dest.exists(), "the archive must exist on disk");
    assert!(
        std::fs::metadata(&dest).unwrap().len() > 0,
        "the archive must not be empty"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compressing_a_single_file_works() {
    let dir = scratch("file");
    let file = dir.join("notes.txt");
    std::fs::write(&file, b"hello").unwrap();

    let sources = vec![file.clone()];
    let dest = dir.join(format!("{}.zip", archive::archive_name(&sources)));
    archive::create_zip(&sources, &dest).expect("zip a single file");
    assert!(dest.exists());

    // The archive must actually contain the file, not just exist.
    let out = dir.join("out");
    archive::extract_zip(&dest, &out).unwrap();
    assert_eq!(std::fs::read(out.join("notes.txt")).unwrap(), b"hello");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_archive_is_deflated_not_stored() {
    // A stored (uncompressed) entry is a silent failure of the feature: the
    // user asks for a zip and gets a container the same size as the input.
    let dir = scratch("ratio");
    let file = dir.join("repetitive.txt");
    // 100 KB of the same byte compresses to almost nothing.
    std::fs::write(&file, vec![b'A'; 100 * 1024]).unwrap();

    let sources = vec![file.clone()];
    let dest = dir.join("out.zip");
    archive::create_zip(&sources, &dest).unwrap();

    let raw = std::fs::metadata(&file).unwrap().len();
    let zipped = std::fs::metadata(&dest).unwrap().len();
    assert!(
        zipped < raw / 2,
        "expected real compression: {raw} bytes in, {zipped} bytes out"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
