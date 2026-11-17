//! Slint callback registration.
//!
//! Every callback here does three things at most: upgrade the weak handle, read
//! what it needs from [`App`], and call one function in a business module. No
//! logic lives in this file — if a callback grows a decision, that decision
//! belongs in the module it calls.

use crate::commands::{menu, menu_tag, palette_model, run_command};
use crate::dialogs::confirm_dialog;
use crate::navigation::{navigate_to, open_row, reload};
use crate::selection::select_row;
use crate::slint_generated::*;
use crate::state::{persist_ui, App, Tab};
use crate::tabs::{close_tab_at, sync_tabs};
use crate::view::{push_view, refresh_theme};
use files_core::{SortBy, SortDirection, SortSpec};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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
            let path = id.split_once(':').map(|(_, p)| p.to_string()).unwrap_or_default();
            if !path.is_empty() {
                navigate_to(&ui, &a, PathBuf::from(path));
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_open_row(move |i| {
            let Some(ui) = w.upgrade() else { return };
            open_row(&ui, &a, i as usize);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_select_row(move |i, mode| {
            let Some(ui) = w.upgrade() else { return };
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
            let parent = {
                let s = a.borrow();
                s.tab().current().parent().map(Path::to_path_buf)
            };
            if let Some(p) = parent {
                navigate_to(&ui, &a, p);
            }
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
            if p.is_dir() {
                navigate_to(&ui, &a, p);
            } else {
                ui.set_error_text(SharedString::from(
                    format!("Không tìm thấy thư mục: {}", p.display()).as_str(),
                ));
                ui.set_busy(false);
            }
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
            {
                let mut s = a.borrow_mut();
                let by = match column.as_str() {
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
                t.sort = SortSpec { by, direction, folders_first: t.sort.folders_first };
                let spec = t.sort;
                spec.sort(&mut t.entries);
                t.rebuild_visible();
            }
            push_view(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_hidden(move || {
            let Some(ui) = w.upgrade() else { return };
            // Phải đọc rồi mới ghi trong hai lần mượn riêng: viết
            // `a.borrow_mut().x = !a.borrow().x` là mượn mutable và immutable
            // cùng lúc, và `RefCell` panic ngay — app chết ngay cú click đầu.
            let v = !a.borrow().show_hidden;
            a.borrow_mut().show_hidden = v;
            ui.set_show_hidden(v);
            persist_ui(&a);
            reload(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_sidebar(move || {
            let Some(ui) = w.upgrade() else { return };
            let v = !a.borrow().show_sidebar;
            a.borrow_mut().show_sidebar = v;
            ui.set_show_sidebar(v);
            persist_ui(&a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_preview(move || {
            let Some(ui) = w.upgrade() else { return };
            let v = !a.borrow().show_preview;
            a.borrow_mut().show_preview = v;
            ui.set_show_preview(v);
            persist_ui(&a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_set_view_mode(move |mode| {
            let Some(ui) = w.upgrade() else { return };
            a.borrow_mut().view_mode = mode;
            ui.set_view_mode(mode);
            persist_ui(&a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_theme(move || {
            let Some(ui) = w.upgrade() else { return };
            let dark = !a.borrow().dark;
            a.borrow_mut().dark = dark;
            refresh_theme(&ui, dark);
            persist_ui(&a);
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
                }
            }
            sync_tabs(&ui, &a.borrow());
            reload(&ui, &a);
            push_view(&ui, &a);
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
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_command(move |id| {
            let Some(ui) = w.upgrade() else { return };
            run_command(&ui, &a, id.as_str());
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
        ui.on_dialog_confirm(move |value| {
            let Some(ui) = w.upgrade() else { return };
            confirm_dialog(&ui, &a, value.as_str());
        });
    }
    {
        let w = ui.as_weak();
        ui.on_dismiss_menu(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_menu_open(false);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_menu_activated(move |id| {
            let Some(ui) = w.upgrade() else { return };
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
                menu("tab-close-others", "Đóng các tab khác", "dismiss", "", has_others, false),
                menu("tab-close-right", "Đóng tab bên phải", "dismiss", "", idx + 1 < count, false),
                menu("tab-close-left", "Đóng tab bên trái", "dismiss", "", idx > 0, false),
                menu("", "", "", "", false, true),
                menu("tab-close-all", "Đóng tất cả tab", "dismiss", "", true, false),
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
            // mục "Bỏ thẻ" cho đúng.
            let (tag_list, attached) = {
                let s = a.borrow();
                let mut attached = [false; 4];
                if let Some(first) = s.tab().selected_entries().first() {
                    let owned = s.tags.tags_for(Path::new(&first.path));
                    for (slot, tag) in s.tags.tags.iter().take(4).enumerate() {
                        attached[slot] = owned.iter().any(|t| t.uid == tag.uid);
                    }
                }
                (s.tags.tags.clone(), attached)
            };
            let items = vec![
                menu("open", "Mở", "open", "Enter", i >= 0, false),
                menu("open-new-tab", "Mở trong tab mới", "window", "", i >= 0, false),
                menu("", "", "", "", false, true),
                menu("copy", "Sao chép", "copy", "Ctrl+C", i >= 0, false),
                menu("cut", "Cắt", "cut", "Ctrl+X", i >= 0, false),
                menu("rename", "Đổi tên", "rename", "F2", i >= 0, false),
                menu("delete", "Xoá", "delete", "Del", i >= 0, false),
                menu("delete-permanent", "Xoá vĩnh viễn", "delete", "Shift+Del", i >= 0, false),
                menu("", "", "", "", false, true),
                menu("copy-path", "Sao chép đường dẫn", "link", "Ctrl+Shift+C", i >= 0, false),
                menu("", "", "", "", false, true),
                // Thẻ màu: đổi màu icon của tệp hoặc thư mục.
                menu("", "Thẻ màu", "", "", i >= 0, false),
                menu_tag(&tag_list[0], i >= 0 && attached[0]),
                menu_tag(&tag_list[1], i >= 0 && attached[1]),
                menu_tag(&tag_list[2], i >= 0 && attached[2]),
                menu_tag(&tag_list[3], i >= 0 && attached[3]),
                menu("tag-clear", "Bỏ thẻ", "dismiss", "", i >= 0 && attached.iter().any(|x| *x), false),
                menu("", "", "", "", false, true),
                menu("compress", "Nén thành ZIP", "folder_zip", "", i >= 0, false),
                menu("extract", "Giải nén", "folder_open", "", is_archive, false),
                menu("", "", "", "", false, true),
                menu("hash-sha256", "Mã băm SHA-256", "shield", "", i >= 0 && !is_dir, false),
                menu("hash-md5", "Mã băm MD5", "shield", "", i >= 0 && !is_dir, false),
                menu("", "", "", "", false, true),
                menu("folder-size", "Tính kích thước", "calculator", "", is_dir, false),
                menu("properties", "Thuộc tính", "info", "", i >= 0, false),
            ];
            ui.set_menu_items(ModelRc::new(VecModel::from(items)));
            ui.set_menu_x(x);
            ui.set_menu_y(y);
            ui.set_menu_open(true);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_background_context_menu(move |x, y| {
            let Some(ui) = w.upgrade() else { return };
            let can_paste = !a.borrow().clipboard.is_empty();
            let items = vec![
                menu("new-folder", "Thư mục mới", "folder", "Ctrl+N", true, false),
                menu("new-file", "Tệp mới", "document", "", true, false),
                menu("", "", "", "", false, true),
                menu("paste", "Dán", "clipboard_paste", "Ctrl+V", can_paste, false),
                menu("select-all", "Chọn tất cả", "checkmark", "Ctrl+A", true, false),
                menu("", "", "", "", false, true),
                menu("refresh", "Làm mới", "arrow_clockwise", "F5", true, false),
                menu("open-terminal", "Mở Terminal tại đây", "code", "", true, false),
                menu("", "", "", "", false, true),
                menu("toggle-extensions", "Ẩn/hiện phần mở rộng", "document", "", true, false),
                menu("toggle-folders-first", "Thư mục trước tệp", "arrow_sort", "", true, false),
                menu("toggle-hidden", if a.borrow().show_hidden { "Ẩn mục ẩn" } else { "Hiện mục ẩn" }, "eye", "Ctrl+H", true, false),
            ];
            ui.set_menu_items(ModelRc::new(VecModel::from(items)));
            ui.set_menu_x(x);
            ui.set_menu_y(y);
            ui.set_menu_open(true);
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
