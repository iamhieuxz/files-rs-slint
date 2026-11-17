// Không mở cửa sổ console: app gốc không có console. Thuộc tính phải đứng
// trước mọi item khác, nên đặt ngay đầu tệp.
#![cfg_attr(windows, windows_subsystem = "windows")]
// Code production không được `unwrap`/`expect`: lỗi phải đi qua `Result`.
// Test được phép dùng (cfg(test) không bị ảnh hưởng).
// Mã do Slint sinh ra nằm ngoài lint này: xem `allow` trên `include_modules`.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
// Code production không được `unwrap`/`expect`: lỗi phải đi qua `Result` và
// hiển thị cho người dùng. Test được phép dùng (cfg(test) không bị ảnh hưởng).
// Mã do Slint sinh ra dùng `unwrap` nội bộ; nằm ngoài lint của mình.
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod slint_generated {
    slint::include_modules!();
}
use slint_generated::*;

mod dialogs;
mod file_ops;
mod format;
mod history;
mod logging;
mod ops_worker;
mod preview;
mod settings;
mod selection;
mod state;
mod tabs;
mod navigation;
mod view;
mod tags;
mod window;

use files_core::{SortBy, SortDirection, SortSpec};
use dialogs::{ask_delete, ask_delete_permanent, ask_new, ask_rename, confirm_dialog, show_new_menu, show_properties};
use file_ops::{
    compress_selection, copy_selected_path, do_paste, extract_selection, hash_selection,
    open_terminal_here, set_clipboard, view_folder_size,
};
use navigation::{apply_listing, build_sidebar, navigate_to, open_row, reload};
use selection::{first_selected_visible, move_selection, select_row};
use state::{apply_saved_ui, persist_ui, settings_path, tag_store_path, App, Tab};
use tabs::{
    close_all_tabs, close_other_tabs, close_tab_at, close_tabs_side, cycle_tab, duplicate_tab,
    focus_menu_tab, menu_tab, reopen_closed_tab, sync_tabs,
};
use view::{
    apply_shell_icon, fill_size_cell, push_status, push_view, refresh_theme, to_slint_image,
};
use ops_worker::{JobDone, Worker};
use slint::{ModelRc, SharedString, Timer, TimerMode, VecModel};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

fn main() -> Result<(), slint::PlatformError> {
    // Không có console nên stderr mất hút; ghi log ra tệp để còn chẩn đoán.
    logging::install_panic_hook();
    // `files-fs` không tự biết ghi log ở đâu; cài sink để lỗi Win32 không mất.
    files_fs::set_log_sink(logging::write);
    logging::write(&format!(
        "khởi động, log tại {:?}",
        logging::log_path()
    ));

    let ui = AppWindow::new()?;
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\"));

    // Nạp giao diện lần trước trước khi dựng App, để mọi thứ nhất quán ngay
    // từ khung hình đầu tiên thay vì chớp sang nền sáng rồi mới đổi.
    let settings_path = settings_path();
    let saved = settings::Settings::load(&settings_path);

    // Không tạo được luồng nền thì app không thể hoạt động: báo lỗi và thoát
    // bằng `?`, không panic.
    let (worker, results) = Worker::spawn().map_err(|e| {
        slint::PlatformError::Other(format!("Không tạo được luồng nền: {e}"))
    })?;
    let app = Rc::new(RefCell::new(App {
        tabs: vec![Tab::new(home.clone())],
        active: 0,
        show_hidden: saved.show_hidden,
        show_sidebar: saved.show_sidebar,
        show_preview: saved.show_preview,
        view_mode: saved.view_mode,
        dark: saved.dark,
        clipboard: Vec::new(),
        clipboard_cut: false,
        load_token: 0,
        load_in_flight: false,
        grid_width: 900.0,
        worker,
        results,
        watch: None,
        watched_path: None,
        closed_tabs: Vec::new(),
        show_extensions: saved.show_extensions,
        folders_first: saved.folders_first,
        tags: {
            let path = tag_store_path();
            tags::TagStore::load(&path)
        },
        tag_path: tag_store_path(),
        settings: saved,
        settings_path,
    }));

    build_sidebar(&ui);
    // Áp giao diện đã lưu TRƯỚC khung hình đầu tiên, để không bị chớp sáng
    // rồi mới tối.
    apply_saved_ui(&ui, &app.borrow());
    reload(&ui, &app);
    sync_tabs(&ui, &app.borrow());

    wire_callbacks(&ui, &app);
    install_result_pump(&ui, &app);
    install_watch_pump(&ui, &app);

    // Cửa sổ không khung: nút thu nhỏ/phóng/đóng do app tự vẽ, nên phải tự
    // chuyển chúng thành lệnh Win32.
    window::wire(&ui);
    window::install_state_poller(&ui);

    ui.run()
}

