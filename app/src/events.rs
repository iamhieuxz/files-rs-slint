//! Background result pumps.
//!
//! The worker thread never touches the UI: it sends results down a channel and a
//! timer drains them on the UI thread. The directory watcher gets its own pump for
//! the same reason. Keeping both here makes the thread boundary visible in one
//! place — nothing outside this module should read from those channels.

use crate::slint_generated::*;
use crate::state::App;
use crate::view::{apply_shell_icon, fill_size_cell, push_status, push_view, to_slint_image};
use crate::{logging, navigation};
use navigation::{apply_listing, reload};
use crate::ops_worker::JobDone;
use slint::{ComponentHandle, SharedString, Timer, TimerMode};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use crate::format;

/// Drain finished background work on a short timer.
///
/// `invoke_from_event_loop` would be simpler but cannot be called from a thread
/// that outlives the component; a timer keeps the handoff inside the event loop
/// and makes shutdown deterministic.
/// Xoá thông báo thành công sau vài giây.
///
/// Thông báo cũ nằm lại vô hạn sẽ khiến thanh trạng thái luôn nói về quá khứ;
/// người dùng không còn tin nó phản ánh việc vừa xảy ra.
pub fn install_status_clear(ui: &AppWindow) {
    use std::time::Duration;
    let w = ui.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(500), move || {
        let Some(ui) = w.upgrade() else { return };
        let text = ui.get_status_tertiary();
        if text.is_empty() {
            return;
        }
        // Đếm số nhịp đã giữ nguyên nội dung: nội dung đổi thì đếm lại.
        thread_local! {
            static LAST: std::cell::RefCell<(String, u32)> =
                const { std::cell::RefCell::new((String::new(), 0)) };
        }
        let reached = LAST.with(|cell| {
            let mut state = cell.borrow_mut();
            if state.0 == text.as_str() {
                state.1 += 1;
            } else {
                state.0 = text.to_string();
                state.1 = 0;
            }
            // 500 ms × 8 = 4 giây.
            state.1 >= 8
        });
        if reached {
            ui.set_status_tertiary(SharedString::default());
            LAST.with(|cell| {
                let mut state = cell.borrow_mut();
                state.0.clear();
                state.1 = 0;
            });
        }
    });
    std::mem::forget(timer);
}

/// Đẩy tiến trình của thao tác đang chạy lên UI theo nhịp.
///
/// Hỏi trạng thái thay vì nhận thông điệp cho từng tệp: kênh kết quả sẽ bị ngập
/// với một thư mục lớn, còn UI chỉ cần vẽ lại vài chục lần mỗi giây.
pub fn install_progress_pump(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    use std::time::Duration;
    let (w, a) = (ui.as_weak(), app.clone());
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(120), move || {
        let Some(ui) = w.upgrade() else { return };
        let snapshot = {
            let a = a.borrow();
            a.worker.progress().snapshot()
        };
        match snapshot {
            Some((done, total)) if total > 0 => {
                ui.set_progress(done as f32 / total as f32);
                // Chỉ cho huỷ khi còn việc: huỷ một thao tác đã xong là vô nghĩa.
                ui.set_cancellable(done < total);
            }
            _ => {
                if ui.get_progress() != 0.0 {
                    ui.set_progress(0.0);
                }
                if ui.get_cancellable() {
                    ui.set_cancellable(false);
                }
            }
        }
    });
    std::mem::forget(timer);
}

