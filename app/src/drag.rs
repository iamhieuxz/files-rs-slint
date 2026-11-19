//! Kéo-thả nội bộ giữa hai khung và vào sidebar.
//!
//! Rust giữ nguồn kéo và quyết định đích từ toạ độ cửa sổ; UI chỉ báo vị trí
//! con trỏ và vẽ ghost/vùng thả. Nhờ vậy cùng một logic dùng được cho kéo-thả
//! nội bộ lẫn tệp từ Explorer.

use crate::file_ops;
use crate::panes;
use crate::slint_generated::*;
use crate::state::{App, Pane};
use slint::SharedString;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

enum DropTarget {
    Pane(Pane),
    Sidebar(String),
    None,
}

fn resolve(app: &App, x: f32, y: f32) -> DropTarget {
    if let Some((path, _)) = app
        .drag
        .sidebar_zones
        .iter()
        .find(|(path, rect)| !path.is_empty() && rect.contains(x, y))
    {
        // Không `is_dir()` giữa lúc kéo: mỗi lần di chuột sẽ là một lời gọi đĩa.
        // Đích được kiểm tra một lần khi thả.
        return DropTarget::Sidebar(path.clone());
    }
    for pane in [Pane::Left, Pane::Right] {
        if let Some(rect) = app.drag.pane_bounds[pane.index()] {
            if rect.contains(x, y) {
                return DropTarget::Pane(pane);
            }
        }
    }
    DropTarget::None
}

fn update_target(ui: &AppWindow, app: &Rc<RefCell<App>>, x: f32, y: f32) {
    let (target, sidebar_path, hover_pane, hover_sidebar) = {
        let a = app.borrow();
        match resolve(&a, x, y) {
            DropTarget::Pane(pane) => (pane.as_str().to_string(), String::new(), Some(pane), None),
            DropTarget::Sidebar(path) => ("sidebar".to_string(), path.clone(), None, Some(path)),
            DropTarget::None => (String::new(), String::new(), None, None),
        }
    };
    {
        let mut a = app.borrow_mut();
        a.drag.hover_pane = hover_pane;
        a.drag.hover_sidebar = hover_sidebar;
    }
    ui.set_drag_target(SharedString::from(target.as_str()));
    ui.set_drop_sidebar_path(SharedString::from(sidebar_path.as_str()));
}

fn clear_ui(ui: &AppWindow) {
    ui.set_drag_active(false);
    ui.set_drag_external(false);
    ui.set_drag_target(SharedString::default());
    ui.set_drop_sidebar_path(SharedString::default());
    ui.set_drag_count(0);
}

fn clear(app: &Rc<RefCell<App>>) {
    let mut a = app.borrow_mut();
    a.drag.source = None;
    a.drag.paths.clear();
    a.drag.copy = false;
    a.drag.hover_pane = None;
    a.drag.hover_sidebar = None;
}

pub fn start(
    ui: &AppWindow,
    app: &Rc<RefCell<App>>,
    pane: &str,
    _index: i32,
    x: f32,
    y: f32,
    copy: bool,
) {
    let pane = if pane == "right" {
        Pane::Right
    } else {
        Pane::Left
    };
    panes::activate(ui, app, pane);
    let paths = {
        let a = app.borrow();
        a.tab_for(pane)
            .map(|tab| tab.selected_paths())
            .unwrap_or_default()
    };
    if paths.is_empty() {
        return;
    }
    {
        let mut a = app.borrow_mut();
        a.drag.source = Some(pane);
        a.drag.paths = paths.clone();
        a.drag.copy = copy;
    }
    ui.set_drag_copy(copy);
    ui.set_drag_count(paths.len() as i32);
    ui.set_drag_x(x);
    ui.set_drag_y(y);
    ui.set_drag_active(true);
    ui.set_drag_external(false);
    update_target(ui, app, x, y);
}