// ---------------------------------------------------------------- sidebar

fn wire_callbacks(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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

/// Build the palette model for `query`.
fn palette_model(query: &str) -> ModelRc<PaletteEntry> {
    let items: Vec<PaletteEntry> = filter_palette(query)
        .into_iter()
        .map(|c| PaletteEntry {
            id: SharedString::from(c.id),
            label: SharedString::from(c.label),
            icon: SharedString::from(c.icon),
            shortcut: SharedString::from(c.shortcut),
        })
        .collect();
    ModelRc::new(VecModel::from(items))
}

fn menu(
    id: &str,
    label: &str,
    icon: &str,
    shortcut: &str,
    enabled: bool,
    separator: bool,
) -> MenuItem {
    MenuItem {
        id: SharedString::from(id),
        label: SharedString::from(label),
        dot: slint::Color::from_argb_u8(0, 0, 0, 0),
        has_dot: false,
        icon: SharedString::from(icon),
        shortcut: SharedString::from(shortcut),
        enabled,
        separator,
    }
}

// ---------------------------------------------------------------- actions

fn run_command(ui: &AppWindow, app: &Rc<RefCell<App>>, id: &str) {
    // Mục thẻ mang uid trong mã lệnh, nên phải bắt trước khi so khớp chuỗi.
    if let Some(uid) = id.strip_prefix("tag-set:") {
        set_tag_on_selection(ui, app, Some(uid));
        return;
    }
    match id {
        "tag-clear" => set_tag_on_selection(ui, app, None),
        "refresh" => reload(ui, app),
        "copy" => set_clipboard(ui, app, false),
        "cut" => set_clipboard(ui, app, true),
        "paste" => do_paste(ui, app),
        "delete" => ask_delete(ui, app),
        "rename" => ask_rename(ui, app),
        "new-folder" => ask_new(ui, "new-folder"),
        "new-file" => ask_new(ui, "new-file"),
        "new-menu" => show_new_menu(ui, 12.0, 40.0),
        "select-all" => {
            {
                let mut s = app.borrow_mut();
                let active = s.active;
                let tab = &mut s.tabs[active];
                tab.selection = tab.visible.clone();
            }
            push_view(ui, app);
            push_status(ui, app);
        }
        "clear-selection" => {
            {
                let mut s = app.borrow_mut();
                let active = s.active;
                s.tabs[active].selection.clear();
            }
            push_view(ui, app);
            push_status(ui, app);
            preview::clear(ui);
        }
        "toggle-hidden" => {
            let v = !app.borrow().show_hidden;
            app.borrow_mut().show_hidden = v;
            ui.set_show_hidden(v);
            reload(ui, app);
        }
        "open-selected" => {
            let first = app.borrow().tab().visible.iter().position(|i| {
                app.borrow().tab().selection.contains(i)
            });
            if let Some(i) = first {
                open_row(ui, app, i);
            }
        }
        "open" => {
            if let Some(i) = first_selected_visible(app) {
                open_row(ui, app, i);
            }
        }
        "properties" => show_properties(ui, app),
        "compress" => compress_selection(ui, app),
        "extract" => extract_selection(ui, app),
        "hash-sha256" => hash_selection(ui, app, files_fs::HashKind::Sha256),
        "hash-md5" => hash_selection(ui, app, files_fs::HashKind::Md5),
        "hash-sha1" => hash_selection(ui, app, files_fs::HashKind::Sha1),
        "folder-size" => view_folder_size(ui, app),
        // Mở thư mục đang chọn trong một tab mới, giữ tab hiện tại nguyên vẹn.
        "open-new-tab" => {
            let target = {
                let s = app.borrow();
                s.tab().selected_entries().first().map(|e| (e.is_dir(), PathBuf::from(&e.path)))
            };
            if let Some((true, path)) = target {
                {
                    let mut s = app.borrow_mut();
                    s.tabs.push(Tab::new(path));
                    s.active = s.tabs.len() - 1;
                }
                sync_tabs(ui, &app.borrow());
                reload(ui, app);
            } else if let Some((false, path)) = target {
                if let Err(e) = open::that_detached(&path) {
                    ui.set_error_text(SharedString::from(
                        format!("Không mở được tệp: {e}").as_str(),
                    ));
                }
            }
        }
        "nav-up" | "nav-down" => move_selection(ui, app, id == "nav-down"),
        // ---- tệp: xoá vĩnh viễn, sao chép đường dẫn, terminal ----
        "delete-permanent" => ask_delete_permanent(ui, app),
        "copy-path" => copy_selected_path(ui, app),
        "open-terminal" => open_terminal_here(ui, app),
        "register-assoc" => ask_register_association(ui),
        "open-default-apps" => open_default_apps(ui),
        // ---- hiển thị ----
        "toggle-extensions" => {
            let v = !app.borrow().show_extensions;
            app.borrow_mut().show_extensions = v;
            persist_ui(app);
            push_view(ui, app);
            ui.set_status_tertiary(SharedString::from(
                if v { "Đang hiện phần mở rộng" } else { "Đang ẩn phần mở rộng" },
            ));
        }
        "toggle-folders-first" => {
            let v = !app.borrow().folders_first;
            app.borrow_mut().folders_first = v;
            persist_ui(app);
            // Sắp xếp lại ngay, không cần đọc lại thư mục.
            {
                let mut s = app.borrow_mut();
                let active = s.active;
                let tab = &mut s.tabs[active];
                tab.sort.folders_first = v;
                let spec = tab.sort;
                spec.sort(&mut tab.entries);
                tab.rebuild_visible();
            }
            push_view(ui, app);
            ui.set_status_tertiary(SharedString::from(
                if v { "Thư mục trước tệp" } else { "Sắp xếp chung" },
            ));
        }
        // ---- tab nâng cao ----
        "reopen-tab" => reopen_closed_tab(ui, app),
        "duplicate-tab" => duplicate_tab(ui, app),
        "next-tab" => cycle_tab(ui, app, 1),
        "prev-tab" => cycle_tab(ui, app, -1),
        "close-other-tabs" | "tab-close-others" => {
            focus_menu_tab(ui, app);
            close_other_tabs(ui, app)
        }
        "close-tabs-left" | "tab-close-left" => {
            focus_menu_tab(ui, app);
            close_tabs_side(ui, app, true)
        }
        "close-tabs-right" | "tab-close-right" => {
            focus_menu_tab(ui, app);
            close_tabs_side(ui, app, false)
        }
        "close-all-tabs" | "tab-close-all" => close_all_tabs(ui, app),
        "tab-close" => {
            let target = menu_tab(ui, app);
            close_tab_at(ui, app, target)
        }
        "tab-duplicate" => {
            focus_menu_tab(ui, app);
            duplicate_tab(ui, app)
        }
        _ => {}
    }
}

/// One entry in the command palette.
///
/// The palette is the discovery surface for everything the app can do: a user
/// who does not know the shortcut can still find the command by typing.
pub struct PaletteCommand {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub shortcut: &'static str,
}

pub const PALETTE: &[PaletteCommand] = &[
    PaletteCommand { id: "new-folder", label: "Thư mục mới", icon: "folder", shortcut: "Ctrl+Shift+N" },
    PaletteCommand { id: "new-file", label: "Tệp mới", icon: "document", shortcut: "" },
    PaletteCommand { id: "copy", label: "Sao chép", icon: "copy", shortcut: "Ctrl+C" },
    PaletteCommand { id: "cut", label: "Cắt", icon: "cut", shortcut: "Ctrl+X" },
    PaletteCommand { id: "paste", label: "Dán", icon: "clipboard_paste", shortcut: "Ctrl+V" },
    PaletteCommand { id: "rename", label: "Đổi tên", icon: "rename", shortcut: "F2" },
    PaletteCommand { id: "delete", label: "Xoá vào Thùng rác", icon: "delete", shortcut: "Del" },
    PaletteCommand { id: "delete-permanent", label: "Xoá vĩnh viễn", icon: "delete", shortcut: "Shift+Del" },
    PaletteCommand { id: "copy-path", label: "Sao chép đường dẫn", icon: "link", shortcut: "Ctrl+Shift+C" },
    PaletteCommand { id: "compress", label: "Nén thành ZIP", icon: "folder_zip", shortcut: "" },
    PaletteCommand { id: "extract", label: "Giải nén", icon: "folder_open", shortcut: "" },
    PaletteCommand { id: "folder-size", label: "Tính kích thước thư mục", icon: "calculator", shortcut: "" },
    PaletteCommand { id: "open-terminal", label: "Mở Terminal tại đây", icon: "code", shortcut: "" },
    PaletteCommand { id: "toggle-hidden", label: "Ẩn/hiện mục ẩn", icon: "eye", shortcut: "Ctrl+H" },
    PaletteCommand { id: "toggle-extensions", label: "Ẩn/hiện phần mở rộng", icon: "document", shortcut: "" },
    PaletteCommand { id: "toggle-folders-first", label: "Thư mục trước tệp", icon: "arrow_sort", shortcut: "" },
    PaletteCommand { id: "toggle-sidebar", label: "Ẩn/hiện sidebar", icon: "panel_left", shortcut: "" },
    PaletteCommand { id: "toggle-preview", label: "Ẩn/hiện panel xem trước", icon: "panel_right", shortcut: "" },
    PaletteCommand { id: "view-details", label: "Xem dạng danh sách", icon: "list", shortcut: "" },
    PaletteCommand { id: "view-grid", label: "Xem dạng lưới", icon: "grid", shortcut: "" },
    PaletteCommand { id: "reopen-tab", label: "Mở lại tab đã đóng", icon: "window", shortcut: "Ctrl+Shift+T" },
    PaletteCommand { id: "duplicate-tab", label: "Nhân bản tab", icon: "window", shortcut: "Ctrl+D" },
    PaletteCommand { id: "next-tab", label: "Tab kế tiếp", icon: "chevron_right", shortcut: "Ctrl+Tab" },
    PaletteCommand { id: "prev-tab", label: "Tab trước", icon: "chevron_left", shortcut: "Ctrl+Shift+Tab" },
    PaletteCommand { id: "close-other-tabs", label: "Đóng các tab khác", icon: "dismiss", shortcut: "" },
    PaletteCommand { id: "close-all-tabs", label: "Đóng tất cả tab", icon: "dismiss", shortcut: "" },
    PaletteCommand { id: "refresh", label: "Làm mới", icon: "arrow_clockwise", shortcut: "F5" },
    PaletteCommand { id: "go-up", label: "Lên một cấp", icon: "arrow_upload", shortcut: "Backspace" },
    PaletteCommand { id: "toggle-theme", label: "Đổi sáng/tối", icon: "weather_moon", shortcut: "" },
    PaletteCommand { id: "tag-set:f7e0e137-2eb5-4fa4-a50d-ddd65df17c34", label: "Thẻ: Home (xanh dương)", icon: "tag", shortcut: "" },
    PaletteCommand { id: "tag-set:c84a8131-c4de-47d9-9440-26e859d14b3d", label: "Thẻ: Work (cam)", icon: "tag", shortcut: "" },
    PaletteCommand { id: "tag-set:d4b8d4bd-ceaf-4e58-ac61-a185fcf96c5d", label: "Thẻ: Photos (vàng)", icon: "tag", shortcut: "" },
    PaletteCommand { id: "tag-set:79376daf-c44a-4fe4-aa3b-8b30baea453e", label: "Thẻ: Important (xanh lá)", icon: "tag", shortcut: "" },
    PaletteCommand { id: "tag-clear", label: "Bỏ thẻ màu", icon: "dismiss", shortcut: "" },
    PaletteCommand { id: "register-assoc", label: "Đặt làm trình duyệt tệp mặc định", icon: "link", shortcut: "" },
    PaletteCommand { id: "open-default-apps", label: "Mở Settings ứng dụng mặc định", icon: "settings", shortcut: "" },
];

/// Commands matching `query`, case-insensitive, in palette order.
///
/// An empty query returns everything, which is what the palette shows on open:
/// a browsable list beats an empty box.
pub fn filter_palette(query: &str) -> Vec<&'static PaletteCommand> {
    let q = query.trim().to_lowercase();
    PALETTE
        .iter()
        .filter(|c| q.is_empty() || c.label.to_lowercase().contains(&q))
        .collect()
}

