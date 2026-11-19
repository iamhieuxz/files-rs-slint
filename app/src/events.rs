//! Background result pumps.
//!
//! The worker thread never touches the UI: it sends results down a channel and a
//! timer drains them on the UI thread. The directory watcher gets its own pump for
//! the same reason. Keeping both here makes the thread boundary visible in one
//! place — nothing outside this module should read from those channels.

use crate::ops_worker::JobDone;
use crate::slint_generated::*;
use crate::state::{App, Pane};
use crate::view::{
    apply_shell_icon, fill_size_cell, push_pane, push_status, push_view, to_slint_image,
};
use crate::{logging, navigation, operations};
use navigation::{apply_listing, reload_pane};
use slint::{ComponentHandle, SharedString, Timer, TimerMode};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use crate::format;

fn pane_for_token(app: &App, token: u64) -> Option<Pane> {
    [Pane::Left, Pane::Right]
        .into_iter()
        .find(|pane| app.pane_load(*pane).token == token)
}

fn reload_both(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    navigation::reload_all(ui, app);
}

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
        let text = ui.get_status_message();
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
            ui.set_status_message(SharedString::default());
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
    // Đếm nhịp toast thành công/lỗi; mỗi nhịp 120 ms, ~4 giây thì tự đóng.
    let toast_ticks = Rc::new(std::cell::Cell::new(0u32));
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(120), move || {
        let Some(ui) = w.upgrade() else { return };
        let running = ui.get_operation_running();
        if running {
            toast_ticks.set(0);
            if let Some(start) = a.borrow().operation_started {
                let elapsed = start.elapsed();
                ui.set_operation_seconds(elapsed.as_secs() as i32);
                if elapsed >= operations::SHOW_AFTER && !ui.get_operation_visible() {
                    ui.set_operation_visible(true);
                }
            }
            operations::phase_tick(&ui);
        } else if ui.get_operation_visible() && ui.get_operation_tone() != 0 {
            let ticks = toast_ticks.get() + 1;
            if ticks >= 33 {
                operations::dismiss(&ui);
                toast_ticks.set(0);
            } else {
                toast_ticks.set(ticks);
            }
        } else {
            // Không còn thao tác nhưng thẻ vẫn ở trạng thái "đang chạy":
            // dọn để một job bị thay thế không để lại spinner vĩnh viễn.
            if ui.get_operation_visible() && ui.get_operation_tone() == 0 {
                operations::dismiss(&ui);
            }
            toast_ticks.set(0);
        }

        let snapshot = {
            let a = a.borrow();
            a.worker.progress().snapshot()
        };
        match snapshot {
            Some((done, total)) if total > 0 => {
                if total == usize::MAX || total <= 1 {
                    // Không đủ mục để chia phần trăm (một tệp/thư mục, hoặc
                    // không biết tổng): dùng thanh chạy vô định + đếm giây.
                    // Một mục cũng không huỷ được giữa chừng.
                    ui.set_progress(0.0);
                    ui.set_operation_total(0);
                    ui.set_operation_done(0);
                    ui.set_cancellable(total == usize::MAX);
                } else {
                    ui.set_progress(done as f32 / total as f32);
                    ui.set_operation_total(total as i32);
                    ui.set_operation_done(done as i32);
                    // Chỉ cho huỷ khi còn việc: huỷ một thao tác đã xong là vô nghĩa.
                    ui.set_cancellable(done < total);
                }
            }
            _ => {
                if ui.get_progress() != 0.0 {
                    ui.set_progress(0.0);
                }
                if ui.get_cancellable() {
                    ui.set_cancellable(false);
                }
                if running {
                    ui.set_operation_total(0);
                    ui.set_operation_done(0);
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
                Ok(JobDone::Loaded {
                    token,
                    path,
                    result,
                }) => {
                    let pane = {
                        let s = a.borrow();
                        pane_for_token(&s, token)
                    };
                    let Some(pane) = pane else { continue };
                    a.borrow_mut().pane_load_mut(pane).in_flight = false;
                    match result {
                        Ok(listing) => {
                            apply_listing(&ui, &a, pane, listing);
                        }
                        Err(e) => {
                            let active = a.borrow().active_pane == pane;
                            if pane == Pane::Right {
                                ui.set_right_loading(false);
                                ui.set_right_error_text(SharedString::from(e.message.as_str()));
                            }
                            if active {
                                ui.set_busy(false);
                                if pane == Pane::Left {
                                    ui.set_error_text(SharedString::from(e.message.as_str()));
                                }
                            }
                            // The history must not keep a path the user never saw.
                            {
                                let mut s = a.borrow_mut();
                                if let Some(tab) = s.tab_for_mut(pane) {
                                    let back = tab.current().parent().map(Path::to_path_buf);
                                    if let Some(p) = back {
                                        if p != path {
                                            tab.history.replace_current(p);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(JobDone::OpDone {
                    label,
                    result,
                    undo,
                }) => {
                    let ok = result.is_ok();
                    operations::finish(&ui, &a, &label, ok);
                    match result {
                        Ok(()) => {
                            ui.set_status_message(SharedString::from(label.as_str()));
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
                            reload_both(&ui, &a);
                        }
                        Err(e) => {
                            logging::write(&format!("thao tác lỗi [{}]: {}", e.code, e.message));
                            // Câu giải thích thay cho "(os error 5)" trần trụi.
                            let text = format::explain_error(&e.code, &e.message);
                            ui.set_error_text(SharedString::from(text.as_str()));
                            // Batch lỗi một phần vẫn có thể đã di chuyển/chép
                            // được vài mục; người dùng phải hoàn tác được đúng
                            // những mục đó.
                            if let Some(action) = undo {
                                a.borrow_mut().undo.record(action);
                            }
                            reload_both(&ui, &a);
                        }
                    }
                }
                Ok(JobDone::UndoDone {
                    action,
                    redo,
                    result,
                }) => {
                    let label = if redo {
                        action.redo_label()
                    } else {
                        action.undo_label()
                    };
                    operations::finish(&ui, &a, &label, result.is_ok());
                    match result {
                        Ok(()) => {
                            ui.set_status_message(SharedString::from(label.as_str()));
                            {
                                let mut s = a.borrow_mut();
                                if redo {
                                    s.undo.redone(action);
                                } else {
                                    s.undo.undone(action);
                                }
                            }
                            reload_both(&ui, &a);
                        }
                        Err(e) => {
                            // Trả action về ngăn cũ để người dùng thử lại được.
                            {
                                let mut s = a.borrow_mut();
                                if redo {
                                    s.undo.undone(action);
                                } else {
                                    s.undo.redone(action);
                                }
                            }
                            logging::write(&format!(
                                "hoàn tác/làm lại lỗi [{}]: {}",
                                e.code, e.message
                            ));
                            let text = format::explain_error(&e.code, &e.message);
                            ui.set_error_text(SharedString::from(text.as_str()));
                        }
                    }
                }
                Ok(JobDone::FolderSize { path, bytes, token }) => {
                    // Token 0 là lệnh thủ công; token khác phải thuộc một lần
                    // đọc còn hiện hành, nếu không kết quả là của thư mục cũ.
                    if token != 0 {
                        let s = a.borrow();
                        if pane_for_token(&s, token).is_none() {
                            continue;
                        }
                    }
                    let path_str = path.to_string_lossy().into_owned();
                    let mut updated_panes = Vec::new();
                    {
                        let mut s = a.borrow_mut();
                        for pane in [Pane::Left, Pane::Right] {
                            if let Some(tab) = s.tab_for_mut(pane) {
                                if let Some(e) = tab.entries.iter_mut().find(|e| e.path == path_str)
                                {
                                    e.size = Some(bytes);
                                    updated_panes.push(pane);
                                }
                            }
                        }
                    }
                    for pane in updated_panes {
                        fill_size_cell(&ui, &a, pane, &path, bytes);
                    }
                    let size_text =
                        format!("Kích thước thư mục: {}", format::format_size(Some(bytes)));
                    if token == 0 {
                        // Lệnh thủ công: kết thúc trạng thái busy + thẻ tiến trình.
                        operations::finish(&ui, &a, &size_text, true);
                    }
                    ui.set_status_message(SharedString::from(size_text.as_str()));
                }
                Ok(JobDone::GitStatus { dir, status, token }) => {
                    let pane = {
                        let s = a.borrow();
                        pane_for_token(&s, token)
                    };
                    if pane != Some(Pane::Left) {
                        continue;
                    }
                    let active = a.borrow().active_pane;
                    let still_here = a
                        .borrow()
                        .tab_for(Pane::Left)
                        .map(|t| t.current() == dir.as_path())
                        .unwrap_or(false);
                    if !still_here {
                        continue;
                    }
                    a.borrow_mut().git_status = status;
                    if active == Pane::Left {
                        push_view(&ui, &a);
                    } else {
                        push_pane(&ui, &a, Pane::Left);
                    }
                }
                Ok(JobDone::Thumbnail { path, image, token }) => {
                    let pane = {
                        let s = a.borrow();
                        pane_for_token(&s, token)
                    };
                    let Some(pane) = pane else { continue };
                    apply_shell_icon(&ui, &a, pane, &path, to_slint_image(&image));
                }
                Ok(JobDone::SearchDone {
                    token,
                    root,
                    hits,
                    cancelled,
                }) => {
                    // Kết quả chỉ áp dụng cho đúng khung đã phát lệnh tìm.
                    let pane = {
                        let s = a.borrow();
                        pane_for_token(&s, token)
                    };
                    let Some(pane) = pane else { continue };
                    if a.borrow().active_pane != pane {
                        continue;
                    }
                    let count = hits.len();
                    {
                        let mut s = a.borrow_mut();
                        let Some(tab) = s.tab_for_mut(pane) else {
                            continue;
                        };
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
                    // Tìm kiếm không dùng thẻ tiến trình, nhưng phải trả lại
                    // thanh trạng thái; trước đây busy kẹt lại sau khi tìm xong.
                    if !ui.get_operation_running() {
                        ui.set_busy(false);
                        ui.set_busy_text(SharedString::default());
                    }
                    ui.set_status_message(SharedString::from(text.as_str()));
                }
                Ok(JobDone::ShellIcon { path, icon, token }) => {
                    let pane = {
                        let s = a.borrow();
                        pane_for_token(&s, token)
                    };
                    let Some(pane) = pane else { continue };
                    let image = to_slint_image(&icon);
                    apply_shell_icon(&ui, &a, pane, &path, image);
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
        let busy = {
            let s = a.borrow();
            s.pane_load(s.active_pane).in_flight
        };
        let pane = a.borrow().active_pane;
        if fired && !busy {
            reload_pane(&ui, &a, pane);
        }
    });
    std::mem::forget(timer);
}