pub fn moved(ui: &AppWindow, app: &Rc<RefCell<App>>, x: f32, y: f32, copy: bool) {
    if app.borrow().drag.source.is_none() {
        return;
    }
    app.borrow_mut().drag.copy = copy;
    ui.set_drag_copy(copy);
    ui.set_drag_x(x);
    ui.set_drag_y(y);
    update_target(ui, app, x, y);
}

pub fn dropped(ui: &AppWindow, app: &Rc<RefCell<App>>, x: f32, y: f32, copy: bool) {
    let (source, paths, copy, hover_pane, hover_sidebar) = {
        let mut a = app.borrow_mut();
        (
            a.drag.source.take(),
            std::mem::take(&mut a.drag.paths),
            copy || a.drag.copy,
            a.drag.hover_pane.take(),
            a.drag.hover_sidebar.take(),
        )
    };
    // Dùng đúng đích của lần DragMove gần nhất: đó là vùng người dùng thấy
    // sáng. Hit-test lại lúc thả có thể chạy sau khi overlay đổi layout.
    let (target_label, dest) = {
        let a = app.borrow();
        match (hover_pane, hover_sidebar) {
            (Some(pane), _) => (
                format!("pane {}", pane.as_str()),
                a.tab_for(pane).map(|tab| tab.current().to_path_buf()),
            ),
            (None, Some(path)) => (format!("sidebar {path}"), Some(PathBuf::from(path))),
            _ => ("none".to_string(), None),
        }
    };
    clear_ui(ui);
    if paths.is_empty() {
        return;
    }
    crate::logging::write(&format!(
        "internal drop at {x},{y} copy={copy} -> {target_label}"
    ));
    let Some(dest) = dest else { return };
    // Không `is_dir()` ở đây: đích được kiểm tra trong `drop_paths`, nơi có
    // thông báo lỗi và chạy ngoài đường đi của chuột.
    if let Some(source) = source {
        let same = {
            let a = app.borrow();
            a.tab_for(source)
                .map(|tab| tab.current() == dest.as_path())
                .unwrap_or(false)
        };
        if same {
            return;
        }
    }
    file_ops::drop_paths(ui, app, paths, dest, copy);
}

pub fn cancel(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    clear_ui(ui);
    clear(app);
}

// ---------------------------------------------------------------- native drop

/// Show the target highlight for a drag that started in another application.
pub fn native_show(ui: &AppWindow, app: &Rc<RefCell<App>>, count: usize, x: f32, y: f32) {
    ui.set_drag_copy(true);
    ui.set_drag_count(count as i32);
    ui.set_drag_x(x);
    ui.set_drag_y(y);
    ui.set_drag_active(true);
    ui.set_drag_external(true);
    update_target(ui, app, x, y);
}

pub fn native_hide(ui: &AppWindow) {
    clear_ui(ui);
}

/// Resolve the destination for a native drop and copy the files there.
pub fn native_drop(ui: &AppWindow, app: &Rc<RefCell<App>>, paths: Vec<PathBuf>, x: f32, y: f32) {
    clear_ui(ui);
    if paths.is_empty() {
        return;
    }
    let (hover_pane, hover_sidebar) = {
        let mut a = app.borrow_mut();
        (a.drag.hover_pane.take(), a.drag.hover_sidebar.take())
    };
    let dest = {
        let a = app.borrow();
        match (hover_pane, hover_sidebar) {
            (Some(pane), _) => a.tab_for(pane).map(|tab| tab.current().to_path_buf()),
            (None, Some(path)) => Some(PathBuf::from(path)),
            _ => match resolve(&a, x, y) {
                DropTarget::Pane(pane) => a.tab_for(pane).map(|tab| tab.current().to_path_buf()),
                DropTarget::Sidebar(path) => Some(PathBuf::from(path)),
                DropTarget::None => None,
            },
        }
    };
    if let Some(dest) = dest {
        if dest.is_dir() {
            file_ops::drop_paths(ui, app, paths, dest, true);
        }
    }
}
