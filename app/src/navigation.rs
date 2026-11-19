//! Navigation: loading a directory, walking history, and opening a row.
//!
//! Every path change goes through here so the load token, the directory watcher
//! and the "in flight" flag stay in sync — those three are what stop a stale
//! listing from overwriting a folder the user already left.

use crate::slint_generated::*;
use crate::state::{App, Pane};
use crate::tabs::sync_tabs;
use crate::view::{
    color_from_u32, push_pane, push_status, push_view, queue_folder_sizes, queue_shell_icons,
    queue_thumbnails,
};
use crate::{format, logging, ops_worker, preview};
use ops_worker::Job;
use slint::{ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub fn build_sidebar(ui: &AppWindow, app: &App) {
    // Ghim của người dùng đứng trước: đó là thứ họ tự chọn, đáng thấy trước.
    let mut pinned: Vec<NavEntry> = app
        .settings
        .pinned_folders
        .iter()
        .map(|path| {
            let label = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
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
    pinned.extend(
        files_fs::drives::quick_access()
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
            }),
    );
    ui.set_pinned(ModelRc::new(VecModel::from(pinned)));

    let system_drive = std::env::var("SystemDrive")
        .unwrap_or_else(|_| "C:".into())
        .to_ascii_lowercase();
    let drives: Vec<NavEntry> = files_fs::list_drives()
        .into_iter()
        .map(|d| {
            let label = match &d.name {
                Some(name) if !name.is_empty() => format!("{} ({})", name, d.label),
                _ => d.label.clone(),
            };
            // Ổ hệ thống dùng icon màn hình xanh như Files; ổ dữ liệu dùng
            // icon ổ cứng. Trước đây mọi ổ đều là một icon xám giống nhau.
            let system = d.path.to_ascii_lowercase().starts_with(&system_drive);
            let (icon, glyph) = if system {
                ("desktop", 0x2f86d4)
            } else {
                ("hard_drive", 0x6b7280)
            };
            NavEntry {
                id: SharedString::from(format!("drive:{}", d.path).as_str()),
                label: SharedString::from(label.as_str()),
                path: SharedString::from(d.path.as_str()),
                icon: SharedString::from(icon),
                glyph: color_from_u32(glyph),
                indent: 0,
                expandable: false,
                expanded: false,
            }
        })
        .collect();
    ui.set_drives(ModelRc::new(VecModel::from(drives)));
}

/// Ask the worker for the current directory of the active pane.
pub fn reload(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let pane = app.borrow().active_pane;
    reload_pane(ui, app, pane);
}

/// Reload both visible panes after an operation that may have changed either
/// side (copy/move/drop, visibility filters).
pub fn reload_all(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    reload_pane(ui, app, Pane::Left);
    if app.borrow().is_dual() {
        reload_pane(ui, app, Pane::Right);
    }
}

/// Ask the worker for one pane's directory. The two panes have independent
/// tokens, so a slow load in one pane can never overwrite the other.
pub fn reload_pane(ui: &AppWindow, app: &Rc<RefCell<App>>, pane: Pane) {
    let (path, sort, opts, token) = {
        let mut a = app.borrow_mut();
        let (path, sort) = {
            let Some(tab) = a.tab_for(pane) else { return };
            (tab.current().to_path_buf(), tab.sort)
        };
        let opts = a.list_options();
        let token = a.next_token();
        let load = a.pane_load_mut(pane);
        load.token = token;
        load.in_flight = true;
        (path, sort, opts, token)
    };

    if app.borrow().active_pane == pane {
        ui.set_busy(true);
        ui.set_busy_text(SharedString::from("Đang đọc thư mục…"));
    } else {
        ui.set_right_loading(true);
    }
    app.borrow().worker.submit(Job::Load {
        path,
        sort,
        opts,
        token,
    });
}

/// Point the watcher at `path`, replacing any previous watch.
pub fn retarget_watch(app: &mut App, path: &Path) {
    if app.watched_path.as_deref() == Some(path) {
        return;
    }
    app.watch = files_fs::watch_directory(path).ok();
    app.watched_path = Some(path.to_path_buf());
}

pub fn apply_listing(
    ui: &AppWindow,
    app: &Rc<RefCell<App>>,
    pane: Pane,
    listing: files_core::Listing,
) {
    let path = {
        let mut a = app.borrow_mut();
        let Some(tab) = a.tab_for_mut(pane) else {
            return;
        };
        let path = tab.current().to_path_buf();
        tab.entries = listing.entries;
        tab.selection.clear();
        tab.anchor = None;
        tab.rebuild_visible();
        tab.hidden_count = listing.hidden_count;
        a.pane_load_mut(pane).in_flight = false;
        if a.active_pane == pane {
            retarget_watch(&mut a, &path);
        }
        path
    };
    // Dọn thẻ của những mục đã biến mất trong thư mục này, để tệp cấu hình
    // không phình ra và đường dẫn bị tái sử dụng không thừa hưởng màu cũ.
    {
        let mut a = app.borrow_mut();
        let mut store = crate::tags::TagStore::load(&a.tag_path);
        let before = store.assignments.len();
        store.prune_missing(&path);
        a.tags = store;
        if a.tags.assignments.len() != before {
            let tag_path = a.tag_path.clone();
            let store = a.tags.clone();
            drop(a);
            if let Err(e) = store.save(&tag_path) {
                logging::write(&format!("không lưu được thẻ sau khi dọn: {e}"));
            }
        }
    }

    let active = app.borrow().active_pane == pane;
    if active {
        ui.set_error_text(SharedString::default());
        ui.set_busy(false);
        push_view(ui, app);
        push_status(ui, app);
    } else {
        ui.set_right_loading(false);
        push_pane(ui, app, pane);
    }

    queue_folder_sizes(app, pane);
    queue_shell_icons(app, pane);
    queue_thumbnails(app, pane);

    // Chỉ khung trái có cột git; đừng chạy `git` cho khung phải.
    if pane == Pane::Left {
        let (dir, token) = {
            let s = app.borrow();
            let Some(tab) = s.tab_for(pane) else { return };
            (tab.current().to_path_buf(), s.pane_load(pane).token)
        };
        if files_fs::repo_root(&dir).is_some() {
            app.borrow()
                .worker
                .submit(ops_worker::Job::GitStatus { dir, token });
        } else {
            app.borrow_mut().git_status.clear();
            if active {
                push_view(ui, app);
            }
        }
    }

    if active {
        // Bảng thuộc tính của mục đầu tiên: bỏ trống thì panel trông như hỏng.
        let first = {
            let s = app.borrow();
            let Some(tab) = s.tab_for(pane) else { return };
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

/// Move to the parent folder, if there is one.
pub fn go_up(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let parent = {
        let s = app.borrow();
        s.tab().current().parent().map(Path::to_path_buf)
    };
    if let Some(p) = parent {
        navigate_to(ui, app, p);
    }
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
