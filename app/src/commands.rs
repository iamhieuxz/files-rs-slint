//! The command table and the dispatcher.
//!
//! One list of commands feeds both the Command Palette and the context menus,
//! so a command cannot exist in one and be forgotten in the other. `run_command`
//! is the single entry point: the palette, the menu and the keyboard all call it.

use crate::dialogs::{
    ask_delete, ask_delete_permanent, ask_new, ask_rename, show_new_menu, show_properties,
};
use crate::file_ops::{
    ask_paste, compress_selection, copy_selected_path, extract_selection, hash_selection,
    open_terminal_here, redo_last, set_clipboard, undo_last, view_folder_size,
};
use crate::navigation::{open_row, reload, reload_all};
use crate::selection::first_selected_visible;
use crate::selection::move_selection;
use crate::slint_generated::*;
use crate::state::{persist_pins, persist_ui, App, Pane, Tab};
use crate::tabs::{
    close_all_tabs, close_other_tabs, close_tab_at, close_tabs_side, cycle_tab, duplicate_tab,
    focus_menu_tab, menu_tab, reopen_closed_tab, sync_tabs,
};
use crate::view::{push_all, push_status, push_view, refresh_theme};
use crate::{logging, panes, preview, tags};
use slint::{ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Build the palette model for `query`.
pub fn palette_model(query: &str) -> ModelRc<PaletteEntry> {
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

pub fn menu(
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
        header: false,
        submenu: false,
    }
}

/// Mục mở menu con, ví dụ "Mở bằng".
pub fn menu_submenu(id: &str, label: &str, icon: &str) -> MenuItem {
    MenuItem {
        id: SharedString::from(id),
        label: SharedString::from(label),
        dot: slint::Color::from_argb_u8(0, 0, 0, 0),
        has_dot: false,
        icon: SharedString::from(icon),
        shortcut: SharedString::default(),
        enabled: true,
        separator: false,
        header: false,
        submenu: true,
    }
}

/// Nhãn nhóm trong menu, ví dụ "Thẻ màu" hay "Mở bằng".
pub fn menu_header(label: &str) -> MenuItem {
    MenuItem {
        id: SharedString::default(),
        label: SharedString::from(label),
        dot: slint::Color::from_argb_u8(0, 0, 0, 0),
        has_dot: false,
        icon: SharedString::default(),
        shortcut: SharedString::default(),
        enabled: false,
        separator: false,
        header: true,
        submenu: false,
    }
}

// ---------------------------------------------------------------- actions

/// Every id `run_command` knows how to execute.
///
/// The palette is the discovery surface for the app, so a palette entry with
/// no branch here is a button that silently does nothing. Keeping the list next
/// to the dispatcher lets a unit test catch that class of bug.
pub const IMPLEMENTED_COMMANDS: &[&str] = &[
    "clear-selection",
    "close-all-tabs",
    "close-other-tabs",
    "close-tabs-left",
    "close-tabs-right",
    "compress",
    "copy",
    "copy-path",
    "cut",
    "delete",
    "delete-permanent",
    "duplicate-tab",
    "empty-recycle-bin",
    "extract",
    "folder-size",
    "go-up",
    "hash-md5",
    "hash-sha1",
    "hash-sha256",
    "nav-down",
    "nav-up",
    "new-file",
    "new-folder",
    "new-menu",
    "new-window",
    "next-tab",
    "open",
    "open-default-apps",
    "open-new-tab",
    "open-selected",
    "open-terminal",
    "open-with-dialog",
    "paste",
    "pin-folder",
    "prev-tab",
    "properties",
    "redo",
    "refresh",
    "register-assoc",
    "rename",
    "reopen-tab",
    "rotate-cw",
    "search-recursive",
    "select-all",
    "tab-close",
    "tab-close-all",
    "tab-close-left",
    "tab-close-others",
    "tab-close-right",
    "tab-duplicate",
    "tag-clear",
    "toggle-dual-pane",
    "toggle-extensions",
    "toggle-folders-first",
    "toggle-hidden",
    "toggle-preview",
    "toggle-sidebar",
    "toggle-theme",
    "undo",
    "view-details",
    "view-grid",
];

/// Shared implementation for the button, the palette and the context menu.
///
/// Every entry point must go through one function: the old duplicated paths had
/// the palette change state without updating the checkbox or saving it.
pub fn toggle_sidebar(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let v = !app.borrow().show_sidebar;
    app.borrow_mut().show_sidebar = v;
    ui.set_show_sidebar(v);
    persist_ui(app);
}

pub fn toggle_preview(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let v = !app.borrow().show_preview;
    app.borrow_mut().show_preview = v;
    ui.set_show_preview(v);
    persist_ui(app);
}

pub fn toggle_hidden(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let v = !app.borrow().show_hidden;
    app.borrow_mut().show_hidden = v;
    ui.set_show_hidden(v);
    persist_ui(app);
    reload_all(ui, app);
}

pub fn set_view_mode(ui: &AppWindow, app: &Rc<RefCell<App>>, mode: i32) {
    app.borrow_mut().view_mode = mode;
    ui.set_view_mode(mode);
    persist_ui(app);
    // Lưới được chia hàng theo bề rộng hiện tại; chuyển sang lưới phải dựng
    // lại model, nếu không sẽ dùng cách chia của lần vẽ trước.
    push_all(ui, app);
}

pub fn toggle_theme(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let dark = !app.borrow().dark;
    app.borrow_mut().dark = dark;
    refresh_theme(ui, dark);
    persist_ui(app);
}

pub fn run_command(ui: &AppWindow, app: &Rc<RefCell<App>>, id: &str) {
    // Mục thẻ mang uid trong mã lệnh, nên phải bắt trước khi so khớp chuỗi.
    if let Some(uid) = id.strip_prefix("tag-set:") {
        set_tag_on_selection(ui, app, Some(uid));
        return;
    }
    // "Mở bằng" mang chỉ số ứng dụng đã liệt kê khi mở menu.
    if let Some(index) = id.strip_prefix("open-with:") {
        open_with_handler(ui, app, index.parse().ok());
        return;
    }
    if !IMPLEMENTED_COMMANDS.contains(&id) {
        // Không im lặng: một lệnh palette không có nhánh là lỗi lập trình.
        logging::write(&format!("lệnh không được hỗ trợ: {id}"));
        debug_assert!(false, "command id chưa có nhánh xử lý: {id}");
        return;
    }
    match id {
        "tag-clear" => set_tag_on_selection(ui, app, None),
        "rotate-cw" => rotate_selection(ui, app),
        "new-window" => open_new_window(ui, app),
        "empty-recycle-bin" => ask_empty_recycle_bin(ui),
        "pin-folder" => toggle_pin(ui, app),
        "toggle-sidebar" => toggle_sidebar(ui, app),
        "toggle-preview" => toggle_preview(ui, app),
        "toggle-dual-pane" => panes::toggle_dual(ui, app),
        "toggle-theme" => toggle_theme(ui, app),
        "view-details" => set_view_mode(ui, app, 0),
        "view-grid" => set_view_mode(ui, app, 1),
        "go-up" => crate::navigation::go_up(ui, app),
        // Tìm đệ quy dùng chính nội dung ô lọc làm từ khoá.
        "search-recursive" => {
            let q = ui.get_filter_text().to_string();
            crate::file_ops::search_recursive(ui, app, &q)
        }
        "undo" => undo_last(ui, app),
        "redo" => redo_last(ui, app),
        "refresh" => reload(ui, app),
        "copy" => set_clipboard(ui, app, false),
        "cut" => set_clipboard(ui, app, true),
        "paste" => ask_paste(ui, app),
        "delete" => ask_delete(ui, app),
        "rename" => ask_rename(ui, app),
        "new-folder" => ask_new(ui, "new-folder"),
        "new-file" => ask_new(ui, "new-file"),
        "new-menu" => show_new_menu(ui, 12.0, 40.0),
        "select-all" => {
            {
                let mut s = app.borrow_mut();
                let tab = s.tab_mut();
                tab.selection = tab.visible.clone();
            }
            push_view(ui, app);
            push_status(ui, app);
        }
        "clear-selection" => {
            {
                let mut s = app.borrow_mut();
                s.tab_mut().selection.clear();
            }
            push_view(ui, app);
            push_status(ui, app);
            preview::clear(ui);
        }
        "toggle-hidden" => toggle_hidden(ui, app),
        "open-selected" => {
            let first = app
                .borrow()
                .tab()
                .visible
                .iter()
                .position(|i| app.borrow().tab().selection.contains(i));
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
                s.tab()
                    .selected_entries()
                    .first()
                    .map(|e| (e.is_dir(), PathBuf::from(&e.path)))
            };
            if let Some((true, path)) = target {
                {
                    let mut s = app.borrow_mut();
                    s.tabs.push(Tab::new(path));
                    s.active = s.tabs.len() - 1;
                    s.active_pane = Pane::Left;
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
        "open-with-dialog" => open_with_dialog_for_selection(ui, app),
        "register-assoc" => ask_register_association(ui),
        "open-default-apps" => open_default_apps(ui),
        // ---- hiển thị ----
        "toggle-extensions" => {
            let v = !app.borrow().show_extensions;
            app.borrow_mut().show_extensions = v;
            persist_ui(app);
            push_all(ui, app);
            ui.set_status_message(SharedString::from(if v {
                "Đang hiện phần mở rộng"
            } else {
                "Đang ẩn phần mở rộng"
            }));
        }
        "toggle-folders-first" => {
            let v = !app.borrow().folders_first;
            app.borrow_mut().folders_first = v;
            persist_ui(app);
            // Sắp xếp lại ngay, không cần đọc lại thư mục.
            {
                let mut s = app.borrow_mut();
                for tab in s.tabs.iter_mut() {
                    tab.sort.folders_first = v;
                    let spec = tab.sort;
                    spec.sort(&mut tab.entries);
                    tab.rebuild_visible();
                }
                if let Some(tab) = s.right.as_mut() {
                    tab.sort.folders_first = v;
                    let spec = tab.sort;
                    spec.sort(&mut tab.entries);
                    tab.rebuild_visible();
                }
            }
            push_all(ui, app);
            ui.set_status_message(SharedString::from(if v {
                "Thư mục trước tệp"
            } else {
                "Sắp xếp chung"
            }));
        }
        // ---- tab nâng cao ----
        "reopen-tab" => reopen_closed_tab(ui, app),
        "duplicate-tab" => duplicate_tab(ui, app),
        "next-tab" => cycle_tab(ui, app, 1),
        "prev-tab" => cycle_tab(ui, app, -1),
        "close-other-tabs" => close_other_tabs(ui, app),
        // Lệnh tab từ menu chuột phải nhắm tab đang mở menu; lệnh palette
        // cùng ý nghĩa phải dùng tab đang active, không đọc `menu-tab` cũ.
        "tab-close-others" => {
            focus_menu_tab(ui, app);
            close_other_tabs(ui, app)
        }
        "close-tabs-left" => close_tabs_side(ui, app, true),
        "tab-close-left" => {
            focus_menu_tab(ui, app);
            close_tabs_side(ui, app, true)
        }
        "close-tabs-right" => close_tabs_side(ui, app, false),
        "tab-close-right" => {
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
pub fn ask_register_association(ui: &AppWindow) {
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
pub fn register_association(ui: &AppWindow) {
    let exe = std::env::current_exe().unwrap_or_default();
    match files_fs::register_file_assoc(&exe) {
        Ok(()) => {
            ui.set_status_message(SharedString::from(
                "Đã đăng ký. Chọn Files RS trong trang Settings vừa mở.",
            ));
            if let Err(e) = files_fs::open_settings_page() {
                logging::write(&format!("không mở được trang Settings: {}", e.message));
            }
        }
        Err(e) => ui.set_error_text(SharedString::from(e.message.as_str())),
    }
}

pub fn open_default_apps(ui: &AppWindow) {
    if let Err(e) = files_fs::open_settings_page() {
        ui.set_error_text(SharedString::from(e.message.as_str()));
    }
}

/// Tệp đầu tiên đang chọn, bỏ qua thư mục.
fn first_selected_file(app: &Rc<RefCell<App>>) -> Option<PathBuf> {
    app.borrow()
        .tab()
        .selected_entries()
        .iter()
        .find(|e| !e.is_dir())
        .map(|e| PathBuf::from(&e.path))
}

/// Mở tệp đang chọn bằng handler `index` đã liệt kê lúc mở menu.
fn open_with_handler(ui: &AppWindow, app: &Rc<RefCell<App>>, index: Option<usize>) {
    let Some(path) = first_selected_file(app) else {
        ui.set_error_text(SharedString::from("Chọn một tệp để mở bằng ứng dụng khác"));
        return;
    };
    let result = {
        let s = app.borrow();
        match index.and_then(|i| s.open_with.get(i)) {
            Some(handler) => handler.invoke(&path).map(|()| handler.name().to_string()),
            None => Err(files_core::FsError::new(
                "open_with_failed",
                "Ứng dụng không còn khả dụng",
            )),
        }
    };
    match result {
        Ok(name) => {
            ui.set_status_message(SharedString::from(format!("Đang mở bằng {name}…").as_str()))
        }
        Err(e) => ui.set_error_text(SharedString::from(e.message.as_str())),
    }
}

/// Hộp thoại "Open with" chuẩn của Windows cho tệp đang chọn.
fn open_with_dialog_for_selection(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let Some(path) = first_selected_file(app) else {
        ui.set_error_text(SharedString::from("Chọn một tệp để mở bằng ứng dụng khác"));
        return;
    };
    if let Err(e) = files_fs::open_with_dialog(&path) {
        ui.set_error_text(SharedString::from(e.message.as_str()));
    }
}

/// Apply `uid` to every selected item, or clear all tags when `None`.
///
/// Tags are stored per path and saved immediately: a crash or a kill must not
/// lose the colour the user just chose.
pub fn set_tag_on_selection(ui: &AppWindow, app: &Rc<RefCell<App>>, uid: Option<&str>) {
    let paths = app.borrow().tab().selected_paths();
    if paths.is_empty() {
        return;
    }
    let mut failed_save = None;
    let (name, changed, store, store_path) = {
        let mut s = app.borrow_mut();
        // Đọc bản mới nhất trên đĩa: cửa sổ khác có thể vừa gắn/bỏ thẻ, và
        // ghi đè bằng snapshot cũ sẽ làm mất thay đổi của họ.
        let mut store = tags::TagStore::load(&s.tag_path);
        let name = uid
            .and_then(|u| store.tag(u).map(|t| t.name.clone()))
            .unwrap_or_else(|| "Bỏ thẻ".to_string());
        // Bấm lại một thẻ đã gắn thì gỡ nó ra: đó là cách tự nhiên nhất để
        // tắt màu mà không phải nhớ "Bỏ thẻ".
        let all_tagged = uid.is_some_and(|u| {
            paths
                .iter()
                .all(|p| store.tags_for(p).iter().any(|t| t.uid == u))
        });
        for path in &paths {
            match uid {
                Some(u) if all_tagged => store.detach(path, u),
                Some(u) => store.attach(path, u),
                None => store.clear(path),
            }
        }
        s.tags = store.clone();
        let store_path = s.tag_path.clone();
        (name, paths.len(), store, store_path)
    };
    if let Err(e) = store.save(&store_path) {
        failed_save = Some(e);
    }

    if let Some(e) = failed_save {
        logging::write(&format!("không lưu được thẻ: {e}"));
        let text = format!("Không lưu được thẻ: {e}");
        ui.set_error_text(SharedString::from(text.as_str()));
    } else {
        ui.set_status_message(SharedString::from(
            format!("{name}: {changed} mục").as_str(),
        ));
    }
    // Vẽ lại để icon đổi màu ngay.
    push_view(ui, app);
}

/// A menu row carrying a colour swatch, used for the tag list.
pub fn tag_menu_items(tags: &[tags::Tag], attached: &[bool]) -> Vec<MenuItem> {
    tags.iter()
        .zip(attached)
        .map(|(tag, attached)| menu_tag(tag, *attached))
        .collect()
}

pub fn menu_tag(tag: &tags::Tag, attached: bool) -> MenuItem {
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
        header: false,
        submenu: false,
        dot: slint::Color::from_argb_u8(
            255,
            (tags::parse_hex(&tag.color).unwrap_or(0x808080) >> 16) as u8,
            ((tags::parse_hex(&tag.color).unwrap_or(0x808080) >> 8) & 0xFF) as u8,
            (tags::parse_hex(&tag.color).unwrap_or(0x808080) & 0xFF) as u8,
        ),
        has_dot: true,
    }
}

pub const PALETTE: &[PaletteCommand] = &[
    PaletteCommand {
        id: "new-folder",
        label: "Thư mục mới",
        icon: "folder",
        shortcut: "Ctrl+Shift+N",
    },
    PaletteCommand {
        id: "new-file",
        label: "Tệp mới",
        icon: "document",
        shortcut: "",
    },
    PaletteCommand {
        id: "new-window",
        label: "Cửa sổ mới",
        icon: "window",
        shortcut: "Ctrl+N",
    },
    PaletteCommand {
        id: "empty-recycle-bin",
        label: "Làm rỗng Thùng rác",
        icon: "delete",
        shortcut: "",
    },
    PaletteCommand {
        id: "rotate-cw",
        label: "Xoay ảnh 90° phải",
        icon: "arrow_clockwise",
        shortcut: "",
    },
    PaletteCommand {
        id: "search-recursive",
        label: "Tìm trong thư mục con",
        icon: "search",
        shortcut: "Ctrl+Shift+F",
    },
    PaletteCommand {
        id: "pin-folder",
        label: "Ghim hoặc bỏ ghim thư mục",
        icon: "pin",
        shortcut: "",
    },
    PaletteCommand {
        id: "undo",
        label: "Hoàn tác",
        icon: "arrow_undo",
        shortcut: "Ctrl+Z",
    },
    PaletteCommand {
        id: "redo",
        label: "Làm lại",
        icon: "arrow_redo",
        shortcut: "Ctrl+Y",
    },
    PaletteCommand {
        id: "copy",
        label: "Sao chép",
        icon: "copy",
        shortcut: "Ctrl+C",
    },
    PaletteCommand {
        id: "cut",
        label: "Cắt",
        icon: "cut",
        shortcut: "Ctrl+X",
    },
    PaletteCommand {
        id: "paste",
        label: "Dán",
        icon: "clipboard_paste",
        shortcut: "Ctrl+V",
    },
    PaletteCommand {
        id: "rename",
        label: "Đổi tên",
        icon: "rename",
        shortcut: "F2",
    },
    PaletteCommand {
        id: "delete",
        label: "Xoá vào Thùng rác",
        icon: "delete",
        shortcut: "Del",
    },
    PaletteCommand {
        id: "delete-permanent",
        label: "Xoá vĩnh viễn",
        icon: "delete",
        shortcut: "Shift+Del",
    },
    PaletteCommand {
        id: "copy-path",
        label: "Sao chép đường dẫn",
        icon: "link",
        shortcut: "Ctrl+Shift+C",
    },
    PaletteCommand {
        id: "compress",
        label: "Nén thành ZIP",
        icon: "folder_zip",
        shortcut: "",
    },
    PaletteCommand {
        id: "extract",
        label: "Giải nén",
        icon: "folder_open",
        shortcut: "",
    },
    PaletteCommand {
        id: "folder-size",
        label: "Tính kích thước thư mục",
        icon: "calculator",
        shortcut: "",
    },
    PaletteCommand {
        id: "open-terminal",
        label: "Mở Terminal tại đây",
        icon: "code",
        shortcut: "",
    },
    PaletteCommand {
        id: "open-with-dialog",
        label: "Mở bằng ứng dụng khác…",
        icon: "open",
        shortcut: "",
    },
    PaletteCommand {
        id: "toggle-hidden",
        label: "Ẩn/hiện mục ẩn",
        icon: "eye",
        shortcut: "Ctrl+H",
    },
    PaletteCommand {
        id: "toggle-extensions",
        label: "Ẩn/hiện phần mở rộng",
        icon: "document",
        shortcut: "",
    },
    PaletteCommand {
        id: "toggle-folders-first",
        label: "Thư mục trước tệp",
        icon: "arrow_sort",
        shortcut: "",
    },
    PaletteCommand {
        id: "toggle-sidebar",
        label: "Ẩn/hiện sidebar",
        icon: "panel_left",
        shortcut: "",
    },
    PaletteCommand {
        id: "toggle-preview",
        label: "Ẩn/hiện panel xem trước",
        icon: "panel_right",
        shortcut: "",
    },
    PaletteCommand {
        id: "toggle-dual-pane",
        label: "Bật/tắt hai khung",
        icon: "panel_split",
        shortcut: "",
    },
    PaletteCommand {
        id: "view-details",
        label: "Xem dạng danh sách",
        icon: "list",
        shortcut: "",
    },
    PaletteCommand {
        id: "view-grid",
        label: "Xem dạng lưới",
        icon: "grid",
        shortcut: "",
    },
    PaletteCommand {
        id: "reopen-tab",
        label: "Mở lại tab đã đóng",
        icon: "window",
        shortcut: "Ctrl+Shift+T",
    },
    PaletteCommand {
        id: "duplicate-tab",
        label: "Nhân bản tab",
        icon: "window",
        shortcut: "Ctrl+D",
    },
    PaletteCommand {
        id: "next-tab",
        label: "Tab kế tiếp",
        icon: "chevron_right",
        shortcut: "Ctrl+Tab",
    },
    PaletteCommand {
        id: "prev-tab",
        label: "Tab trước",
        icon: "chevron_left",
        shortcut: "Ctrl+Shift+Tab",
    },
    PaletteCommand {
        id: "close-other-tabs",
        label: "Đóng các tab khác",
        icon: "dismiss",
        shortcut: "",
    },
    PaletteCommand {
        id: "close-all-tabs",
        label: "Đóng tất cả tab",
        icon: "dismiss",
        shortcut: "",
    },
    PaletteCommand {
        id: "refresh",
        label: "Làm mới",
        icon: "arrow_clockwise",
        shortcut: "F5",
    },
    PaletteCommand {
        id: "go-up",
        label: "Lên một cấp",
        icon: "arrow_upload",
        shortcut: "Backspace",
    },
    PaletteCommand {
        id: "toggle-theme",
        label: "Đổi sáng/tối",
        icon: "weather_moon",
        shortcut: "",
    },
    PaletteCommand {
        id: "tag-set:f7e0e137-2eb5-4fa4-a50d-ddd65df17c34",
        label: "Thẻ: Home (xanh dương)",
        icon: "tag",
        shortcut: "",
    },
    PaletteCommand {
        id: "tag-set:c84a8131-c4de-47d9-9440-26e859d14b3d",
        label: "Thẻ: Work (cam)",
        icon: "tag",
        shortcut: "",
    },
    PaletteCommand {
        id: "tag-set:d4b8d4bd-ceaf-4e58-ac61-a185fcf96c5d",
        label: "Thẻ: Photos (vàng)",
        icon: "tag",
        shortcut: "",
    },
    PaletteCommand {
        id: "tag-set:79376daf-c44a-4fe4-aa3b-8b30baea453e",
        label: "Thẻ: Important (xanh lá)",
        icon: "tag",
        shortcut: "",
    },
    PaletteCommand {
        id: "tag-clear",
        label: "Bỏ thẻ màu",
        icon: "dismiss",
        shortcut: "",
    },
    PaletteCommand {
        id: "register-assoc",
        label: "Đặt làm trình duyệt tệp mặc định",
        icon: "link",
        shortcut: "",
    },
    PaletteCommand {
        id: "open-default-apps",
        label: "Mở Settings ứng dụng mặc định",
        icon: "settings",
        shortcut: "",
    },
];

/// Ghim thư mục đang mở vào sidebar, hoặc bỏ ghim nếu đã có.
///
/// Ghim nằm trong `settings.json` nên sống qua lần khởi động sau; danh sách
/// được ghi ngay để tắt app đột ngột không làm mất.
pub fn toggle_pin(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let settings_path = app.borrow().settings_path.clone();
    // Lấy danh sách ghim mới nhất từ đĩa trước khi sửa: cửa sổ khác có thể
    // vừa ghim thêm, và danh sách trong bộ nhớ của cửa sổ này đã cũ.
    let latest_pins = crate::settings::Settings::load(&settings_path).pinned_folders;
    let current = app.borrow().tab().current().to_string_lossy().into_owned();
    let now_pinned = {
        let mut a = app.borrow_mut();
        a.settings.pinned_folders = latest_pins;
        let list = &mut a.settings.pinned_folders;
        match list.iter().position(|p| p == &current) {
            Some(i) => {
                list.remove(i);
                false
            }
            None => {
                list.push(current.clone());
                true
            }
        }
    };
    // Ghi xuong dia truoc khi dung lai sidebar.
    persist_pins(app);
    let app_ref = app.borrow();
    crate::navigation::build_sidebar(ui, &app_ref);
    ui.set_status_message(SharedString::from(if now_pinned {
        "Đã ghim vào thanh bên"
    } else {
        "Đã bỏ ghim"
    }));
}

/// Xoay các ảnh đang chọn 90° sang phải.
///
/// Ghi đè lên tệp gốc nên không hoàn tác được — hộp thoại xác nhận nói rõ.
pub fn rotate_selection(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let paths: Vec<std::path::PathBuf> = app
        .borrow()
        .tab()
        .selected_paths()
        .into_iter()
        .filter(|p| {
            p.extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .is_some_and(|e| {
                    matches!(
                        e.as_str(),
                        "jpg" | "jpeg" | "png" | "bmp" | "gif" | "tif" | "tiff"
                    )
                })
        })
        .collect();
    if paths.is_empty() {
        ui.set_status_message(SharedString::from(
            "Chọn ảnh (jpg, png, bmp, gif, tiff) để xoay",
        ));
        return;
    }
    ui.set_dialog_kind(SharedString::from("confirm-rotate"));
    ui.set_dialog_title(SharedString::from("Xoay ảnh"));
    ui.set_dialog_text(SharedString::from(
        format!(
            "Xoay {} ảnh 90° sang phải. Ảnh gốc bị ghi đè nên không hoàn tác được.",
            paths.len()
        )
        .as_str(),
    ));
    ui.set_dialog_primary_label(SharedString::from("Xoay"));
    ui.set_dialog_has_input(false);
    ui.set_dialog_allows_empty(false);
    ui.set_dialog_open(true);
}

/// Hỏi trước khi làm rỗng Thùng rác — không hoàn tác được.
pub fn ask_empty_recycle_bin(ui: &AppWindow) {
    ui.set_dialog_kind(SharedString::from("confirm-empty-bin"));
    ui.set_dialog_title(SharedString::from("Làm rỗng Thùng rác"));
    ui.set_dialog_text(SharedString::from(
        "Xoá vĩnh viễn mọi mục trong Thùng rác. Không thể hoàn tác.",
    ));
    ui.set_dialog_primary_label(SharedString::from("Làm rỗng"));
    ui.set_dialog_has_input(false);
    ui.set_dialog_allows_empty(false);
    ui.set_dialog_open(true);
}

/// Mở cửa sổ mới ở đúng thư mục đang xem.
///
/// Mỗi cửa sổ có tab, clipboard và lịch sử riêng: dùng chung sẽ khiến việc
/// chuyển thư mục ở cửa sổ này làm nhảy cả cửa sổ kia.
pub fn open_new_window(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let here = app.borrow().tab().current().to_path_buf();
    match crate::windows::open(here) {
        Ok(()) => ui.set_status_message(SharedString::from("Đã mở cửa sổ mới")),
        Err(e) => {
            logging::write(&format!("không mở được cửa sổ mới: {e}"));
            let text = format!("Không mở được cửa sổ mới: {e}");
            ui.set_error_text(SharedString::from(text.as_str()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_command_has_a_dispatch_branch() {
        // Bài học từ lỗi thật: sáu mục palette mở ra rồi bấm không có gì xảy ra
        // vì `run_command` không có nhánh tương ứng. Test này chặn tái diễn.
        let missing: Vec<&str> = PALETTE
            .iter()
            .filter(|c| !c.id.starts_with("tag-set:"))
            .filter(|c| !IMPLEMENTED_COMMANDS.contains(&c.id))
            .map(|c| c.id)
            .collect();
        assert!(
            missing.is_empty(),
            "palette id chưa có nhánh xử lý: {missing:?}"
        );
    }

    #[test]
    fn a_short_tag_list_builds_a_menu_without_panicking() {
        // tags.json do người dùng sửa có thể có ít hơn 4 thẻ; code cũ index
        // thẳng tag_list[0..3] và panic ngay khi mở menu chuột phải.
        let tags = tags::default_tags().into_iter().take(2).collect::<Vec<_>>();
        let items = tag_menu_items(&tags, &[true, false]);
        assert_eq!(items.len(), 2);
        assert!(items[0].label.contains('✓'), "thẻ đã gắn phải có dấu ✓");
        assert!(!items[1].label.contains('✓'));
    }

    #[test]
    fn implemented_command_ids_are_unique() {
        let mut ids = IMPLEMENTED_COMMANDS.to_vec();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "id trùng trong IMPLEMENTED_COMMANDS");
    }
}