// ---------------------------------------------------------------- file commands

/// Đăng ký app làm lựa chọn mở thư mục, rồi mở Settings để người dùng chọn.
///
/// Windows không cho đặt mặc định bằng code; bước này chỉ làm app xuất hiện
/// trong danh sách, còn quyết định cuối cùng là của người dùng.
/// Hỏi trước khi đăng ký: đây là thay đổi có thể thấy được trên toàn hệ thống.
fn ask_register_association(ui: &AppWindow) {
    ui.set_dialog_kind(SharedString::from("confirm-default"));
    ui.set_dialog_title(SharedString::from("Đặt làm trình duyệt tệp mặc định"));
    ui.set_dialog_text(SharedString::from(
        "Files RS sẽ được thêm vào danh sách ứng dụng mở thư mục. \
         Windows sẽ mở trang Settings để bạn chọn — chỉ cần bấm một lần ở đó. \
         Bạn có thể gỡ bỏ bất cứ lúc nào.",
    ));
    ui.set_dialog_primary_label(SharedString::from("Đăng ký"));
    ui.set_dialog_has_input(false);
    ui.set_dialog_allows_empty(false);
    ui.set_dialog_value(SharedString::default());
    ui.set_dialog_open(true);
}

/// Thực hiện đăng ký sau khi người dùng đã xác nhận.
fn register_association(ui: &AppWindow) {
    let exe = std::env::current_exe().unwrap_or_default();
    match files_fs::register_file_assoc(&exe) {
        Ok(()) => {
            ui.set_status_tertiary(SharedString::from(
                "Đã đăng ký. Chọn Files RS trong trang Settings vừa mở.",
            ));
            if let Err(e) = files_fs::open_settings_page() {
                logging::write(&format!("không mở được trang Settings: {}", e.message));
            }
        }
        Err(e) => ui.set_error_text(SharedString::from(e.message.as_str())),
    }
}

