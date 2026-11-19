// Kiểm chứng khôi phục trên Thùng rác thật: xoá một tệp tạm vào Thùng rác,
// đọc lại qua API mới, khôi phục, rồi so sánh nội dung.
//
// # Vì sao các test này bị `#[ignore]`
//
// Chúng cần **ghi được vào** `C:\$Recycle.Bin\<SID>`. Trong môi trường bị sandbox
// (nhóm `CodexSandboxUsers` chỉ có `ReadAndExecute`), `SHFileOperationW` trả
// `rc = 0` nhưng **xoá vĩnh viễn** tệp mà không tạo `$I`. Đó là hành vi của
// môi trường, không phải của app, nên test không thể chạy trên CI.
//
// Chạy tay trên máy không sandbox:
//
// ```powershell
// cargo test -p files-fs --test recycle_restore -- --ignored --nocapture
// ```

use std::path::PathBuf;

/// Đưa một mục vào Thùng rác thật bằng đúng đường app dùng khi người dùng xoá.
///
/// Dùng `delete_to_recycle_bin` của crate thay vì tự gọi `SHFileOperationW`:
/// đường tự viết trong test từng âm thầm xoá **vĩnh viễn** (không tạo `$I`), và
/// đó chính là loại lỗi mà test này phải phát hiện chứ không phải lặp lại.
fn send_to_recycle_bin(path: &std::path::Path) -> Result<(), String> {
    files_fs::delete_to_recycle_bin(&[path.to_path_buf()])
        .map(|_| ())
        .map_err(|e| e.message)
}

