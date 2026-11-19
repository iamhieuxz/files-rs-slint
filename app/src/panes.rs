//! Chuyển khung trái/phải và bật/tắt chế độ hai khung.

use crate::navigation::{reload_pane, retarget_watch};
use crate::preview;
use crate::slint_generated::*;
use crate::state::{App, Pane, Tab};
use crate::view::{push_chrome, push_status, push_view};
use slint::{ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

/// Make one pane own the address bar, command bar and keyboard.
pub fn activate(ui: &AppWindow, app: &Rc<RefCell<App>>, pane: Pane) {
    if pane == Pane::Right && !app.borrow().is_dual() {
        return;
    }
    {
        let mut a = app.borrow_mut();
        if a.active_pane == pane {
            // State có thể đã đúng trong khi property UI còn cũ (ví dụ sau khi
            // menu/navigation đổi khung); luôn đồng bộ lại trước khi thoát.
            ui.set_active_pane(SharedString::from(pane.as_str()));
            return;
        }
        a.active_pane = pane;
        if let Some(tab) = a.tab_for(pane) {
            let path = tab.current().to_path_buf();
            retarget_watch(&mut a, &path);
        }
    }

    ui.set_editing_address(false);
    ui.set_filtering(false);
    ui.set_error_text(SharedString::default());
    ui.set_active_pane(SharedString::from(pane.as_str()));
    let in_flight = app.borrow().pane_load(pane).in_flight;
    ui.set_busy(in_flight);
    if !in_flight {
        ui.set_busy_text(SharedString::default());
    }
    if pane == Pane::Right {
        ui.set_right_loading(false);
    }
    if let Some(tab) = app.borrow().tab_for(pane) {
        ui.set_filter_text(SharedString::from(tab.filter.as_str()));
    }
    // Không dựng lại model khi chỉ đổi khung đang nhận lệnh: cú nhấn chuột
    // đang giữ `TouchArea` của dòng, thay model sẽ phá thao tác kéo-thả.
    push_chrome(ui, app);
    push_status(ui, app);
    show_preview(ui, app);
}

/// Toggle the second pane. Enabling keeps the left pane active and loads the
/// right pane at the same folder; disabling returns focus to the left pane.
pub fn toggle_dual(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let enabling = !app.borrow().is_dual();
    if enabling {
        let start = app.borrow().tab().current().to_path_buf();
        {
            let mut a = app.borrow_mut();
            a.right = Some(Tab::new(start));
            a.loads[Pane::Right.index()] = Default::default();
            a.right_grid_width = 420.0;
        }
        ui.set_dual_pane(true);
        ui.set_active_pane(SharedString::from("left"));
        ui.set_right_loading(true);
        reload_pane(ui, app, Pane::Right);
        push_view(ui, app);
        push_status(ui, app);
    } else {
        {
            let mut a = app.borrow_mut();
            a.right = None;
            a.active_pane = Pane::Left;
            a.loads[Pane::Right.index()] = Default::default();
            let path = a.tabs[a.active].current().to_path_buf();
            retarget_watch(&mut a, &path);
        }
        ui.set_dual_pane(false);
        ui.set_active_pane(SharedString::from("left"));
        ui.set_right_loading(false);
        ui.set_right_rows(ModelRc::new(VecModel::from(Vec::<FileRow>::new())));
        ui.set_right_grid_rows(ModelRc::new(VecModel::from(Vec::<ModelRc<FileRow>>::new())));
        ui.set_right_path(SharedString::default());
        push_view(ui, app);
        push_status(ui, app);
        show_preview(ui, app);
    }
}

pub fn show_preview(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let entry = {
        let s = app.borrow();
        let Some(tab) = s.tab_for(s.active_pane) else {
            return;
        };
        tab.selected_entries()
            .first()
            .map(|e| (*e).clone())
            .or_else(|| {
                tab.visible
                    .first()
                    .and_then(|i| tab.entries.get(*i))
                    .cloned()
            })
    };
    if let Some(entry) = entry {
        preview::show(ui, &entry);
        queue_preview(app, &entry);
    } else {
        preview::clear(ui);
    }
}

/// Hỏi luồng nền phần dữ liệu nặng của panel xem trước.
///
/// Ảnh, văn bản và số mục con đều cần chạm đĩa; làm trên UI thread sẽ làm đứng
/// cửa sổ khi gặp thư mục mạng hoặc ảnh lớn.
pub fn queue_preview(app: &Rc<RefCell<App>>, entry: &files_core::Entry) {
    let kind = preview::kind_for(entry);
    if kind == files_fs::PreviewKind::None {
        return;
    }
    let token = {
        let s = app.borrow();
        s.pane_load(s.active_pane).token
    };
    app.borrow()
        .worker
        .submit_preview(std::path::PathBuf::from(&entry.path), kind, token);
}