fn open_default_apps(ui: &AppWindow) {
    if let Err(e) = files_fs::open_settings_page() {
        ui.set_error_text(SharedString::from(e.message.as_str()));
    }
}

/// Apply `uid` to every selected item, or clear all tags when `None`.
///
/// Tags are stored per path and saved immediately: a crash or a kill must not
/// lose the colour the user just chose.
fn set_tag_on_selection(ui: &AppWindow, app: &Rc<RefCell<App>>, uid: Option<&str>) {
    let paths = app.borrow().tab().selected_paths();
    if paths.is_empty() {
        return;
    }
    let mut failed_save = None;
    let (name, changed) = {
        let mut s = app.borrow_mut();
        let base = uid
            .and_then(|u| s.tags.tag(u).map(|t| t.name.clone()))
            .unwrap_or_else(|| "Bỏ thẻ".to_string());
        let name = base;
        // Bấm lại một thẻ đã gắn thì gỡ nó ra: đó là cách tự nhiên nhất để
        // tắt màu mà không phải nhớ "Bỏ thẻ".
        let all_tagged = uid.is_some_and(|u| {
            paths
                .iter()
                .all(|p| s.tags.tags_for(p).iter().any(|t| t.uid == u))
        });
        for path in &paths {
            match uid {
                Some(u) if all_tagged => s.tags.detach(path, u),
                Some(u) => s.tags.attach(path, u),
                None => s.tags.clear(path),
            }
        }
        let path = s.tag_path.clone();
        let store = s.tags.clone();
        drop(s);
        if let Err(e) = store.save(&path) {
            failed_save = Some(e);
        }
        (name, paths.len())
    };

    if let Some(e) = failed_save {
        logging::write(&format!("không lưu được thẻ: {e}"));
        let text = format!("Không lưu được thẻ: {e}");
        ui.set_error_text(SharedString::from(text.as_str()));
    } else {
        ui.set_status_tertiary(SharedString::from(
            format!("{name}: {changed} mục").as_str(),
        ));
    }
    // Vẽ lại để icon đổi màu ngay.
    push_view(ui, app);
}

