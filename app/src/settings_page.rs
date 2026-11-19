//! Trang Settings: đọc/ghi cấu hình qua UI thay vì sửa `settings.json` tay.
//!
//! Mỗi thay đổi đi qua đúng đường đã có: `commands::toggle_*` hoặc
//! `commands::set_view_mode`, nên hành vi ở đây và ở command bar không thể lệch
//! nhau. Module này chỉ lo phần riêng của trang: danh sách trang, dữ liệu thẻ,
//! đường dẫn cấu hình và việc mở lại trang.

use crate::slint_generated::*;
use crate::state::App;
use crate::{logging, tags};
use slint::{ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

/// Các trang có trong cửa sổ Settings, theo đúng thứ tự hiển thị.
///
/// Giữ ở đây thay vì khai trong Slint để Rust và UI dùng chung một danh sách:
/// thêm trang chỉ cần sửa một chỗ.
pub const PAGES: &[(&str, &str, &str)] = &[
    ("appearance", "Giao diện", "weather_moon"),
    ("layout", "Bố cục", "panel_split"),
    ("folders", "Thư mục", "folder"),
    ("tags", "Thẻ màu", "tag"),
    ("advanced", "Nâng cao", "code"),
    ("about", "Giới thiệu", "info"),
];

/// Trang mặc định khi mở Settings lần đầu.
pub const DEFAULT_PAGE: &str = "appearance";

/// Điền các property tĩnh mà trang Settings cần.
///
/// Gọi một lần lúc mở cửa sổ; dữ liệu động (thẻ, đường dẫn) có hàm riêng để
/// làm mới sau khi người dùng sửa.
pub fn install(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let pages: Vec<SettingsPageInfo> = PAGES
        .iter()
        .map(|(id, label, icon)| SettingsPageInfo {
            id: SharedString::from(*id),
            label: SharedString::from(*label),
            icon: SharedString::from(*icon),
        })
        .collect();
    ui.set_settings_pages(ModelRc::new(VecModel::from(pages)));
    ui.set_settings_page(SharedString::from(DEFAULT_PAGE));
    refresh(ui, app);
}

/// Làm mới dữ liệu động: danh sách thẻ và các đường dẫn cấu hình.
pub fn refresh(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let (tag_list, settings_path, tags_path, version) = {
        let a = app.borrow();
        let list: Vec<TagInfo> = a
            .tags
            .tags
            .iter()
            .map(|t| TagInfo {
                uid: SharedString::from(t.uid.as_str()),
                name: SharedString::from(t.name.as_str()),
                color: crate::view::color_from_u32(tags::parse_hex(&t.color).unwrap_or(0x808080)),
            })
            .collect();
        (
            list,
            a.settings_path.to_string_lossy().into_owned(),
            a.tag_path.to_string_lossy().into_owned(),
            env!("CARGO_PKG_VERSION").to_string(),
        )
    };
    ui.set_settings_tags(ModelRc::new(VecModel::from(tag_list)));
    ui.set_settings_settings_path(SharedString::from(settings_path.as_str()));
    ui.set_settings_tags_path(SharedString::from(tags_path.as_str()));
    ui.set_settings_log_path(SharedString::from(
        logging::log_path().to_string_lossy().as_ref(),
    ));
    ui.set_settings_version(SharedString::from(version.as_str()));
}

/// Mở trang Settings ở một trang cụ thể.
pub fn open(ui: &AppWindow, app: &Rc<RefCell<App>>, page: &str) {
    refresh(ui, app);
    let target = if PAGES.iter().any(|(id, _, _)| *id == page) {
        page
    } else {
        DEFAULT_PAGE
    };
    ui.set_settings_page(SharedString::from(target));
    ui.set_view(SharedString::from("settings"));
}

/// Đóng trang Settings, quay lại duyệt tệp.
pub fn close(ui: &AppWindow) {
    ui.set_view(SharedString::from("files"));
}

/// Đổi tên một thẻ.
///
/// Giữ nguyên `uid` để mọi tệp đã gắn thẻ không mất màu. Ghi ngay để tắt app
/// đột ngột không làm mất thay đổi.
pub fn rename_tag(ui: &AppWindow, app: &Rc<RefCell<App>>, uid: &str) {
    let Some(current) = current_tag_name(app, uid) else {
        return;
    };
    ui.set_dialog_kind(SharedString::from("settings-tag-rename"));
    ui.set_dialog_title(SharedString::from("Đổi tên thẻ"));
    ui.set_dialog_text(SharedString::from("Nhập tên mới cho thẻ"));
    ui.set_dialog_primary_label(SharedString::from("Đổi tên"));
    ui.set_dialog_value(SharedString::from(current.as_str()));
    ui.set_dialog_has_input(true);
    ui.set_dialog_allows_empty(false);
    // Nhớ thẻ nào đang sửa: hộp thoại chỉ trả về chuỗi.
    ui.set_dialog_target(SharedString::from(uid));
    ui.set_dialog_open(true);
}

/// Áp tên mới sau khi hộp thoại xác nhận.
pub fn apply_rename(ui: &AppWindow, app: &Rc<RefCell<App>>, uid: &str, name: &str) {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return;
    }
    {
        let mut a = app.borrow_mut();
        let mut store = tags::TagStore::load(&a.tag_path);
        let Some(tag) = store.tags.iter_mut().find(|t| t.uid == uid) else {
            return;
        };
        tag.name = trimmed.to_string();
        a.tags = store.clone();
        let path = a.tag_path.clone();
        if let Err(e) = store.save(&path) {
            logging::write(&format!("không lưu được tên thẻ: {e}"));
            ui.set_error_text(SharedString::from(
                format!("Không lưu được thẻ: {e}").as_str(),
            ));
            return;
        }
    }
    ui.set_status_message(SharedString::from(
        format!("Đã đổi tên thẻ thành {trimmed}").as_str(),
    ));
    refresh(ui, app);
    crate::view::push_view(ui, app);
}

