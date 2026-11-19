//! Slint callback registration.
//!
//! Every callback here does three things at most: upgrade the weak handle, read
//! what it needs from [`App`], and call one function in a business module. No
//! logic lives in this file — if a callback grows a decision, that decision
//! belongs in the module it calls.

use crate::commands::{
    menu, menu_header, menu_submenu, palette_model, run_command, set_view_mode, tag_menu_items,
    toggle_hidden, toggle_preview, toggle_sidebar, toggle_theme,
};
use crate::dialogs::confirm_dialog;
use crate::drag;
use crate::file_ops;
use crate::logging;
use crate::navigation::{go_up, navigate_to, open_row, reload};
use crate::operations;
use crate::panes;
use crate::selection::select_row;
use crate::slint_generated::*;
use crate::state::{persist_ui, App, Pane, Tab};
use crate::tabs::{close_tab_at, sync_tabs};
use crate::view::{push_pane, push_view};
use files_core::{SortBy, SortDirection, SortSpec};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn show_row_menu(ui: &AppWindow, a: &Rc<RefCell<App>>, i: i32, x: f32, y: f32) {
    // Lệnh nén/giải nén chỉ có nghĩa với đúng loại mục.
    let (is_dir, is_archive) = {
        let s = a.borrow();
        match s.tab().selected_entries().first() {
            Some(e) => (
                e.is_dir(),
                e.extension
                    .as_deref()
                    .map(|x| x.eq_ignore_ascii_case("zip"))
                    .unwrap_or(false),
            ),
            None => (false, false),
        }
    };
    // Thẻ hiện có của mục đang chọn, để menu hiện dấu ✓ và bật/tắt
    // mục "Bỏ thẻ" cho đúng. Danh sách có thể ngắn hơn 4 thẻ nếu
    // người dùng sửa tags.json, nên không index cứng.
    let (tag_list, attached) = {
        let s = a.borrow();
        let tag_list: Vec<_> = s.tags.tags.iter().take(4).cloned().collect();
        let mut attached = vec![false; tag_list.len()];
        if let Some(first) = s.tab().selected_entries().first() {
            let owned = s.tags.tags_for(Path::new(&first.path));
            for (slot, tag) in tag_list.iter().enumerate() {
                attached[slot] = owned.iter().any(|t| t.uid == tag.uid);
            }
        }
        (tag_list, attached)
    };
    // Ứng dụng Shell đề xuất cho tệp đang chọn. Liệt kê một lần khi mở
    // menu; bấm vào mục chỉ cần gọi handler đã lưu.
    let target_file = if i >= 0 {
        let s = a.borrow();
        s.tab()
            .selected_entries()
            .first()
            .filter(|e| !e.is_dir())
            .map(|e| PathBuf::from(&e.path))
    } else {
        None
    };
    let open_with = match &target_file {
        Some(path) => match files_fs::open_with_apps(path) {
            Ok(apps) => apps,
            Err(e) => {
                logging::write(&format!(
                    "không liệt kê được ứng dụng mở tệp: {}",
                    e.message
                ));
                Vec::new()
            }
        },
        None => Vec::new(),
    };
    a.borrow_mut().open_with = open_with;
    let open_with_names: Vec<String> = a
        .borrow()
        .open_with
        .iter()
        .map(|app| app.name().to_string())
        .collect();
    // Menu con "Mở bằng" giữ menu chính ngắn; hộp thoại hệ thống luôn
    // có mặt kể cả khi Shell không trả về handler nào.
    let mut open_with_items: Vec<MenuItem> = open_with_names
        .iter()
        .enumerate()
        .map(|(index, name)| menu(&format!("open-with:{index}"), name, "open", "", true, false))
        .collect();
    if !open_with_items.is_empty() {
        open_with_items.push(menu("", "", "", "", false, true));
    }
    open_with_items.push(menu(
        "open-with-dialog",
        "Ứng dụng khác…",
        "open",
        "",
        true,
        false,
    ));
    ui.set_open_with_items(ModelRc::new(VecModel::from(open_with_items)));
    ui.set_open_with_open(false);

    let mut items = vec![
        menu("open", "Mở", "open", "Enter", i >= 0, false),
        menu(
            "open-new-tab",
            "Mở trong tab mới",
            "window",
            "",
            i >= 0,
            false,
        ),
    ];
    if target_file.is_some() {
        items.push(menu_submenu("open-with-menu", "Mở bằng", "open"));
    }
    items.extend([
        menu("", "", "", "", false, true),
        menu("copy", "Sao chép", "copy", "Ctrl+C", i >= 0, false),
        menu("cut", "Cắt", "cut", "Ctrl+X", i >= 0, false),
        menu("rename", "Đổi tên", "rename", "F2", i >= 0, false),
        menu("delete", "Xoá", "delete", "Del", i >= 0, false),
        menu(
            "delete-permanent",
            "Xoá vĩnh viễn",
            "delete",
            "Shift+Del",
            i >= 0,
            false,
        ),
        menu("", "", "", "", false, true),
        menu(
            "copy-path",
            "Sao chép đường dẫn",
            "link",
            "Ctrl+Shift+C",
            i >= 0,
            false,
        ),
        menu("", "", "", "", false, true),
        // Thẻ màu: đổi màu icon của tệp hoặc thư mục.
        menu_header("Thẻ màu"),
    ]);
    items.extend(tag_menu_items(&tag_list, &attached));
    items.extend([
        menu(
            "tag-clear",
            "Bỏ thẻ",
            "dismiss",
            "",
            i >= 0 && attached.iter().any(|x| *x),
            false,
        ),
        menu("", "", "", "", false, true),
        menu("compress", "Nén thành ZIP", "folder_zip", "", i >= 0, false),
        menu("extract", "Giải nén", "folder_open", "", is_archive, false),
        menu("", "", "", "", false, true),
        menu(
            "hash-sha256",
            "Mã băm SHA-256",
            "shield",
            "",
            i >= 0 && !is_dir,
            false,
        ),
        menu(
            "hash-md5",
            "Mã băm MD5",
            "shield",
            "",
            i >= 0 && !is_dir,
            false,
        ),
        menu("", "", "", "", false, true),
        menu(
            "folder-size",
            "Tính kích thước",
            "calculator",
            "",
            is_dir,
            false,
        ),
        menu("properties", "Thuộc tính", "info", "", i >= 0, false),
    ]);
    ui.set_menu_items(ModelRc::new(VecModel::from(items)));
    ui.set_menu_x(x);
    ui.set_menu_y(y);
    ui.set_menu_open(true);
}

