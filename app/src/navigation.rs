//! Navigation: loading a directory, walking history, and opening a row.
//!
//! Every path change goes through here so the load token, the directory watcher
//! and the "in flight" flag stay in sync — those three are what stop a stale
//! listing from overwriting a folder the user already left.

use crate::slint_generated::*;
use crate::state::App;
use crate::view::{color_from_u32, push_status, push_view, queue_folder_sizes, queue_shell_icons};
use crate::{format, logging, ops_worker, preview};
use ops_worker::Job;
use crate::tabs::sync_tabs;
use slint::{ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub fn build_sidebar(ui: &AppWindow) {
    let pinned: Vec<NavEntry> = files_fs::drives::quick_access()
        .into_iter()
        .map(|(label, path)| {
            let (icon, glyph) = format::folder_glyph(&label);
            NavEntry {
                id: SharedString::from(format!("pin:{path}").as_str()),
                label: SharedString::from(label.as_str()),
                path: SharedString::from(path.as_str()),
                icon: SharedString::from(icon),
                glyph: color_from_u32(glyph),
                indent: 0,
                expandable: false,
                expanded: false,
            }
        })
        .collect();
    ui.set_pinned(ModelRc::new(VecModel::from(pinned)));

    let drives: Vec<NavEntry> = files_fs::list_drives()
        .into_iter()
        .map(|d| {
            let label = match &d.name {
                Some(name) if !name.is_empty() => format!("{} ({})", name, d.label),
                _ => d.label.clone(),
            };
            NavEntry {
                id: SharedString::from(format!("drive:{}", d.path).as_str()),
                label: SharedString::from(label.as_str()),
                path: SharedString::from(d.path.as_str()),
                icon: SharedString::from("hard_drive"),
                glyph: color_from_u32(0x6b7280),
                indent: 0,
                expandable: false,
                expanded: false,
            }
        })
        .collect();
    ui.set_drives(ModelRc::new(VecModel::from(drives)));
}

/// Ask the worker for the current directory, and start watching it.
pub fn reload(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let (path, sort, opts, token) = {
        let mut a = app.borrow_mut();
        a.load_token += 1;
        a.load_in_flight = true;
        let token = a.load_token;
        let path = a.tab().current().to_path_buf();
        let sort = a.tab().sort;
        let opts = a.list_options();
        (path, sort, opts, token)
    };

    ui.set_busy(true);
    ui.set_busy_text(SharedString::from("Đang đọc thư mục…"));
    app.borrow().worker.submit(Job::Load { path, sort, opts, token });
}

/// Point the watcher at `path`, replacing any previous watch.
pub fn retarget_watch(app: &mut App, path: &Path) {
    if app.watched_path.as_deref() == Some(path) {
        return;
    }
    app.watch = files_fs::watch_directory(path).ok();
    app.watched_path = Some(path.to_path_buf());
}

pub fn apply_listing(ui: &AppWindow, app: &Rc<RefCell<App>>, listing: files_core::Listing) {
    {
        let mut a = app.borrow_mut();
        let active = a.active;
        let path = a.tabs[active].current().to_path_buf();
        a.tabs[active].entries = listing.entries;
        a.tabs[active].selection.clear();
        a.tabs[active].anchor = None;
        a.tabs[active].rebuild_visible();
        retarget_watch(&mut a, &path);
        a.tabs[active].hidden_count = listing.hidden_count;
    }
    // Dọn thẻ của những mục đã biến mất trong thư mục này, để tệp cấu hình
    // không phình ra và đường dẫn bị tái sử dụng không thừa hưởng màu cũ.
    {
        let mut a = app.borrow_mut();
        let current = a.tab().current().to_path_buf();
        let before = a.tags.assignments.len();
        a.tags.prune_missing(&current);
        if a.tags.assignments.len() != before {
            let path = a.tag_path.clone();
            let store = a.tags.clone();
            drop(a);
            if let Err(e) = store.save(&path) {
                logging::write(&format!("không lưu được thẻ sau khi dọn: {e}"));
            }
        }
    }
    ui.set_error_text(SharedString::default());
    ui.set_busy(false);
    push_view(ui, app);
    push_status(ui, app);

    // Tính kích thước mọi thư mục con ở luồng riêng: người dùng thấy "—" rồi
    // từng ô tự điền, thay vì phải bấm "Tính kích thước" cho từng cái.
    queue_folder_sizes(app);
    // Icon thật của Windows cho exe/msi/lnk… — chạy sau khi bảng đã hiện để
    // không làm chậm lần vẽ đầu.
    queue_shell_icons(app);

    // Bảng thuộc tính của mục đầu tiên: bỏ trống thì panel trông như hỏng.
    let first = {
        let s = app.borrow();
        let tab = s.tab();
        tab.visible
            .first()
            .and_then(|i| tab.entries.get(*i))
            .cloned()
    };
    if let Some(entry) = first {
        preview::show(ui, &entry);
    } else {
        preview::clear(ui);
    }
}

pub fn navigate_to(ui: &AppWindow, app: &Rc<RefCell<App>>, target: PathBuf) {
    if !target.is_dir() {
        ui.set_error_text(SharedString::from(
            format!("Không tìm thấy thư mục: {}", target.display()).as_str(),
        ));
        ui.set_busy(false);
        return;
    }
    {
        let mut s = app.borrow_mut();
        s.tab_mut().history.push(target);
        s.tab_mut().filter.clear();
        s.tab_mut().selection.clear();
    }
    ui.set_filtering(false);
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

pub fn open_row(ui: &AppWindow, app: &Rc<RefCell<App>>, visible_index: usize) {
    let target = {
        let s = app.borrow();
        let tab = s.tab();
        tab.visible
            .get(visible_index)
            .and_then(|i| tab.entries.get(*i))
            .map(|e| (e.is_dir(), PathBuf::from(&e.path)))
    };
    let Some((is_dir, path)) = target else { return };
    if is_dir {
        navigate_to(ui, app, path);
    } else if let Err(e) = open::that_detached(&path) {
        ui.set_error_text(SharedString::from(
            format!("Không mở được tệp: {e}").as_str(),
        ));
    }
}
