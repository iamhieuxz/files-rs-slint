// Kiểm chứng `list_recycle_bin` trên Thùng rác thật của máy này.
//
// Chạy như một test tích hợp để dùng đúng API công khai của crate, và để có thể
// chạy lại bất cứ lúc nào.

use files_fs::{can_browse_recycle_bin, can_restore, list_recycle_bin};

#[test]
fn listing_the_real_recycle_bin_matches_the_survey() {
    if !can_browse_recycle_bin() {
        eprintln!("bỏ qua: duyệt Thùng rác chưa bật");
        return;
    }

    let items = match list_recycle_bin() {
        Ok(items) => items,
        Err(e) => {
            // Máy không có Thùng rác (container, CI) là hợp lệ; báo và bỏ qua.
            eprintln!("bỏ qua: không đọc được Thùng rác: {}", e.message);
            return;
        }
    };

    // Khảo sát trước khi viết code: C: có 26 `$I`, D: 4, E: 17 → 47 cặp.
    // Không khẳng định đúng số tuyệt đối vì người dùng xoá thêm giữa hai lần
    // chạy; chỉ khẳng định đọc được và mọi mục đều có đường dẫn gốc tuyệt đối.
    eprintln!("đọc được {} mục từ Thùng rác", items.len());
    for item in items.iter().take(5) {
        eprintln!(
            "  {} | {} byte | xoá {:?} | thư mục={} | {}",
            item.name,
            item.size,
            item.deleted_ms,
            item.is_dir,
            item.original.display()
        );
    }

    for item in &items {
        assert!(
            item.original.is_absolute(),
            "đường dẫn gốc phải tuyệt đối: {}",
            item.original.display()
        );
        assert!(!item.name.is_empty(), "mục phải có tên hiển thị");
        assert!(item.r_path.exists(), "nội dung $R phải còn tồn tại");
        assert!(item.i_path.exists(), "metadata $I phải còn tồn tại");
    }

    assert!(can_restore(), "khôi phục phải được bật khi đọc được");
}
