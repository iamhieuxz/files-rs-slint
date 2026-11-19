//! Trạng thái thao tác dài để UI hiện thẻ tiến trình nổi.
//!
//! Thẻ chỉ hiện sau một khoảng ngắn: thao tác tức thời không được làm phiền
//! người dùng bằng một popup nhấp nháy. Thời gian bắt đầu nằm trong `App` để
//! bộ đếm của UI và lúc kết thúc dùng chung một nguồn.

use crate::slint_generated::*;
use crate::state::App;
use slint::SharedString;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Thẻ chỉ xuất hiện khi thao tác đã chạy lâu hơn ngưỡng này.
pub const SHOW_AFTER: Duration = Duration::from_millis(300);

pub fn begin(ui: &AppWindow, app: &Rc<RefCell<App>>, label: &str) {
    app.borrow_mut().operation_started = Some(Instant::now());
    ui.set_operation_running(true);
    ui.set_operation_visible(false);
    ui.set_operation_tone(0);
    ui.set_operation_seconds(0);
    ui.set_operation_done(0);
    ui.set_operation_total(0);
    ui.set_operation_phase(0.0);
    ui.set_operation_title(SharedString::from(label));
    ui.set_busy(true);
    ui.set_busy_text(SharedString::from(label));
    ui.set_progress(0.0);
    ui.set_cancellable(false);
}

/// Đổi nhãn khi người dùng bấm Huỷ, trước khi worker dừng ở tệp kế tiếp.
pub fn relabel(ui: &AppWindow, label: &str) {
    ui.set_operation_title(SharedString::from(label));
    ui.set_busy_text(SharedString::from(label));
}

pub fn finish(ui: &AppWindow, app: &Rc<RefCell<App>>, label: &str, ok: bool) {
    let was_running = ui.get_operation_running();
    let started = app.borrow_mut().operation_started.take();
    ui.set_busy(false);
    ui.set_busy_text(SharedString::default());
    ui.set_operation_running(false);
    ui.set_cancellable(false);
    if !was_running {
        // Không có thao tác dài: giữ nguyên toast đang hiện (nếu có).
        return;
    }
    let elapsed = started.map(|s| s.elapsed()).unwrap_or_default();
    if elapsed >= SHOW_AFTER {
        ui.set_operation_visible(true);
        ui.set_operation_tone(if ok { 1 } else { 2 });
        ui.set_operation_title(SharedString::from(label));
        ui.set_operation_seconds(elapsed.as_secs() as i32);
    } else {
        // Thao tác xong trước khi kịp hiện thẻ: chỉ báo ở thanh trạng thái.
        ui.set_operation_visible(false);
        ui.set_operation_tone(0);
        ui.set_operation_title(SharedString::default());
    }
}

pub fn dismiss(ui: &AppWindow) {
    ui.set_operation_visible(false);
    ui.set_operation_tone(0);
    ui.set_operation_title(SharedString::default());
}

pub fn phase_tick(ui: &AppWindow) {
    let next = ui.get_operation_phase() + 0.07;
    ui.set_operation_phase(if next >= 1.0 { next - 1.0 } else { next });
}