fn scratch(name: &str) -> PathBuf {
    // `std::env::temp_dir()` đọc `TEMP`/`TMP`, mà trên máy này chúng là dạng
    // **8.3 ngắn** (`C:\Users\IAMHIE~1\...`). Shell ghi đường dẫn **dài** vào
    // `$I`, nên phải chuẩn hoá ngay từ đầu để so sánh về sau là cùng một chỗ.
    //
    // `canonicalize` trả tiền tố `\\?\` mà `SHFileOperationW` **không** nhận
    // (nó trả 124 = DE_INVALIDFILES), nên phải bỏ tiền tố đó đi.
    let base = std::env::temp_dir();
    let long_base = base.canonicalize().unwrap_or(base);
    let long_base = match long_base.to_string_lossy().strip_prefix(r"\\?\") {
        Some(stripped) => PathBuf::from(stripped),
        None => long_base,
    };
    let dir = long_base.join(format!("files-rs-rc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tạo thư mục tạm");
    dir
}

/// Hai đường dẫn có trỏ tới cùng một chỗ không.
///
/// Phải so theo dạng chuẩn vì `$I` lưu đường dẫn **dài**
/// (`C:\Users\iamhieuxz\AppData\Local\Temp\...`) còn `TEMP` trên máy này là dạng
/// **8.3 ngắn** (`C:\Users\IAMHIE~1\AppData\Local\Temp\...`). So chuỗi thô sẽ
/// trượt dù hai đường dẫn là một.
///
/// Canonicalize thư mục cha chứ không cả đường dẫn: tệp đã bị xoá nên
/// `canonicalize` trên chính nó sẽ lỗi.
fn same_location(a: &std::path::Path, b: &std::path::Path) -> bool {
    let (Some(pa), Some(pb)) = (a.parent(), b.parent()) else {
        return a == b;
    };
    let (Some(na), Some(nb)) = (a.file_name(), b.file_name()) else {
        return a == b;
    };
    let ca = pa.canonicalize().unwrap_or_else(|_| pa.to_path_buf());
    let cb = pb.canonicalize().unwrap_or_else(|_| pb.to_path_buf());
    ca == cb && na == nb
}

/// Tìm một mục trong Thùng rác, chờ tối đa vài giây.
///
/// Shell ghi `$I` không đồng bộ với lời gọi `SHFileOperationW`: hàm trả về ngay
/// cả khi tệp `$I` chưa kịp xuất hiện. Đọc một lần rồi kết luận "không có" là
/// sai, và đây chính là lỗi làm test trượt trong khi tính năng vẫn đúng.
fn find_in_bin(original: &std::path::Path) -> Option<files_fs::RecycleItem> {
    for _ in 0..20 {
        if let Ok(items) = files_fs::list_recycle_bin() {
            if let Some(found) = items
                .into_iter()
                .find(|i| same_location(&i.original, original))
            {
                return Some(found);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

#[test]
#[ignore = "cần quyền ghi vào $Recycle.Bin, bị sandbox chặn — chạy tay với --ignored"]
fn a_file_sent_to_the_bin_can_be_listed_and_restored() {
    if !files_fs::can_browse_recycle_bin() || !files_fs::can_restore() {
        eprintln!("bỏ qua: Thùng rác chưa được bật");
        return;
    }

    let dir = scratch("restore");
    let file = dir.join("khoi-phuc-toi.txt");
    let content = b"noi dung can khoi phuc";
    std::fs::write(&file, content).expect("ghi tệp mẫu");

    if let Err(e) = send_to_recycle_bin(&file) {
        panic!("không đưa được tệp vào Thùng rác: {e}");
    }
    assert!(!file.exists(), "tệp phải rời khỏi chỗ cũ");

    // Shell xử lý bất đồng bộ phần ghi `$I`; đọc ngay có thể chưa thấy.
    // `find_in_bin` trả về giá trị sở hữu nên `target` là `RecycleItem`.
    let target = find_in_bin(&file);
    let Some(target) = target else {
        panic!("không thấy {} trong Thùng rác sau khi chờ", file.display());
    };

    assert_eq!(target.size, content.len() as u64, "kích thước phải khớp");
    assert!(target.deleted_ms.is_some(), "phải có thời điểm xoá");
    assert!(!target.is_dir, "đây là tệp, không phải thư mục");

    // Khôi phục và kiểm tra nội dung quay lại nguyên vẹn.
    files_fs::restore_recycle_item(&target).expect("phải khôi phục được");
    assert!(file.exists(), "tệp phải quay lại đúng chỗ");
    assert_eq!(
        std::fs::read(&file).expect("đọc lại"),
        content,
        "nội dung phải giữ nguyên"
    );

    // Đã khôi phục thì không còn trong Thùng rác nữa.
    let after = files_fs::list_recycle_bin().expect("đọc lại Thùng rác");
    assert!(
        !after.iter().any(|i| same_location(&i.original, &file)),
        "mục đã khôi phục không được còn trong danh sách"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "cần quyền ghi vào $Recycle.Bin, bị sandbox chặn — chạy tay với --ignored"]
fn a_folder_sent_to_the_bin_comes_back_with_its_contents() {
    if !files_fs::can_browse_recycle_bin() || !files_fs::can_restore() {
        eprintln!("bỏ qua: Thùng rác chưa được bật");
        return;
    }

    let dir = scratch("folder");
    let folder = dir.join("thu-muc-can-khoi-phuc");
    std::fs::create_dir_all(&folder).expect("tạo thư mục");
    std::fs::write(folder.join("ben-trong.txt"), b"con cua thu muc").expect("ghi tệp con");
    std::fs::create_dir_all(folder.join("thu-muc-con")).expect("tạo thư mục con");
    std::fs::write(folder.join("thu-muc-con").join("sau.txt"), b"tang sau").expect("ghi tệp");

    if let Err(e) = send_to_recycle_bin(&folder) {
        panic!("không đưa được thư mục vào Thùng rác: {e}");
    }
    assert!(!folder.exists(), "thư mục phải rời khỏi chỗ cũ");

    let items = files_fs::list_recycle_bin().expect("đọc được Thùng rác");
    let target = items
        .iter()
        .find(|i| same_location(&i.original, &folder))
        .unwrap_or_else(|| panic!("không thấy {} trong Thùng rác", folder.display()));
    assert!(target.is_dir, "phải nhận ra đây là thư mục");

    files_fs::restore_recycle_item(target).expect("phải khôi phục được");

    assert!(folder.is_dir(), "thư mục phải quay lại");
    assert_eq!(
        std::fs::read(folder.join("ben-trong.txt")).expect("đọc"),
        b"con cua thu muc"
    );
    assert_eq!(
        std::fs::read(folder.join("thu-muc-con").join("sau.txt")).expect("đọc"),
        b"tang sau"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