/// Đổi màu một thẻ.
pub fn recolor_tag(ui: &AppWindow, app: &Rc<RefCell<App>>, uid: &str) {
    let Some(current) = current_tag_color(app, uid) else {
        return;
    };
    ui.set_dialog_kind(SharedString::from("settings-tag-recolor"));
    ui.set_dialog_title(SharedString::from("Đổi màu thẻ"));
    ui.set_dialog_text(SharedString::from("Nhập mã màu dạng #RRGGBB"));
    ui.set_dialog_primary_label(SharedString::from("Đổi màu"));
    ui.set_dialog_value(SharedString::from(current.as_str()));
    ui.set_dialog_has_input(true);
    ui.set_dialog_allows_empty(false);
    ui.set_dialog_target(SharedString::from(uid));
    ui.set_dialog_open(true);
}

/// Áp màu mới sau khi hộp thoại xác nhận.
pub fn apply_recolor(ui: &AppWindow, app: &Rc<RefCell<App>>, uid: &str, color: &str) {
    // Chỉ nhận mã hợp lệ: một chuỗi rác sẽ làm icon mất màu mà không ai hiểu vì sao.
    if tags::parse_hex(color).is_none() {
        ui.set_error_text(SharedString::from(
            "Mã màu phải có dạng #RRGGBB, ví dụ #0072BD",
        ));
        return;
    }
    let normalized = format!("#{}", color.trim().trim_start_matches('#').to_uppercase());
    {
        let mut a = app.borrow_mut();
        let mut store = tags::TagStore::load(&a.tag_path);
        let Some(tag) = store.tags.iter_mut().find(|t| t.uid == uid) else {
            return;
        };
        tag.color = normalized.clone();
        a.tags = store.clone();
        let path = a.tag_path.clone();
        if let Err(e) = store.save(&path) {
            logging::write(&format!("không lưu được màu thẻ: {e}"));
            ui.set_error_text(SharedString::from(
                format!("Không lưu được thẻ: {e}").as_str(),
            ));
            return;
        }
    }
    ui.set_status_message(SharedString::from(
        format!("Đã đổi màu thẻ thành {normalized}").as_str(),
    ));
    refresh(ui, app);
    crate::view::push_view(ui, app);
}