pub fn install_result_pump(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let (w, a) = (ui.as_weak(), app.clone());
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        let Some(ui) = w.upgrade() else { return };
        loop {
            let done = {
                let s = a.borrow();
                s.results.try_recv()
            };
            match done {
                Ok(JobDone::Loaded { token, path, result }) => {
                    // A reply for a directory the user already left must not
                    // overwrite the current view.
                    if token != a.borrow().load_token {
                        continue;
                    }
                    // This reply settles the in-flight read, whatever it says.
                    a.borrow_mut().load_in_flight = false;
                    match result {
                        Ok(listing) => {
                            apply_listing(&ui, &a, listing);
                        }
                        Err(e) => {
                            ui.set_error_text(SharedString::from(e.message.as_str()));
                            ui.set_busy(false);
                            // The history must not keep a path the user never saw.
                            let back = a.borrow().tab().current().parent().map(Path::to_path_buf);
                            if let Some(p) = back {
                                if p != path {
                                    let mut s = a.borrow_mut();
                                    s.tab_mut().history.replace_current(p);
                                }
                            }
                        }
                    }
                }
                Ok(JobDone::OpDone { label, result, undo }) => {
                    ui.set_busy(false);
                    ui.set_busy_text(SharedString::default());
                    match result {
                        Ok(()) => {
                            ui.set_status_tertiary(SharedString::from(label.as_str()));
                            // Chỉ ghi lịch sử khi thao tác thật sự thành công.
                            if let Some(action) = undo {
                                a.borrow_mut().undo.record(action);
                            }
                            // Sau khi di chuyển, nguồn đã biến mất nên clipboard
                            // phải rỗng; dán lại sẽ báo lỗi không tìm thấy.
                            if a.borrow().clipboard_cut {
                                a.borrow_mut().clipboard.clear();
                                ui.set_clipboard_count(0);
                                a.borrow_mut().clipboard_cut = false;
                                ui.set_clipboard_mode(SharedString::default());
                            }
                            reload(&ui, &a);
                        }
                        Err(e) => {
                            logging::write(&format!("thao tác lỗi [{}]: {}", e.code, e.message));
                            // Câu giải thích thay cho "(os error 5)" trần trụi.
                            let text = format::explain_error(&e.code, &e.message);
                            ui.set_error_text(SharedString::from(text.as_str()));
                        }
                    }
                }
                Ok(JobDone::FolderSize { path, bytes, token }) => {
                    // Token 0 là lệnh thủ công từ menu; token khác phải khớp
                    // lần đọc thư mục hiện tại, nếu không kết quả thuộc về
                    // một thư mục người dùng đã rời đi.
                    if token != 0 && token != a.borrow().load_token {
                        continue;
                    }
                    let updated = {
                        let mut s = a.borrow_mut();
                        let active = s.active;
                        let path_str = path.to_string_lossy().into_owned();
                        match s.tabs[active]
                            .entries
                            .iter_mut()
                            .find(|e| e.path == path_str)
                        {
                            Some(e) => {
                                e.size = Some(bytes);
                                true
                            }
                            None => false,
                        }
                    };
                    if updated {
                        // Điền đúng ô đó, không dựng lại cả bảng.
                        fill_size_cell(&ui, &a, &path, bytes);
                    }
                    if token != 0 {
                        ui.set_status_tertiary(SharedString::from(
                            format!("Kích thước thư mục: {}", format::format_size(Some(bytes))).as_str(),
                        ));
                    }
                }
                Ok(JobDone::SearchDone { token, root, hits, cancelled }) => {
                    // Bỏ kết quả của lần tìm trước khi người dùng đã đổi thư mục.
                    if token != a.borrow().load_token {
                        continue;
                    }
                    let count = hits.len();
                    {
                        let mut s = a.borrow_mut();
                        let active = s.active;
                        let tab = &mut s.tabs[active];
                        // Hiện kết quả như một danh sách tạm: không thay `entries`
                        // của thư mục, để thoát tìm kiếm là quay lại đúng chỗ cũ.
                        tab.entries = hits.iter().map(|h| h.entry.clone()).collect();
                        tab.selection.clear();
                        tab.anchor = None;
                        tab.filter.clear();
                        tab.rebuild_visible();
                    }
                    push_view(&ui, &a);
                    push_status(&ui, &a);
                    let text = if cancelled {
                        format!("Đã huỷ tìm kiếm sau {count} kết quả")
                    } else {
                        format!("Tìm thấy {count} kết quả trong {}", root.display())
                    };
                    ui.set_status_tertiary(SharedString::from(text.as_str()));
                }
                Ok(JobDone::ShellIcon { path, icon, token }) => {
                    // Bỏ qua nếu người dùng đã rời thư mục.
                    if token != a.borrow().load_token {
                        continue;
                    }
                    let image = to_slint_image(&icon);
                    apply_shell_icon(&ui, &a, &path, image);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
        }
    });
    // The timer must stay alive for the lifetime of the window.
    std::mem::forget(timer);
}

/// Poll the directory watcher and refresh when the folder changes.
pub fn install_watch_pump(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let (w, a) = (ui.as_weak(), app.clone());
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(400), move || {
        let Some(ui) = w.upgrade() else { return };
        let fired = {
            let s = a.borrow();
            match &s.watch {
                Some(h) => h.try_recv().is_some(),
                None => false,
            }
        };
        // Một lần đọc đang chạy thì bỏ qua nhịp này: nếu không, mỗi sự kiện
        // của `notify` lại xếp thêm một lần đọc và UI quay vòng vô ích.
        if fired && !a.borrow().load_in_flight {
            reload(&ui, &a);
        }
    });
    std::mem::forget(timer);
}