/// A menu row carrying a colour swatch, used for the tag list.
fn menu_tag(tag: &tags::Tag, attached: bool) -> MenuItem {
    // Mã lệnh mang uid để bấm vào là biết gắn thẻ nào.
    let id = format!("tag-set:{}", tag.uid);
    MenuItem {
        id: SharedString::from(id.as_str()),
        // Dấu ✓ cho thẻ đã gắn, để bấm lần nữa là biết trạng thái hiện tại.
        label: SharedString::from(if attached {
            format!("{}  ✓", tag.name)
        } else {
            tag.name.clone()
        }),
        icon: SharedString::default(),
        shortcut: SharedString::default(),
        enabled: true,
        separator: false,
        dot: slint::Color::from_argb_u8(
            255,
            (tags::parse_hex(&tag.color).unwrap_or(0x808080) >> 16) as u8,
            ((tags::parse_hex(&tag.color).unwrap_or(0x808080) >> 8) & 0xFF) as u8,
            (tags::parse_hex(&tag.color).unwrap_or(0x808080) & 0xFF) as u8,
        ),
        has_dot: true,
    }
}

/// Drain finished background work on a short timer.
///
/// `invoke_from_event_loop` would be simpler but cannot be called from a thread
/// that outlives the component; a timer keeps the handoff inside the event loop
/// and makes shutdown deterministic.
fn install_result_pump(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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
                Ok(JobDone::OpDone { label, result }) => {
                    ui.set_busy(false);
                    ui.set_busy_text(SharedString::default());
                    match result {
                        Ok(()) => {
                            ui.set_status_tertiary(SharedString::from(label.as_str()));
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
fn install_watch_pump(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ops_worker::Job;

    #[test]
    fn an_empty_palette_query_lists_every_command() {
        // The palette must be browsable on open: an empty box teaches nothing.
        assert_eq!(filter_palette("").len(), PALETTE.len());
    }

    #[test]
    fn a_query_filters_case_insensitively() {
        let hits = filter_palette("ĐỔI TÊN");
        assert!(
            hits.iter().any(|c| c.id == "rename"),
            "Vietnamese is case-folded too"
        );
    }

    #[test]
    fn a_query_with_no_match_returns_nothing() {
        assert!(filter_palette("zzzznotacommand").is_empty());
    }

    #[test]
    fn every_palette_id_is_unique() {
        // Duplicate ids would make two entries run the same command, which is
        // how a palette silently rots.
        let mut ids: Vec<&str> = PALETTE.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate command id in PALETTE");
    }

    /// Scratch directory next to the test binary.
    ///
    /// `CARGO_TARGET_TMPDIR` only exists for integration tests in `tests/`, not
    /// for unit tests inside a binary, and `%TEMP%` is not writable on every
    /// machine. Sitting beside the exe keeps the test self-contained.
    fn scratch(name: &str) -> PathBuf {
        let exe = std::env::current_exe().expect("đường dẫn exe");
        let base = exe.parent().expect("thư mục chứa exe").join("test-scratch");
        let dir = base.join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tạo thư mục tạm");
        dir
    }

    #[test]
    fn creating_and_renaming_through_the_worker_reaches_disk() {
        // Đúng đường đi của hộp thoại: dialog gửi Job, worker chạy, đĩa đổi.
        // Test này bắt được lỗi "bấm Tạo mà không có gì xảy ra".
        let root = scratch("worker-create");
        let (worker, rx) = Worker::spawn().expect("tạo luồng worker cho test");

        worker.submit(Job::CreateFolder {
            parent: root.clone(),
            name: "tài liệu".into(),
        });
        match rx.recv_timeout(Duration::from_secs(10)).expect("worker phải trả lời") {
            JobDone::OpDone { result, .. } => result.expect("tạo thư mục phải thành công"),
            _ => panic!("cần OpDone"),
        }
        let created = root.join("tài liệu");
        assert!(created.is_dir(), "thư mục phải có trên đĩa");

        worker.submit(Job::Rename {
            from: created.clone(),
            to: "đã đổi tên".into(),
        });
        match rx.recv_timeout(Duration::from_secs(10)).expect("worker phải trả lời") {
            JobDone::OpDone { result, .. } => result.expect("đổi tên phải thành công"),
            _ => panic!("cần OpDone"),
        }
        assert!(root.join("đã đổi tên").is_dir(), "tên mới phải tồn tại");
        assert!(!created.exists(), "tên cũ phải biến mất");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_name_still_creates_a_folder() {
        // Hộp thoại cho bấm "Tạo" khi ô trống; Rust tự đặt tên.
        let root = scratch("worker-empty-name");
        let (worker, rx) = Worker::spawn().expect("tạo luồng worker cho test");
        worker.submit(Job::CreateFolder {
            parent: root.clone(),
            name: String::new(),
        });
        match rx.recv_timeout(Duration::from_secs(10)).expect("worker phải trả lời") {
            JobDone::OpDone { result, .. } => result.expect("tạo với tên rỗng phải thành công"),
            _ => panic!("cần OpDone"),
        }
        let count = std::fs::read_dir(&root).unwrap().count();
        assert_eq!(count, 1, "phải có đúng một thư mục được tạo");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn icon_work_never_blocks_file_operations() {
        // Đây là hồi quy cho lỗi thật: icon và thao tác tệp từng chung một
        // luồng, nên một icon chậm làm mọi lệnh tạo/đổi tên sau đó đứng im.
        // Xếp 300 icon trước, rồi gửi lệnh tạo: lệnh tạo vẫn phải xong nhanh.
        let root = scratch("worker-not-blocked");
        let (worker, rx) = Worker::spawn().expect("tạo luồng worker cho test");

        let exe = std::env::current_exe().expect("đường dẫn exe");
        for _ in 0..300 {
            worker.submit_icon(exe.clone(), 20, 1);
        }
        worker.submit(Job::CreateFolder {
            parent: root.clone(),
            name: "sau-icon".into(),
        });

        let start = std::time::Instant::now();
        // Bỏ qua các kết quả icon cho tới khi gặp OpDone.
        loop {
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(JobDone::OpDone { result, .. }) => {
                    result.expect("tạo thư mục phải thành công");
                    break;
                }
                Ok(_) => continue,
                Err(e) => panic!("worker không trả lời: {e}"),
            }
        }
        let elapsed = start.elapsed();
        assert!(
            root.join("sau-icon").is_dir(),
            "thư mục phải được tạo dù có 300 icon xếp trước"
        );
        // Ngưỡng rộng: mục tiêu là bắt được việc bị chặn hẳn, không phải đo hiệu năng.
        assert!(
            elapsed < Duration::from_secs(5),
            "lệnh tạo bị chặn bởi icon: mất {elapsed:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }








}
