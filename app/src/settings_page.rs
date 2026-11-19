//! Cửa sổ Settings độc lập.
//!
//! Mở dạng cửa sổ riêng thay vì chiếm vùng nội dung của cửa sổ duyệt tệp: người
//! dùng không mất ngữ cảnh đang xem, và cửa sổ chính không bị đổi kích thước khi
//! mở cấu hình.
//!
//! Mỗi thay đổi đi qua đúng bảng lệnh đã có của cửa sổ chính, nên đường ghi
//! `settings.json` không bị nhân đôi: cửa sổ Settings chỉ phát lệnh, cửa sổ chính
//! thực thi và lưu.

use crate::slint_generated::*;
use crate::state::App;
use crate::{logging, tags};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

/// Các trang có trong cửa sổ Settings, theo đúng thứ tự hiển thị.
pub const PAGES: &[(&str, &str, &str)] = &[
    ("appearance", "Giao diện", "weather_moon"),
    ("layout", "Bố cục", "panel_split"),
    ("folders", "Thư mục", "folder"),
    ("tags", "Thẻ màu", "tag"),
    ("advanced", "Nâng cao", "code"),
    ("about", "Giới thiệu", "info"),
];

/// Trang mặc định khi mở Settings.
pub const DEFAULT_PAGE: &str = "appearance";

// Cửa sổ Settings đang mở, nếu có.
//
// Chỉ một cửa sổ tại một thời điểm: mở lần nữa thì đưa cửa sổ cũ lên trước thay
// vì tạo thêm, để hai cửa sổ không ghi đè cấu hình của nhau.
//
// `Timer` đi kèm là **chủ sở hữu thật** của cửa sổ. Slint chỉ giữ `VRc` sống khi
// còn một `Timer` hoặc callback đang hoạt động trỏ tới nó; nếu chỉ cất
// `SettingsWindow` trong thread-local thì cửa sổ tự đóng sau vài giây. Cửa sổ
// chính tránh được điều này nhờ các event pump giữ `Timer`.
struct OpenSettings {
    ui: SettingsWindow,
    /// Giữ cửa sổ sống; dừng timer là điều kiện cần để đóng hẳn.
    _keepalive: slint::Timer,
}

thread_local! {
    static OPEN: RefCell<Option<OpenSettings>> = const { RefCell::new(None) };
}