fn show_background_menu(ui: &AppWindow, a: &Rc<RefCell<App>>, x: f32, y: f32) {
    let can_paste = !a.borrow().clipboard.is_empty();
    let pinned_now = {
        let a = a.borrow();
        let here = a.tab().current().to_string_lossy().into_owned();
        a.settings.pinned_folders.iter().any(|p| p == &here)
    };
    let items = vec![
        menu(
            "new-folder",
            "Thư mục mới",
            "folder",
            "Ctrl+Shift+N",
            true,
            false,
        ),
        menu("new-file", "Tệp mới", "document", "", true, false),
        menu("", "", "", "", false, true),
        menu(
            "paste",
            "Dán",
            "clipboard_paste",
            "Ctrl+V",
            can_paste,
            false,
        ),
        menu(
            "select-all",
            "Chọn tất cả",
            "checkmark",
            "Ctrl+A",
            true,
            false,
        ),
        menu("", "", "", "", false, true),
        menu("refresh", "Làm mới", "arrow_clockwise", "F5", true, false),
        // Ghim thư mục đang mở; nhãn đổi theo trạng thái hiện tại.
        menu(
            "pin-folder",
            if pinned_now {
                "Bỏ ghim thư mục này"
            } else {
                "Ghim vào thanh bên"
            },
            "pin",
            "",
            true,
            false,
        ),
        menu(
            "open-terminal",
            "Mở Terminal tại đây",
            "code",
            "",
            true,
            false,
        ),
        menu("", "", "", "", false, true),
        menu(
            "toggle-extensions",
            "Ẩn/hiện phần mở rộng",
            "document",
            "",
            true,
            false,
        ),
        menu(
            "toggle-folders-first",
            "Thư mục trước tệp",
            "arrow_sort",
            "",
            true,
            false,
        ),
        // Vừa cột theo nội dung: trước đây chỉ có trong Command Palette nên
        // người dùng khó tìm. Đặt cạnh các lệnh hiển thị khác.
        menu(
            "autofit-columns",
            "Vừa cột theo nội dung",
            "resize",
            "",
            true,
            false,
        ),
        // Nhóm: các mục phẳng thay vì menu con. Menu con chỉ có một cơ chế
        // (`open-with`) và nó gắn với danh sách ứng dụng, không dùng lại được
        // cho danh sách tiêu chí nhóm.
        menu_header("Nhóm theo"),
        menu("group-none", "Không nhóm", "apps_list", "", true, false),
        menu("group-name", "Tên", "text_bullet_list", "", true, false),
        menu("group-modified", "Ngày sửa", "clock", "", true, false),
        menu("group-size", "Kích thước", "resize", "", true, false),
        menu("group-kind", "Loại", "document", "", true, false),
        menu("group-tag", "Thẻ màu", "tag", "", true, false),
        menu(
            "toggle-hidden",
            if a.borrow().show_hidden {
                "Ẩn mục ẩn"
            } else {
                "Hiện mục ẩn"
            },
            "eye",
            "Ctrl+H",
            true,
            false,
        ),
    ];
    ui.set_menu_items(ModelRc::new(VecModel::from(items)));
    ui.set_menu_x(x);
    ui.set_menu_y(y);
    ui.set_menu_open(true);
}

