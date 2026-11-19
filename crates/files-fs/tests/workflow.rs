//! Một luồng công việc đầy đủ trên thư mục tạm.
//!
//! Các test riêng lẻ đã phủ từng thao tác; test này nối chúng lại theo đúng
//! thứ tự người dùng làm: tạo, đổi tên, sao chép, di chuyển, nén, giải nén,
//! xoá. Nếu một bước để lại trạng thái sai, bước sau sẽ lộ ra.

use files_fs::{archive, ops};
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let dir = root.join(format!("workflow-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn assert_file(path: &Path, expected: &[u8]) {
    assert!(path.is_file(), "missing file: {}", path.display());
    assert_eq!(std::fs::read(path).unwrap(), expected);
}

#[test]
fn create_rename_copy_move_zip_extract_delete() {
    let root = scratch("all");
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();

    // Tạo thư mục và tệp, rồi ghi nội dung như người dùng vừa tạo.
    let folder = ops::create_folder(&work, "alpha").expect("create folder");
    let file = ops::create_file(&folder, "note.txt").expect("create file");
    std::fs::write(&file, b"hello workflow").unwrap();

    // Đổi tên trong cùng thư mục.
    let renamed = ops::rename(&file, "renamed.txt").expect("rename");
    assert_eq!(renamed, folder.join("renamed.txt"));
    assert!(!file.exists());
    assert_file(&renamed, b"hello workflow");

    // Sao chép cả thư mục sang đích mới.
    let copy_dest = root.join("copy-dest");
    std::fs::create_dir_all(&copy_dest).unwrap();
    let copied = ops::copy_into(&folder, &copy_dest).expect("copy");
    assert_eq!(copied, copy_dest.join("alpha"));
    assert_file(&copied.join("renamed.txt"), b"hello workflow");
    assert_file(&renamed, b"hello workflow");

    // Di chuyển bản sao sang đích khác; nguồn phải biến mất.
    let move_dest = root.join("move-dest");
    std::fs::create_dir_all(&move_dest).unwrap();
    let moved = ops::move_into(&copied, &move_dest).expect("move");
    assert_eq!(moved, move_dest.join("alpha"));
    assert!(!copied.exists());
    assert_file(&moved.join("renamed.txt"), b"hello workflow");

    // Nén thư mục đã di chuyển rồi giải nén sang thư mục mới.
    let zip_path = root.join("alpha.zip");
    let written = archive::create_zip(std::slice::from_ref(&moved), &zip_path).expect("zip");
    assert_eq!(written, 1);
    assert!(zip_path.is_file());
    let out = root.join("extracted");
    let extracted = archive::extract_zip(&zip_path, &out).expect("extract");
    assert_eq!(extracted, 1);
    assert_file(&out.join("alpha/renamed.txt"), b"hello workflow");

    // Xoá vĩnh viễn cả tệp nén và cây đã giải nén.
    let removed =
        ops::delete_permanently(&[zip_path.clone(), out.clone()]).expect("delete permanently");
    assert_eq!(removed, 2);
    assert!(!zip_path.exists());
    assert!(!out.exists());

    let _ = std::fs::remove_dir_all(&root);
}