/// Mở cửa sổ Settings, hoặc đưa cửa sổ đang có lên trước.
///
/// `command` là hàm của cửa sổ chính; cửa sổ Settings gọi lại nó cho mọi thay
/// đổi cấu hình, nhờ vậy chỉ có một đường ghi.
pub fn open(
    app: &Rc<RefCell<App>>,
    page: &str,
    command: impl Fn(&str) + 'static,
) -> Result<(), slint::PlatformError> {
    // Đã mở thì chỉ cần đưa lên trước và chuyển trang.
    let already = OPEN.with(|slot| slot.borrow().is_some());
    if already {
        OPEN.with(|slot| {
            if let Some(existing) = slot.borrow().as_ref() {
                if let Some(target) = valid_page(page) {
                    existing.ui.set_page(SharedString::from(target));
                }
                refresh(&existing.ui, app);
                let _ = existing.ui.show();
            }
        });
        return Ok(());
    }

    let ui = SettingsWindow::new()?;
    install(&ui, app, command);
    if let Some(target) = valid_page(page) {
        ui.set_page(SharedString::from(target));
    }

    // Đóng cửa sổ thì quên handle để lần sau tạo mới.
    {
        let weak = ui.as_weak();
        ui.on_window_close(move || {
            if let Some(ui) = weak.upgrade() {
                let _ = ui.hide();
            }
            OPEN.with(|slot| *slot.borrow_mut() = None);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_close_requested(move || {
            if let Some(ui) = weak.upgrade() {
                let _ = ui.hide();
            }
            OPEN.with(|slot| *slot.borrow_mut() = None);
        });
    }

    ui.show()?;
    // Timer giữ cửa sổ sống. Không dùng `mem::forget`: handle nằm trong `OPEN`
    // nên khi đóng, `OPEN` được dọn và timer bị thả theo.
    let keepalive = slint::Timer::default();
    OPEN.with(|slot| {
        *slot.borrow_mut() = Some(OpenSettings {
            ui,
            _keepalive: keepalive,
        })
    });
    Ok(())
}

/// Đóng cửa sổ Settings nếu đang mở.
///
/// Cửa sổ tự đóng khi bấm nút X, nhưng lệnh "Đóng Settings" từ Command Palette
/// hoặc phím Esc ở cửa sổ chính cũng cần đóng được nó.
#[allow(dead_code)]
pub fn close() {
    OPEN.with(|slot| {
        if let Some(open) = slot.borrow_mut().take() {
            let _ = open.ui.hide();
        }
    });
}

/// Điền danh sách trang và nối callback.
fn install(ui: &SettingsWindow, app: &Rc<RefCell<App>>, command: impl Fn(&str) + 'static) {
    // Bọc một lần rồi chia sẻ: mọi callback đều cần gọi cùng một hàm.
    let command = Rc::new(command);
    let pages: Vec<SettingsPageInfo> = PAGES
        .iter()
        .map(|(id, label, icon)| SettingsPageInfo {
            id: SharedString::from(*id),
            label: SharedString::from(*label),
            icon: SharedString::from(*icon),
        })
        .collect();
    ui.set_pages(ModelRc::new(VecModel::from(pages)));
    ui.set_page(SharedString::from(DEFAULT_PAGE));
    refresh(ui, app);

    // Lệnh gửi về cửa sổ chính. Sau mỗi lệnh phải đọc lại trạng thái vì cửa sổ
    // chính mới là nơi giữ giá trị thật.
    {
        let (weak, a, cmd) = (ui.as_weak(), app.clone(), command.clone());
        ui.on_command(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            cmd(id.as_str());
            refresh(&ui, &a);
        });
    }

    {
        let (weak, a) = (ui.as_weak(), app.clone());
        ui.on_page_changed(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_page(id);
            refresh(&ui, &a);
        });
    }

    {
        let (weak, cmd) = (ui.as_weak(), command.clone());
        ui.on_tag_rename(move |uid| {
            let Some(_ui) = weak.upgrade() else { return };
            // Hộp thoại nhập liệu nằm ở cửa sổ chính: nó đã có sẵn `DialogSurface`
            // và đường xác nhận. Cửa sổ Settings chỉ yêu cầu mở nó.
            cmd(&format!("settings-tag-rename:{uid}"));
        });
    }
    {
        let (weak, cmd) = (ui.as_weak(), command.clone());
        ui.on_tag_recolor(move |uid| {
            let Some(_ui) = weak.upgrade() else { return };
            cmd(&format!("settings-tag-recolor:{uid}"));
        });
    }
    {
        let (weak, a, cmd) = (ui.as_weak(), app.clone(), command.clone());
        ui.on_tag_remove(move |uid| {
            let Some(ui) = weak.upgrade() else { return };
            let _ = &cmd;
            remove_tag(&ui, &a, uid.as_str());
        });
    }
    {
        let (weak, a) = (ui.as_weak(), app.clone());
        ui.on_tag_add(move || {
            let Some(ui) = weak.upgrade() else { return };
            add_tag(&ui, &a);
        });
    }
    {
        let (weak, cmd) = (ui.as_weak(), command.clone());
        ui.on_open_location(move |path| {
            let Some(_ui) = weak.upgrade() else { return };
            cmd(&format!("settings-open:{path}"));
        });
    }

    // Nút cửa sổ và kéo/đổi kích thước, giống cửa sổ chính.
    {
        let weak = ui.as_weak();
        ui.on_window_minimize(move || {
            if let Some(ui) = weak.upgrade() {
                ui.window().set_minimized(true);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_window_toggle_maximize(move || {
            if let Some(ui) = weak.upgrade() {
                let max = !ui.get_is_maximized();
                ui.set_is_maximized(max);
                ui.window().set_maximized(max);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_window_drag(move |_, _| {
            let Some(ui) = weak.upgrade() else { return };
            use slint::winit_030::WinitWindowAccessor;
            ui.window().with_winit_window(|win| {
                if let Err(e) = win.drag_window() {
                    logging::write(&format!("Settings: drag_window lỗi: {e}"));
                }
            });
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_window_resize(move |edge| {
            let Some(ui) = weak.upgrade() else { return };
            use slint::winit_030::winit::window::ResizeDirection;
            use slint::winit_030::WinitWindowAccessor;
            let direction = match edge.as_str() {
                "n" => ResizeDirection::North,
                "s" => ResizeDirection::South,
                "e" => ResizeDirection::East,
                "w" => ResizeDirection::West,
                "ne" => ResizeDirection::NorthEast,
                "nw" => ResizeDirection::NorthWest,
                "se" => ResizeDirection::SouthEast,
                "sw" => ResizeDirection::SouthWest,
                _ => return,
            };
            ui.window().with_winit_window(|win| {
                if let Err(e) = win.drag_resize_window(direction) {
                    logging::write(&format!("Settings: drag_resize_window lỗi: {e}"));
                }
            });
        });
    }
}

/// Làm mới dữ liệu động: thẻ, đường dẫn cấu hình, và trạng thái gương của cửa
/// sổ chính để các công tắc hiện đúng lựa chọn hiện tại.
pub fn refresh(ui: &SettingsWindow, app: &Rc<RefCell<App>>) {
    let a = app.borrow();
    // `Theme` là global riêng của từng cây component, không dùng chung giữa hai
    // cửa sổ. Thiếu dòng này thì cửa sổ Settings luôn sáng dù cửa sổ chính đang
    // tối — nền hai bên lệch nhau.
    ui.global::<Theme>().set_dark(a.dark);
    let tag_list: Vec<TagInfo> = a
        .tags
        .tags
        .iter()
        .map(|t| TagInfo {
            uid: SharedString::from(t.uid.as_str()),
            name: SharedString::from(t.name.as_str()),
            color: crate::view::color_from_u32(tags::parse_hex(&t.color).unwrap_or(0x808080)),
        })
        .collect();
    ui.set_tags(ModelRc::new(VecModel::from(tag_list)));
    ui.set_settings_path(SharedString::from(
        a.settings_path.to_string_lossy().as_ref(),
    ));
    ui.set_tags_path(SharedString::from(a.tag_path.to_string_lossy().as_ref()));
    ui.set_log_path(SharedString::from(
        logging::log_path().to_string_lossy().as_ref(),
    ));
    ui.set_version(SharedString::from(env!("CARGO_PKG_VERSION")));

    ui.set_dark(a.dark);
    ui.set_view_mode(a.view_mode);
    ui.set_show_sidebar(a.show_sidebar);
    ui.set_show_preview(a.show_preview);
    ui.set_dual_pane(a.right.is_some());
    ui.set_show_hidden(a.show_hidden);
    ui.set_show_extensions(a.show_extensions);
    ui.set_folder_order(SharedString::from(a.folder_order.label()));
}

/// Trả `Some(id)` khi `page` là trang có thật.
fn valid_page(page: &str) -> Option<&'static str> {
    PAGES
        .iter()
        .find(|(id, _, _)| *id == page)
        .map(|(id, _, _)| *id)
}

/// Áp tên mới cho thẻ, gọi từ hộp thoại của cửa sổ chính.
///
/// Hộp thoại nhập liệu nằm ở cửa sổ chính (`dialogs::ask_tag_rename`) vì cửa sổ
/// Settings chỉ có công tắc và danh sách. Cửa sổ chính ghi nhớ thẻ đang sửa
/// trong `dialog-target` rồi gọi hàm này khi người dùng xác nhận.
pub fn apply_rename(app: &Rc<RefCell<App>>, uid: &str, name: &str) {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return;
    }
    apply_tag_change(app, uid, |tag| tag.name = trimmed.to_string());
    refresh_open(app);
}

/// Áp màu mới cho thẻ, gọi từ hộp thoại của cửa sổ chính.
pub fn apply_recolor(app: &Rc<RefCell<App>>, uid: &str, color: &str) {
    if tags::parse_hex(color).is_none() {
        return;
    }
    let normalized = format!("#{}", color.trim().trim_start_matches('#').to_uppercase());
    apply_tag_change(app, uid, |tag| tag.color = normalized);
    refresh_open(app);
}

/// Làm mới cửa sổ Settings nếu nó đang mở.
pub fn refresh_open(app: &Rc<RefCell<App>>) {
    OPEN.with(|slot| {
        if let Some(open) = slot.borrow().as_ref() {
            refresh(&open.ui, app);
        }
    });
}

/// Xoá một thẻ khỏi định nghĩa và khỏi mọi tệp đang gắn nó.
pub fn remove_tag(ui: &SettingsWindow, app: &Rc<RefCell<App>>, uid: &str) {
    {
        let mut a = app.borrow_mut();
        let mut store = tags::TagStore::load(&a.tag_path);
        let before = store.tags.len();
        store.tags.retain(|t| t.uid != uid);
        if store.tags.len() == before {
            return;
        }
        for list in store.assignments.values_mut() {
            list.retain(|u| u != uid);
        }
        store.assignments.retain(|_, list| !list.is_empty());
        a.tags = store.clone();
        let path = a.tag_path.clone();
        if let Err(e) = store.save(&path) {
            logging::write(&format!("không lưu được thẻ sau khi xoá: {e}"));
            return;
        }
    }
    refresh(ui, app);
}

/// Thêm một thẻ mới với tên và màu mặc định.
pub fn add_tag(ui: &SettingsWindow, app: &Rc<RefCell<App>>) {
    {
        let mut a = app.borrow_mut();
        let mut store = tags::TagStore::load(&a.tag_path);
        let n = store.tags.len() + 1;
        store.tags.push(tags::Tag {
            // uid mới không trùng uid cũ: dùng số thứ tự hiện tại.
            uid: format!("tag-{n}-{}", store.tags.len()),
            name: format!("Thẻ {n}"),
            color: "#808080".into(),
        });
        a.tags = store.clone();
        let path = a.tag_path.clone();
        if let Err(e) = store.save(&path) {
            logging::write(&format!("không lưu được thẻ mới: {e}"));
            return;
        }
    }
    refresh(ui, app);
}

/// Mở thư mục chứa một tệp cấu hình, hoặc mở URL bằng trình duyệt.
pub fn open_location(ui: &AppWindow, app: &Rc<RefCell<App>>, path: &str) {
    if path.starts_with("http://") || path.starts_with("https://") {
        if let Err(e) = open::that_detached(path) {
            ui.set_error_text(SharedString::from(
                format!("Không mở được liên kết: {e}").as_str(),
            ));
        }
        return;
    }
    let file = std::path::Path::new(path);
    let folder = if file.is_dir() {
        file.to_path_buf()
    } else {
        file.parent().unwrap_or(file).to_path_buf()
    };
    if folder.as_os_str().is_empty() {
        return;
    }
    // Điều hướng trong cửa sổ chính thay vì mở Explorer: người dùng đang ở đây
    // để sửa cấu hình, mở cửa sổ mới chỉ làm mất ngữ cảnh.
    crate::navigation::navigate_to(ui, app, folder);
}

/// Áp một thay đổi lên một thẻ rồi lưu ngay.
fn apply_tag_change(app: &Rc<RefCell<App>>, uid: &str, change: impl FnOnce(&mut tags::Tag)) {
    let mut a = app.borrow_mut();
    let mut store = tags::TagStore::load(&a.tag_path);
    let Some(tag) = store.tags.iter_mut().find(|t| t.uid == uid) else {
        return;
    };
    change(tag);
    a.tags = store.clone();
    let path = a.tag_path.clone();
    if let Err(e) = store.save(&path) {
        logging::write(&format!("không lưu được thẻ: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_ids() -> Vec<&'static str> {
        PAGES.iter().map(|(id, _, _)| *id).collect()
    }

    #[test]
    fn every_page_has_an_id_label_and_icon() {
        for (id, label, icon) in PAGES {
            assert!(!id.is_empty(), "trang phải có id");
            assert!(!label.is_empty(), "trang {id} phải có nhãn");
            assert!(!icon.is_empty(), "trang {id} phải có icon");
        }
    }

    #[test]
    fn page_ids_are_unique() {
        let mut ids = page_ids();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(before, ids.len(), "id trang bị trùng");
    }

    #[test]
    fn the_default_page_exists() {
        assert!(page_ids().contains(&DEFAULT_PAGE));
    }

    #[test]
    fn an_unknown_page_is_rejected() {
        assert_eq!(valid_page("khong-ton-tai"), None);
        assert_eq!(valid_page("appearance"), Some("appearance"));
    }

    #[test]
    fn the_six_pages_cover_the_documented_features() {
        let ids = page_ids();
        for required in [
            "appearance",
            "layout",
            "folders",
            "tags",
            "advanced",
            "about",
        ] {
            assert!(ids.contains(&required), "thiếu trang {required}");
        }
    }
}