fn apply_sort(ui: &AppWindow, app: &Rc<RefCell<App>>, column: &str) {
    {
        let mut s = app.borrow_mut();
        let by = match column {
            "size" => SortBy::Size,
            "modified" => SortBy::DateModified,
            "kind" => SortBy::Kind,
            _ => SortBy::Name,
        };
        let t = s.tab_mut();
        let direction = if t.sort.by == by && t.sort.direction == SortDirection::Ascending {
            SortDirection::Descending
        } else {
            SortDirection::Ascending
        };
        t.sort = SortSpec {
            by,
            direction,
            folder_order: t.sort.folder_order,
        };
        let spec = t.sort;
        spec.sort(&mut t.entries);
        t.rebuild_visible();
    }
    push_view(ui, app);
}

pub fn wire_callbacks(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_navigate(move |p| {
            let Some(ui) = w.upgrade() else { return };
            navigate_to(&ui, &a, PathBuf::from(p.as_str()));
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_sidebar_activated(move |id| {
            let Some(ui) = w.upgrade() else { return };
            let path = id
                .split_once(':')
                .map(|(_, p)| p.to_string())
                .unwrap_or_default();
            if !path.is_empty() {
                navigate_to(&ui, &a, PathBuf::from(path));
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_open_row(move |i| {
            let Some(ui) = w.upgrade() else { return };
            panes::activate(&ui, &a, Pane::Left);
            open_row(&ui, &a, i as usize);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_select_row(move |i, mode| {
            let Some(ui) = w.upgrade() else { return };
            panes::activate(&ui, &a, Pane::Left);
            select_row(&ui, &a, i as usize, mode);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_go_back(move || {
            let Some(ui) = w.upgrade() else { return };
            let changed = {
                let mut s = a.borrow_mut();
                let next = s.tab_mut().history.back().map(|p| p.to_path_buf());
                next.is_some()
            };
            if changed {
                reload(&ui, &a);
                sync_tabs(&ui, &a.borrow());
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_go_forward(move || {
            let Some(ui) = w.upgrade() else { return };
            let changed = {
                let mut s = a.borrow_mut();
                let next = s.tab_mut().history.forward().map(|p| p.to_path_buf());
                next.is_some()
            };
            if changed {
                reload(&ui, &a);
                sync_tabs(&ui, &a.borrow());
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_go_up(move || {
            let Some(ui) = w.upgrade() else { return };
            go_up(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_refresh(move || {
            let Some(ui) = w.upgrade() else { return };
            reload(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_address_submitted(move |text| {
            let Some(ui) = w.upgrade() else { return };
            let p = files_core::path::normalize(text.as_str());
            ui.set_editing_address(false);
            // Không `is_dir()` ở đây: `Job::Load` kiểm tra và báo lỗi trên
            // luồng nền, nên đường dẫn sai vẫn hiện thông báo mà không treo UI.
            navigate_to(&ui, &a, p);
        });
    }
    {
        let w = ui.as_weak();
        ui.on_start_edit_address(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_editing_address(true);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_sort_by(move |column| {
            let Some(ui) = w.upgrade() else { return };
            apply_sort(&ui, &a, column.as_str());
        });
    }
    {
        let w = ui.as_weak();
        ui.on_pane_area_width(move |width| {
            let Some(ui) = w.upgrade() else { return };
            ui.set_pane_half_width(((width - 1.0) / 2.0).max(200.0));
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_drag_start(move |pane, index, x, y, copy| {
            let Some(ui) = w.upgrade() else { return };
            drag::start(&ui, &a, pane.as_str(), index, x, y, copy);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_drag_move(move |x, y, copy| {
            let Some(ui) = w.upgrade() else { return };
            drag::moved(&ui, &a, x, y, copy);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_drag_drop(move |x, y, copy| {
            let Some(ui) = w.upgrade() else { return };
            drag::dropped(&ui, &a, x, y, copy);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_drag_cancel(move || {
            let Some(ui) = w.upgrade() else { return };
            drag::cancel(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_pane_geometry(move |side, x, y, width, height| {
            let Some(_ui) = w.upgrade() else { return };
            let pane = if side.as_str() == "right" {
                Pane::Right
            } else {
                Pane::Left
            };
            let mut s = a.borrow_mut();
            if width > 0.0 && height > 0.0 {
                s.drag.pane_bounds[pane.index()] = Some(crate::state::Rect {
                    x,
                    y,
                    w: width,
                    h: height,
                });
            } else {
                s.drag.pane_bounds[pane.index()] = None;
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_sidebar_zone(move |path, x, y, width, height| {
            let Some(_ui) = w.upgrade() else { return };
            let mut s = a.borrow_mut();
            s.drag.sidebar_zones.retain(|(p, _)| p != path.as_str());
            if !path.is_empty() && width > 0.0 && height > 0.0 {
                s.drag.sidebar_zones.push((
                    path.to_string(),
                    crate::state::Rect {
                        x,
                        y,
                        w: width,
                        h: height,
                    },
                ));
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_pane_activated(move |side| {
            let Some(ui) = w.upgrade() else { return };
            let pane = if side.as_str() == "right" {
                Pane::Right
            } else {
                Pane::Left
            };
            panes::activate(&ui, &a, pane);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_right_open_row(move |i| {
            let Some(ui) = w.upgrade() else { return };
            panes::activate(&ui, &a, Pane::Right);
            open_row(&ui, &a, i as usize);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_right_select_row(move |i, mode| {
            let Some(ui) = w.upgrade() else { return };
            panes::activate(&ui, &a, Pane::Right);
            select_row(&ui, &a, i as usize, mode);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_right_sort_by(move |column| {
            let Some(ui) = w.upgrade() else { return };
            panes::activate(&ui, &a, Pane::Right);
            apply_sort(&ui, &a, column.as_str());
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_right_grid_width_changed(move |width| {
            let Some(ui) = w.upgrade() else { return };
            let needs_view = {
                let mut s = a.borrow_mut();
                let old = s.right_grid_width;
                s.right_grid_width = width;
                crate::view::grid_width_change_needs_view(old, width, s.view_mode)
            };
            if needs_view {
                push_pane(&ui, &a, Pane::Right);
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_hidden(move || {
            let Some(ui) = w.upgrade() else { return };
            toggle_hidden(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_sidebar(move || {
            let Some(ui) = w.upgrade() else { return };
            toggle_sidebar(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_preview(move || {
            let Some(ui) = w.upgrade() else { return };
            toggle_preview(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_set_view_mode(move |mode| {
            let Some(ui) = w.upgrade() else { return };
            set_view_mode(&ui, &a, mode);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_theme(move || {
            let Some(ui) = w.upgrade() else { return };
            toggle_theme(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_new_tab(move || {
            let Some(ui) = w.upgrade() else { return };
            {
                let mut s = a.borrow_mut();
                let start = s.tab().current().to_path_buf();
                s.tabs.push(Tab::new(start));
                s.active = s.tabs.len() - 1;
                s.active_pane = crate::state::Pane::Left;
            }
            sync_tabs(&ui, &a.borrow());
            reload(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_close_tab(move |i| {
            let Some(ui) = w.upgrade() else { return };
            // Dùng chung một đường với menu chuột phải, để nút × và menu không
            // bao giờ lệch hành vi.
            close_tab_at(&ui, &a, (i >= 0).then_some(i as usize));
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_select_tab(move |i| {
            let Some(ui) = w.upgrade() else { return };
            {
                let mut s = a.borrow_mut();
                if (i as usize) < s.tabs.len() {
                    s.active = i as usize;
                    s.active_pane = crate::state::Pane::Left;
                }
            }
            sync_tabs(&ui, &a.borrow());
            reload(&ui, &a);
            push_view(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_grid_width_changed(move |width| {
            let Some(ui) = w.upgrade() else { return };
            let needs_view = {
                let mut s = a.borrow_mut();
                let old = s.grid_width;
                s.grid_width = width;
                crate::view::grid_width_change_needs_view(old, width, s.view_mode)
            };
            if needs_view {
                push_pane(&ui, &a, Pane::Left);
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_column_resized(move |id, width| {
            let Some(_ui) = w.upgrade() else { return };
            let (min, max) = crate::settings::column_bounds(id.as_str());
            let width = (width.round().max(0.0) as u32).clamp(min, max);
            {
                let mut s = a.borrow_mut();
                s.settings.column_widths.insert(id.to_string(), width);
            }
            crate::logging::write(&format!("cột {id}: rộng {width}"));
            persist_ui(&a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_column_moved(move |id, direction| {
            let Some(ui) = w.upgrade() else { return };
            let model = {
                let mut s = a.borrow_mut();
                if crate::settings::move_column(
                    &mut s.settings.column_order,
                    id.as_str(),
                    direction,
                ) {
                    Some(crate::view::column_order_model(&s.settings.column_order))
                } else {
                    None
                }
            };
            if let Some(model) = model {
                ui.set_column_order(model);
                crate::logging::write(&format!("dời cột {id} {direction}"));
                persist_ui(&a);
            }
        });
    }
    {
        let w = ui.as_weak();
        ui.on_start_filter(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_filtering(true);
            ui.set_filter_text(SharedString::default());
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_filter_changed(move |t| {
            let Some(ui) = w.upgrade() else { return };
            let mut s = a.borrow_mut();
            s.tab_mut().filter = t.to_lowercase();
            s.tab_mut().rebuild_visible();
            drop(s);
            push_view(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_end_filter(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_filtering(false);
            ui.set_filter_text(SharedString::default());
            {
                let mut s = a.borrow_mut();
                s.tab_mut().filter.clear();
                s.tab_mut().rebuild_visible();
            }
            push_view(&ui, &a);
        });
    }
    {
        // Nút phụ của hộp thoại trùng tên: "Thay thế".
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_dialog_secondary(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_dialog_open(false);
            file_ops::do_paste_with(&ui, &a, files_fs::ConflictPolicy::Replace);
        });
    }
    {
        // Huỷ thao tác đang chạy: chỉ đặt cờ, worker dừng ở ranh giới tệp kế
        // tiếp. Không cắt ngang một tệp đang chép dở.
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_cancel_operation(move || {
            let Some(ui) = w.upgrade() else { return };
            a.borrow().worker.progress().request_cancel();
            operations::relabel(&ui, "Đang huỷ…");
            ui.set_cancellable(false);
        });
    }
    {
        let w = ui.as_weak();
        ui.on_dismiss_operation(move || {
            let Some(ui) = w.upgrade() else { return };
            operations::dismiss(&ui);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_command(move |id| {
            let Some(ui) = w.upgrade() else { return };
            run_command(&ui, &a, id.as_str());
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_copy_current_path(move || {
            let Some(ui) = w.upgrade() else { return };
            // Bấm đúp thanh địa chỉ: sao chép đường dẫn thư mục đang mở, không
            // phải đường dẫn của mục đang chọn.
            let path = {
                let s = a.borrow();
                s.tab().current().to_string_lossy().into_owned()
            };
            match files_fs::copy_text_to_clipboard(&path) {
                Ok(()) => ui.set_status_message(SharedString::from(
                    format!("Đã sao chép đường dẫn: {path}").as_str(),
                )),
                Err(e) => ui.set_error_text(SharedString::from(
                    format!("Không sao chép được: {}", e.message).as_str(),
                )),
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_autofit_column(move |id| {
            let Some(ui) = w.upgrade() else { return };
            crate::view::autofit_column(&ui, &a, id.as_str());
        });
    }
    {
        let w = ui.as_weak();
        ui.on_dialog_cancel(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_dialog_open(false);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_row(move |index| {
            let Some(ui) = w.upgrade() else { return };
            if index < 0 {
                return;
            }
            crate::selection::toggle_at(&ui, &a, index as usize);
        });
    }
    // ---- Settings ----
    // Cửa sổ Settings là cửa sổ riêng. Nó phát lệnh về đây để cửa sổ chính thực
    // thi và lưu, nên chỉ có một đường ghi `settings.json`.
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_open_settings(move || {
            let Some(ui) = w.upgrade() else { return };
            let cmd_ui = ui.as_weak();
            // Closure cần sở hữu bản `Rc` riêng: `open` cũng đang mượn `a`.
            let cmd_app = a.clone();
            let result =
                crate::settings_page::open(&a, crate::settings_page::DEFAULT_PAGE, move |id| {
                    if let Some(ui) = cmd_ui.upgrade() {
                        crate::commands::run_command(&ui, &cmd_app, id);
                    }
                });
            if let Err(e) = result {
                logging::write(&format!("không mở được cửa sổ Settings: {e}"));
                ui.set_error_text(SharedString::from(
                    format!("Không mở được cửa sổ Settings: {e}").as_str(),
                ));
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_dialog_confirm(move |value| {
            let Some(ui) = w.upgrade() else { return };
            confirm_dialog(&ui, &a, value.as_str());
        });
    }
    {
        let w = ui.as_weak();
        ui.on_dismiss_menu(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_open_with_open(false);
            ui.set_menu_open(false);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_menu_activated(move |id| {
            let Some(ui) = w.upgrade() else { return };
            ui.set_open_with_open(false);
            ui.set_menu_open(false);
            run_command(&ui, &a, id.as_str());
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_open_with_activated(move |id| {
            let Some(ui) = w.upgrade() else { return };
            ui.set_open_with_open(false);
            ui.set_menu_open(false);
            run_command(&ui, &a, id.as_str());
        });
    }
    {
        let w = ui.as_weak();
        let a = app.clone();
        ui.on_tab_context_menu(move |i, x, y| {
            let Some(ui) = w.upgrade() else { return };
            let count = a.borrow().tabs.len();
            let idx = i as usize;
            if idx >= count {
                return;
            }
            // Nhớ tab đang nhắm tới: các lệnh bên dưới đọc lại `menu-tab`.
            ui.set_menu_tab(i);
            let has_others = count > 1;
            ui.set_menu_items(ModelRc::new(VecModel::from(vec![
                menu("tab-duplicate", "Nhân bản tab", "window", "", true, false),
                menu("", "", "", "", false, true),
                menu("tab-close", "Đóng tab", "dismiss", "Ctrl+W", true, false),
                menu(
                    "tab-close-others",
                    "Đóng các tab khác",
                    "dismiss",
                    "",
                    has_others,
                    false,
                ),
                menu(
                    "tab-close-right",
                    "Đóng tab bên phải",
                    "dismiss",
                    "",
                    idx + 1 < count,
                    false,
                ),
                menu(
                    "tab-close-left",
                    "Đóng tab bên trái",
                    "dismiss",
                    "",
                    idx > 0,
                    false,
                ),
                menu("", "", "", "", false, true),
                menu(
                    "tab-close-all",
                    "Đóng tất cả tab",
                    "dismiss",
                    "",
                    true,
                    false,
                ),
            ])));
            ui.set_menu_x(x);
            ui.set_menu_y(y);
            ui.set_menu_open(true);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_row_context_menu(move |i, x, y| {
            let Some(ui) = w.upgrade() else { return };
            panes::activate(&ui, &a, Pane::Left);
            show_row_menu(&ui, &a, i, x, y);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_right_row_context_menu(move |i, x, y| {
            let Some(ui) = w.upgrade() else { return };
            panes::activate(&ui, &a, Pane::Right);
            show_row_menu(&ui, &a, i, x, y);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_background_context_menu(move |x, y| {
            let Some(ui) = w.upgrade() else { return };
            panes::activate(&ui, &a, Pane::Left);
            show_background_menu(&ui, &a, x, y);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_right_background_context_menu(move |x, y| {
            let Some(ui) = w.upgrade() else { return };
            panes::activate(&ui, &a, Pane::Right);
            show_background_menu(&ui, &a, x, y);
        });
    }
    {
        let w = ui.as_weak();
        ui.on_open_palette(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_palette_query(SharedString::default());
            ui.set_palette_items(palette_model(""));
            ui.set_palette_open(true);
        });
    }
    {
        let w = ui.as_weak();
        ui.on_close_palette(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_palette_open(false);
        });
    }
    {
        let w = ui.as_weak();
        ui.on_palette_changed(move |q| {
            let Some(ui) = w.upgrade() else { return };
            ui.set_palette_items(palette_model(q.as_str()));
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_palette_accepted(move |id| {
            let Some(ui) = w.upgrade() else { return };
            ui.set_palette_open(false);
            run_command(&ui, &a, id.as_str());
        });
    }
    {
        // Mọi `NavEntry` hiện đặt `expandable: false`, nên chưa có mục con nào
        // để mở/đóng. Giữ callback để UI biên dịch được, nhưng không giả vờ
        // xử lý: khi thêm nhóm thư mục (ghim sidebar) thì cài đặt ở đây.
        ui.on_sidebar_toggled(|_id| {});
    }
}