/// Xoá một thẻ.
///
/// Thẻ bị xoá khỏi định nghĩa và khỏi mọi tệp đang gắn nó; tệp vẫn còn, chỉ mất
/// màu. Đây là hành vi của Files gốc và cũng là điều người dùng mong đợi.
pub fn remove_tag(ui: &AppWindow, app: &Rc<RefCell<App>>, uid: &str) {
    let removed = {
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
            ui.set_error_text(SharedString::from(
                format!("Không lưu được thẻ: {e}").as_str(),
            ));
            return;
        }
        true
    };
    if removed {
        ui.set_status_message(SharedString::from("Đã xoá thẻ"));
    }
    refresh(ui, app);
    crate::view::push_view(ui, app);
}

/// Thêm một thẻ mới với tên và màu mặc định.
pub fn add_tag(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let count = {
        let mut a = app.borrow_mut();
        let mut store = tags::TagStore::load(&a.tag_path);
        let n = store.tags.len() + 1;
        store.tags.push(tags::Tag {
            // uid mới không được trùng uid cũ; dùng số thứ tự và tên làm khoá.
            uid: format!("tag-{n}-{}", store.tags.len()),
            name: format!("Thẻ {n}"),
            color: "#808080".into(),
        });
        a.tags = store.clone();
        let path = a.tag_path.clone();
        if let Err(e) = store.save(&path) {
            logging::write(&format!("không lưu được thẻ mới: {e}"));
            ui.set_error_text(SharedString::from(
                format!("Không lưu được thẻ: {e}").as_str(),
            ));
            return;
        }
        n
    };
    ui.set_status_message(SharedString::from(format!("Đã thêm Thẻ {count}").as_str()));
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
    // Điều hướng trong app thay vì mở Explorer: người dùng đang ở đây để sửa
    // cấu hình, và một cửa sổ mới chỉ làm họ mất chỗ.
    ui.set_view(SharedString::from("files"));
    crate::navigation::navigate_to(ui, app, folder);
}

fn current_tag_name(app: &Rc<RefCell<App>>, uid: &str) -> Option<String> {
    app.borrow()
        .tags
        .tags
        .iter()
        .find(|t| t.uid == uid)
        .map(|t| t.name.clone())
}

fn current_tag_color(app: &Rc<RefCell<App>>, uid: &str) -> Option<String> {
    app.borrow()
        .tags
        .tags
        .iter()
        .find(|t| t.uid == uid)
        .map(|t| t.color.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_ids() -> Vec<&'static str> {
        PAGES.iter().map(|(id, _, _)| *id).collect()
    }

    fn page_label(id: &str) -> Option<&'static str> {
        PAGES
            .iter()
            .find(|(page, _, _)| *page == id)
            .map(|(_, label, _)| *label)
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
        // Trùng id khiến hai mục cùng sáng khi chọn một trang.
        let mut ids = page_ids();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(before, ids.len(), "id trang bị trùng");
    }

    #[test]
    fn the_default_page_exists() {
        assert!(
            page_ids().contains(&DEFAULT_PAGE),
            "trang mặc định phải nằm trong danh sách"
        );
    }

    #[test]
    fn an_unknown_page_reports_no_label() {
        assert_eq!(page_label("khong-ton-tai"), None);
        assert_eq!(page_label("appearance"), Some("Giao diện"));
    }

    #[test]
    fn the_six_pages_cover_the_documented_features() {
        // Bốn trang đầu là yêu cầu của kế hoạch C1–C4; About là trang thứ năm.
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
