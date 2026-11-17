//! Behavioural tests for file operations.
//!
//! These run against a real temporary directory: the copy/move/rename rules are
//! the ones a user feels, so they are asserted on disk rather than mocked.

use files_fs::ops;

fn scratch(name: &str) -> std::path::PathBuf {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let dir = root.join(format!("ops-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn rename_keeps_the_entry_in_place() {
    let dir = scratch("rename");
    let file = dir.join("before.txt");
    std::fs::write(&file, b"x").unwrap();

    let renamed = ops::rename(&file, "after.txt").unwrap();
    assert_eq!(renamed, dir.join("after.txt"));
    assert!(renamed.exists());
    assert!(!file.exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rename_rejects_a_separator_in_the_name() {
    let dir = scratch("rename-bad");
    let file = dir.join("a.txt");
    std::fs::write(&file, b"x").unwrap();

    // A name with a slash would move the entry out of its folder silently.
    let err = ops::rename(&file, "sub/a.txt").unwrap_err();
    assert_eq!(err.code, "invalid_name");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rename_does_not_clobber_an_existing_entry() {
    let dir = scratch("rename-clash");
    std::fs::write(dir.join("a.txt"), b"a").unwrap();
    std::fs::write(dir.join("b.txt"), b"b").unwrap();

    let err = ops::rename(&dir.join("a.txt"), "b.txt").unwrap_err();
    assert_eq!(err.code, "exists");
    // Both originals survive untouched.
    assert_eq!(std::fs::read(dir.join("b.txt")).unwrap(), b"b");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn copy_recurses_into_directories() {
    let src = scratch("copy-src");
    let dst = scratch("copy-dst");
    std::fs::create_dir_all(src.join("nested/deep")).unwrap();
    std::fs::write(src.join("nested/deep/file.txt"), b"payload").unwrap();

    let copied = ops::copy_into(&src.join("nested"), &dst).unwrap();
    assert_eq!(
        std::fs::read(copied.join("deep/file.txt")).unwrap(),
        b"payload"
    );
    // The source is untouched by a copy.
    assert!(src.join("nested/deep/file.txt").exists());

    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn copy_refuses_to_overwrite() {
    let src = scratch("copy-clash-src");
    let dst = scratch("copy-clash-dst");
    std::fs::write(src.join("same.txt"), b"new").unwrap();
    std::fs::write(dst.join("same.txt"), b"old").unwrap();

    let err = ops::copy_into(&src.join("same.txt"), &dst).unwrap_err();
    assert_eq!(err.code, "exists");
    assert_eq!(std::fs::read(dst.join("same.txt")).unwrap(), b"old");

    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn move_removes_the_source() {
    let src = scratch("move-src");
    let dst = scratch("move-dst");
    std::fs::write(src.join("thing.txt"), b"x").unwrap();

    let moved = ops::move_into(&src.join("thing.txt"), &dst).unwrap();
    assert!(moved.exists());
    assert!(!src.join("thing.txt").exists());

    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn new_folder_numbers_around_a_collision() {
    let dir = scratch("newfolder");
    let first = ops::create_folder(&dir, "Thư mục mới").unwrap();
    let second = ops::create_folder(&dir, "Thư mục mới").unwrap();

    assert_ne!(first, second);
    assert_eq!(second.file_name().unwrap(), "Thư mục mới (2)");
    assert!(first.is_dir() && second.is_dir());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_file_is_created_empty_and_unique() {
    let dir = scratch("newfile");
    let first = ops::create_file(&dir, "Tệp mới").unwrap();
    let second = ops::create_file(&dir, "Tệp mới").unwrap();

    assert_eq!(first.extension().unwrap(), "txt");
    assert_eq!(std::fs::metadata(&first).unwrap().len(), 0);
    assert_ne!(first, second);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleting_to_the_recycle_bin_removes_the_entry() {
    let dir = scratch("recycle");
    let file = dir.join("trash-me.txt");
    std::fs::write(&file, b"x").unwrap();

    let count = ops::delete_to_recycle_bin(std::slice::from_ref(&file)).expect("shell delete");
    assert_eq!(count, 1);
    assert!(!file.exists(), "the entry must leave its original location");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleting_nothing_is_a_no_op() {
    assert_eq!(ops::delete_to_recycle_bin(&[]).unwrap(), 0);
}

#[test]
fn a_dotfile_is_reported_hidden() {
    let dir = scratch("hidden-flag");
    std::fs::write(dir.join(".secret"), b"x").unwrap();
    let listing = files_fs::read_dir(
        &dir,
        files_core::sort::SortSpec::default(),
        files_core::sort::ListOptions { show_hidden: true, show_dotfiles: true },
    )
    .unwrap();
    let entry = listing.entries.iter().find(|e| e.name == ".secret").unwrap();
    assert!(entry.is_hidden, "dotfiles stay hidden on every platform");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn directory_size_sums_the_whole_tree() {
    let dir = scratch("dirsize");
    std::fs::create_dir_all(dir.join("a/b")).unwrap();
    std::fs::write(dir.join("top.txt"), vec![0u8; 100]).unwrap();
    std::fs::write(dir.join("a/mid.txt"), vec![0u8; 200]).unwrap();
    std::fs::write(dir.join("a/b/deep.txt"), vec![0u8; 300]).unwrap();

    assert_eq!(files_fs::directory_size(&dir), 600);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn directory_size_of_an_empty_folder_is_zero() {
    let dir = scratch("dirsize-empty");
    assert_eq!(files_fs::directory_size(&dir), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn directory_size_of_a_missing_folder_is_zero_not_a_panic() {
    let dir = scratch("dirsize-missing");
    let missing = dir.join("nope");
    assert_eq!(files_fs::directory_size(&missing), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn keep_both_renames_instead_of_overwriting() {
    // "Giữ cả hai": mục cũ phải còn nguyên, mục mới mang tên khác.
    let root = scratch("keep-both");
    let src_dir = root.join("nguon");
    let dst_dir = root.join("dich");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&dst_dir).unwrap();
    let src = src_dir.join("bao-cao.txt");
    std::fs::write(&src, b"noi dung moi").unwrap();
    // Đã có sẵn tệp cùng tên ở đích.
    std::fs::write(dst_dir.join("bao-cao.txt"), b"noi dung cu").unwrap();

    let dest = ops::copy_into_with(&src, &dst_dir, ops::ConflictPolicy::KeepBoth)
        .expect("phải chép được");

    assert_ne!(dest, dst_dir.join("bao-cao.txt"), "không được ghi đè");
    assert_eq!(
        std::fs::read(dst_dir.join("bao-cao.txt")).unwrap(),
        b"noi dung cu",
        "tệp cũ phải nguyên vẹn"
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"noi dung moi");
    assert!(dest.exists(), "bản mới phải tồn tại");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn replace_overwrites_the_old_file() {
    let root = scratch("replace");
    let src_dir = root.join("nguon");
    let dst_dir = root.join("dich");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&dst_dir).unwrap();
    let src = src_dir.join("t.txt");
    std::fs::write(&src, b"moi").unwrap();
    std::fs::write(dst_dir.join("t.txt"), b"cu").unwrap();

    let dest = ops::copy_into_with(&src, &dst_dir, ops::ConflictPolicy::Replace)
        .expect("phải chép được");

    assert_eq!(dest, dst_dir.join("t.txt"), "ghi đè đúng chỗ");
    assert_eq!(std::fs::read(&dest).unwrap(), b"moi");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_default_policy_still_refuses_to_overwrite() {
    // Hành vi cũ phải giữ nguyên: không bao giờ âm thầm mất dữ liệu.
    let root = scratch("fail-policy");
    let src_dir = root.join("nguon");
    let dst_dir = root.join("dich");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&dst_dir).unwrap();
    let src = src_dir.join("t.txt");
    std::fs::write(&src, b"moi").unwrap();
    std::fs::write(dst_dir.join("t.txt"), b"cu").unwrap();

    let err = ops::copy_into(&src, &dst_dir).expect_err("phải từ chối");
    assert_eq!(err.code, "exists");
    assert_eq!(
        std::fs::read(dst_dir.join("t.txt")).unwrap(),
        b"cu",
        "tệp cũ không được đổi"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_shortcut_reports_its_target() {
    // Đọc đích bằng IShellLinkW trên một shortcut thật do Windows tạo.
    let root = scratch("lnk-target");
    let target = root.join("dich.txt");
    std::fs::write(&target, b"x").unwrap();
    let link = root.join("shortcut.lnk");

    // Tạo shortcut bằng PowerShell để không phải tự dựng định dạng .lnk.
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath='{}'; $s.Save()",
        link.display(),
        target.display()
    );
    let ok = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok || !link.exists() {
        // Máy không cho tạo shortcut: bỏ qua thay vì báo hỏng.
        let _ = std::fs::remove_dir_all(&root);
        return;
    }

    let got = files_fs::shortcut_target(&link).expect("phải đọc được đích");
    assert_eq!(
        got.to_string_lossy().to_lowercase(),
        target.to_string_lossy().to_lowercase()
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_file_that_is_not_a_shortcut_returns_none() {
    // Gọi nhầm trên tệp thường không được panic.
    let root = scratch("lnk-not");
    let f = root.join("thuong.txt");
    std::fs::write(&f, b"x").unwrap();
    assert!(files_fs::shortcut_target(&f).is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rotating_a_png_swaps_its_dimensions() {
    // Xoay 90° phải đổi rộng↔cao. Đây là bằng chứng ảnh thật sự bị xoay,
    // không chỉ là ghi lại tệp.
    let root = scratch("rotate-png");
    let src = root.join("anh.png");
    // Tạo PNG 4×2 bằng PowerShell + System.Drawing.
    let script = format!(
        "Add-Type -AssemblyName System.Drawing; \
         $b = New-Object System.Drawing.Bitmap 4,2; \
         $g = [System.Drawing.Graphics]::FromImage($b); \
         $g.Clear([System.Drawing.Color]::Red); \
         $b.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png); \
         $g.Dispose(); $b.Dispose()",
        src.display()
    );
    let ok = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok || !src.exists() {
        let _ = std::fs::remove_dir_all(&root);
        return;
    }

    let before = read_png_size(&src);
    files_fs::rotate_image_clockwise(&src).expect("phải xoay được");
    let after = read_png_size(&src);

    assert_eq!(before.0, after.1, "rộng trước phải thành cao sau");
    assert_eq!(before.1, after.0, "cao trước phải thành rộng sau");
    let _ = std::fs::remove_dir_all(&root);
}

/// Đọc rộng×cao từ header PNG (IHDR) mà không cần thư viện ảnh.
fn read_png_size(path: &std::path::Path) -> (u32, u32) {
    let b = std::fs::read(path).expect("đọc được");
    assert_eq!(&b[12..16], b"IHDR", "không phải PNG hợp lệ");
    let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
    let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
    (w, h)
}

#[test]
fn rotating_an_unsupported_extension_is_refused() {
    // Không được ghi ra tệp hỏng cho định dạng không hỗ trợ.
    let root = scratch("rotate-bad");
    let f = root.join("tai-lieu.txt");
    std::fs::write(&f, b"khong phai anh").unwrap();
    let err = files_fs::rotate_image_clockwise(&f).expect_err("phải từ chối");
    assert_eq!(err.code, "unsupported");
    assert_eq!(std::fs::read(&f).unwrap(), b"khong phai anh", "tệp gốc không đổi");
    let _ = std::fs::remove_dir_all(&root);
}
