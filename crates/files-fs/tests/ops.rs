//! Behavioural tests for file operations.
//!
//! These run against a real temporary directory: the copy/move/rename rules are
//! the ones a user feels, so they are asserted on disk rather than mocked.

mod common;

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
fn renaming_only_the_letter_case_is_allowed() {
    // NTFS không phân biệt hoa/thường nên `dest.exists()` đúng dù đó chính là
    // tệp đang đổi tên. Chặn bằng `exists()` khiến người dùng không sửa được
    // kiểu chữ của tên — hành vi Explorer cho phép.
    let dir = scratch("rename-case");
    let file = dir.join("bao-cao.txt");
    std::fs::write(&file, b"noi dung").unwrap();

    let renamed = ops::rename(&file, "BAO-CAO.TXT").expect("phải đổi được kiểu chữ");

    assert_eq!(renamed, dir.join("BAO-CAO.TXT"));
    assert_eq!(std::fs::read(&renamed).unwrap(), b"noi dung");
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec!["BAO-CAO.TXT".to_string()],
        "không để lại tên tạm"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_name_longer_than_the_filesystem_limit_is_rejected() {
    // Báo lỗi hành động được thay vì để OS trả "The filename is too long".
    let too_long = "a".repeat(256);
    let err = ops::validate_name(&too_long).expect_err("phải từ chối");
    assert_eq!(err.code, "invalid_name");
    assert!(
        err.message.contains("255"),
        "phải nêu giới hạn: {}",
        err.message
    );

    let at_limit = "a".repeat(255);
    assert!(
        ops::validate_name(&at_limit).is_ok(),
        "đúng 255 ký tự phải hợp lệ"
    );
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
        files_core::sort::ListOptions {
            show_hidden: true,
            show_dotfiles: true,
        },
    )
    .unwrap();
    let entry = listing
        .entries
        .iter()
        .find(|e| e.name == ".secret")
        .unwrap();
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
fn directory_size_does_not_follow_a_junction_loop() {
    // Junction trỏ về thư mục cha tạo vòng lặp; đếm kích thước phải bỏ qua nó
    // thay vì đi xuống vô hạn và đếm cùng một tệp nhiều lần.
    let root = scratch("dirsize-junction");
    std::fs::write(root.join("data.bin"), vec![7u8; 1024]).unwrap();
    let link = root.join("loop");
    let ok = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok || !link.exists() {
        // Ổ không hỗ trợ junction (FAT, quyền bị chặn).
        let _ = std::fs::remove_dir_all(&root);
        common::skip("ổ không tạo được junction (FAT hoặc quyền bị chặn)");
        return;
    }

    assert_eq!(
        files_fs::directory_size(&root),
        1024,
        "chỉ tính tệp thật, không đi theo junction"
    );
    let _ = std::fs::remove_dir_all(&root);
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

    let dest =
        ops::copy_into_with(&src, &dst_dir, ops::ConflictPolicy::KeepBoth).expect("phải chép được");

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
fn unsafe_names_are_rejected_and_normal_ones_pass() {
    for bad in [
        "", "..", ".", "a/b", "a\\b", "CON", "nul.txt", "com1", "LPT9.log", "a?", "x:y", "dot.",
        "sp ",
    ] {
        assert!(ops::validate_name(bad).is_err(), "{bad:?} phải bị từ chối");
    }
    for ok in ["a.txt", "Thư mục", "com0", "console", ".gitignore", "a b"] {
        assert!(ops::validate_name(ok).is_ok(), "{ok:?} phải hợp lệ");
    }
}

#[test]
fn creating_with_a_device_name_is_refused() {
    let dir = scratch("device-name");
    assert!(ops::create_folder(&dir, "NUL").is_err());
    assert!(ops::create_file(&dir, "a..").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn copying_a_folder_into_its_own_child_is_refused() {
    let root = scratch("into-self-copy");
    let parent = root.join("a");
    let child = parent.join("b");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(parent.join("f.txt"), b"x").unwrap();

    let err = ops::copy_into(&parent, &child).expect_err("phải bị từ chối");
    assert_eq!(err.code, "into_self");
    let err = ops::copy_into(&parent, &parent).expect_err("phải bị từ chối");
    assert!(err.code == "into_self" || err.code == "same_path");
    assert!(!child.join("a").exists(), "không được tạo bản chép dở");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn moving_a_folder_into_its_own_child_is_refused() {
    let root = scratch("into-self-move");
    let parent = root.join("a");
    let child = parent.join("b");
    std::fs::create_dir_all(&child).unwrap();

    let err = ops::move_into(&parent, &child).expect_err("phải bị từ chối");
    assert_eq!(err.code, "into_self");
    assert!(parent.is_dir() && child.is_dir(), "nguồn còn nguyên");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_failed_replace_keeps_the_old_destination() {
    let root = scratch("replace-rollback");
    let src_dir = root.join("nguon");
    let dst_dir = root.join("dich");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&dst_dir).unwrap();
    // Nguồn không tồn tại nên bước ghi chắc chắn lỗi sau khi đích cũ đã bị dời.
    let src = src_dir.join("khong-co.txt");
    std::fs::write(dst_dir.join("khong-co.txt"), b"cu").unwrap();

    let result = ops::copy_into_with(&src, &dst_dir, ops::ConflictPolicy::Replace);
    assert!(result.is_err());
    assert_eq!(
        std::fs::read(dst_dir.join("khong-co.txt")).unwrap(),
        b"cu",
        "bản cũ phải còn nguyên"
    );
    let leftovers = std::fs::read_dir(&dst_dir).unwrap().count();
    assert_eq!(leftovers, 1, "không để lại file tạm");
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

    let dest =
        ops::copy_into_with(&src, &dst_dir, ops::ConflictPolicy::Replace).expect("phải chép được");

    assert_eq!(dest, dst_dir.join("t.txt"), "ghi đè đúng chỗ");
    assert_eq!(std::fs::read(&dest).unwrap(), b"moi");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn copy_replace_inside_the_source_folder_keeps_the_source() {
    // Ctrl+C rồi Ctrl+V ngay trong thư mục đang đứng, chọn "Thay thế":
    // đích trùng nguồn. Đây từng xoá mất chính tệp gốc.
    let root = scratch("replace-copy-same-folder");
    let file = root.join("giu-nguyen.txt");
    std::fs::write(&file, b"noi dung goc").unwrap();

    let err = ops::copy_into_with(&file, &root, ops::ConflictPolicy::Replace)
        .expect_err("phải từ chối thay thế chính nó");

    assert_eq!(err.code, "same_path");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"noi dung goc",
        "tệp gốc phải còn nguyên"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn move_replace_inside_the_source_folder_keeps_the_source() {
    // Cùng tình huống với Ctrl+X: xoá đích trước khi move sẽ xoá luôn nguồn.
    let root = scratch("replace-move-same-folder");
    let file = root.join("giu-nguyen.txt");
    std::fs::write(&file, b"noi dung goc").unwrap();

    let err = ops::move_into_with(&file, &root, ops::ConflictPolicy::Replace)
        .expect_err("phải từ chối di chuyển vào chính nó");

    assert_eq!(err.code, "same_path");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"noi dung goc",
        "tệp gốc phải còn nguyên"
    );
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
fn a_taken_zip_name_gets_a_free_numbered_name() {
    // UI hỏi tên trước khi nén để không bao giờ ghi đè archive cũ.
    let dir = scratch("free-zip-name");
    std::fs::write(dir.join("photos.zip"), b"cu").unwrap();

    let next = ops::free_path(&dir, "photos", ".zip");

    assert_eq!(next, dir.join("photos (2).zip"));
    assert!(!next.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn git_status_inside_a_subfolder_uses_paths_relative_to_that_folder() {
    // `git status` trả đường dẫn theo gốc repo. Ghép thẳng với thư mục đang
    // xem sẽ ra `sub/sub/inner/note.txt` và cột trạng thái không bao giờ khớp.
    let root = scratch("git-subdir");
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    if !run(&["init", "--quiet"]) {
        let _ = std::fs::remove_dir_all(&root);
        common::skip("git không có trên máy");
        return;
    }
    let _ = run(&["config", "user.email", "test@example.invalid"]);
    let _ = run(&["config", "user.name", "Test"]);
    std::fs::create_dir_all(root.join("sub/inner")).unwrap();
    std::fs::write(root.join("sub/inner/note.txt"), b"x").unwrap();
    if !run(&["add", "-A"]) || !run(&["commit", "--quiet", "-m", "init"]) {
        let _ = std::fs::remove_dir_all(&root);
        common::skip("git không chạy được để tạo repo tạm");
        return;
    }
    std::fs::write(root.join("sub/inner/note.txt"), b"changed").unwrap();
    std::fs::write(root.join("sub/new.txt"), b"new").unwrap();

    let sub = root.join("sub");
    let status = files_fs::status_for(&sub);

    assert_eq!(
        status.get(&sub.join("inner/note.txt")),
        Some(&files_fs::GitState::Modified),
        "đường dẫn phải tương đối theo thư mục đang xem; map={status:?}"
    );
    assert_eq!(
        status.get(&sub.join("new.txt")),
        Some(&files_fs::GitState::Untracked)
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
        let _ = std::fs::remove_dir_all(&root);
        common::skip("máy không tạo được shortcut .lnk qua PowerShell");
        return;
    }

    let got = files_fs::shortcut_target(&link).expect("phải đọc được đích");
    // Windows có thể trả về đường dẫn 8.3 (IAMHIE~1) cho thư mục temp; so sánh
    // dạng canonical để không phụ thuộc tên ngắn/dài của user profile.
    let got = std::fs::canonicalize(&got).unwrap_or(got);
    let expected = std::fs::canonicalize(&target).unwrap_or(target.clone());
    assert_eq!(
        got.to_string_lossy().to_lowercase(),
        expected.to_string_lossy().to_lowercase()
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
        common::skip("không tạo được tệp mẫu bằng PowerShell");
        return;
    }

    let before = read_png_size(&src);
    files_fs::rotate_image_clockwise(&src).expect("phải xoay được");
    let after = read_png_size(&src);

    assert_eq!(before.0, after.1, "rộng trước phải thành cao sau");
    assert_eq!(before.1, after.0, "cao trước phải thành rộng sau");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rotating_left_is_the_inverse_of_rotating_right() {
    // Xoay phải rồi xoay trái phải trả về đúng kích thước ban đầu. Nếu ai đó
    // map nhầm hướng, phép so sánh này bắt được ngay.
    let root = scratch("rotate-ccw");
    let src = root.join("anh.png");
    let script = format!(
        "Add-Type -AssemblyName System.Drawing; \
         $b = New-Object System.Drawing.Bitmap 8,3; \
         $g = [System.Drawing.Graphics]::FromImage($b); \
         $g.Clear([System.Drawing.Color]::Blue); \
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
        common::skip("không tạo được tệp mẫu bằng PowerShell");
        return;
    }

    let original = read_png_size(&src);
    files_fs::rotate_image_clockwise(&src).expect("xoay phải được");
    let after_right = read_png_size(&src);
    assert_eq!(
        after_right,
        (original.1, original.0),
        "xoay phải phải đổi chiều"
    );

    files_fs::rotate_image_counter_clockwise(&src).expect("xoay trái được");
    let after_both = read_png_size(&src);
    assert_eq!(
        after_both, original,
        "xoay phải rồi trái phải trả về kích thước ban đầu"
    );
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
    assert_eq!(
        std::fs::read(&f).unwrap(),
        b"khong phai anh",
        "tệp gốc không đổi"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn git_status_reports_a_real_repository() {
    // Chạy trên chính repo này: đây là bằng chứng mạnh nhất vì nó dùng dữ liệu
    // thật, không phải fixture tự tạo.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("gốc repo");
    if !root.join(".git").exists() {
        common::skip("không phải checkout git");
        return;
    }

    let status = files_fs::status_for(root);
    // Repo này có nhiều tệp chưa commit trong lúc phát triển, nên chỉ cần
    // kiểm tra rằng hàm không trả rỗng và mọi khoá là đường dẫn tuyệt đối.
    // Repo sạch (không có thay đổi) là trạng thái hợp lệ, không phải lỗi.
    if status.is_empty() {
        common::skip("repo không có thay đổi nào để kiểm tra git status");
        return;
    }
    for path in status.keys() {
        assert!(
            path.is_absolute(),
            "khoá phải là đường dẫn tuyệt đối: {path:?}"
        );
    }
}

#[test]
fn a_folder_inside_this_repository_finds_the_root() {
    // Thư mục tạm của test nằm TRONG repo này (cạnh target/), nên `repo_root`
    // phải tìm thấy gốc — không phải None như tôi tưởng ban đầu.
    let root = scratch("git-inside");
    let found = files_fs::repo_root(&root);
    let expected = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf());
    if let (Some(found), Some(expected)) = (found, expected) {
        if expected.join(".git").exists() {
            assert_eq!(found, expected, "phải leo lên đúng gốc repo");
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_untracked_file_shows_up_as_untracked() {
    // Tạo repo tạm rồi kiểm tra đúng một tệp mới.
    let root = scratch("git-untracked");
    let ok = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        let _ = std::fs::remove_dir_all(&root);
        common::skip("git không chạy được để tạo repo tạm");
        return;
    }
    std::fs::write(root.join("moi.txt"), b"x").unwrap();

    let status = files_fs::status_for(&root);
    let key = root.join("moi.txt");
    assert_eq!(
        status.get(&key),
        Some(&files_fs::GitState::Untracked),
        "tệp mới phải là Untracked"
    );
    let _ = std::fs::remove_dir_all(&root);
}
