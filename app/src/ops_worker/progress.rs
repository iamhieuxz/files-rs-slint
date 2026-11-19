//! Tiến trình thao tác: chia sẻ giữa UI và luồng worker. Tách từ `ops_worker.rs`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Tiến trình của thao tác đang chạy, chia sẻ giữa UI và luồng worker.
///
/// UI đọc bằng cách hỏi theo nhịp (`snapshot`) chứ không nhận thông điệp cho
/// từng tệp: một thư mục 10.000 tệp sẽ làm ngập kênh kết quả, và UI chỉ cần
/// vẽ lại vài chục lần mỗi giây là đủ.
///
/// `total == 0` nghĩa là không có thao tác nào đang chạy.
pub struct Progress {
    total: AtomicUsize,
    done: AtomicUsize,
    cancel: AtomicBool,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            total: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            cancel: AtomicBool::new(false),
        }
    }
}

impl Progress {
    /// Bắt đầu một thao tác gồm `total` mục.
    pub fn begin(&self, total: usize) {
        self.done.store(0, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
        // Đặt `total` SAU cùng: UI thấy total khác 0 là biết thao tác đã bắt đầu,
        // và lúc đó `done`/`cancel` đã ở trạng thái đúng.
        self.total.store(total, Ordering::Release);
    }

    /// Bắt đầu một thao tác chưa biết tổng (ví dụ tìm kiếm).
    pub fn begin_indeterminate(&self) {
        // Total khác 0 để UI biết đang chạy và còn huỷ được; `done < total`
        // giữ nút Huỷ hiện, còn thanh tiến trình không giả vờ có tỉ lệ.
        self.done.store(0, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
        self.total.store(usize::MAX, Ordering::Release);
    }

    /// Đánh dấu xong một mục.
    pub fn tick(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    /// Kết thúc thao tác (thành công, lỗi, hoặc bị huỷ).
    pub fn finish(&self) {
        self.total.store(0, Ordering::Release);
        self.done.store(0, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
    }

    /// Người dùng bấm Huỷ. Thao tác dừng ở ranh giới tệp kế tiếp.
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Đã được yêu cầu huỷ chưa. Worker hỏi trước mỗi tệp.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// `Some((đã xong, tổng))` khi có thao tác đang chạy.
    pub fn snapshot(&self) -> Option<(usize, usize)> {
        let total = self.total.load(Ordering::Acquire);
        if total == 0 {
            return None;
        }
        Some((self.done.load(Ordering::Relaxed), total))
    }
}
