//! Dialog state and confirmation.
//!
//! The UI has one reusable dialog surface; these functions decide its title,
//! body, whether it needs a text box, and what to do when the user confirms.
//! Keeping them together means the "which dialog is open" contract is in one
//! place instead of spread across the callbacks.

use crate::commands::{menu, register_association};
use crate::slint_generated::*;
use crate::state::App;
use crate::{format, logging, operations, ops_worker, preview};
use ops_worker::Job;
use slint::{ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Ask before a permanent delete: there is no undo, so the dialog is the guard.
pub fn ask_delete_permanent(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let n = app.borrow().tab().selection.len();
    if n == 0 {
        return;
    }
    ui.set_dialog_kind(SharedString::from("confirm-delete-permanent"));
    ui.set_dialog_title(SharedString::from("Xoá vĩnh viễn"));
    ui.set_dialog_text(SharedString::from(
        format!("Xoá {n} mục và KHÔNG chuyển vào Thùng rác? Không thể hoàn tác.").as_str(),
    ));
    ui.set_dialog_primary_label(SharedString::from("Xoá vĩnh viễn"));
    ui.set_dialog_has_input(false);
    ui.set_dialog_allows_empty(false);
    ui.set_dialog_value(SharedString::default());
    ui.set_dialog_open(true);
}

pub fn ask_delete(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let n = app.borrow().tab().selection.len();
    if n == 0 {
        return;
    }
    ui.set_dialog_kind(SharedString::from("confirm-delete"));
    ui.set_dialog_title(SharedString::from("Xoá mục"));
    ui.set_dialog_text(SharedString::from(
        format!("Chuyển {n} mục đã chọn vào Thùng rác?").as_str(),
    ));
    ui.set_dialog_primary_label(SharedString::from("Xoá"));
    ui.set_dialog_has_input(false);
    ui.set_dialog_allows_empty(false);
    ui.set_dialog_value(SharedString::default());
    ui.set_dialog_open(true);
}

pub fn ask_rename(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let current = {
        let s = app.borrow();
        s.tab().selected_entries().first().map(|e| e.name.clone())
    };
    let Some(name) = current else { return };
    ui.set_dialog_kind(SharedString::from("rename"));
    ui.set_dialog_title(SharedString::from("Đổi tên"));
    ui.set_dialog_text(SharedString::from("Nhập tên mới"));
    ui.set_dialog_primary_label(SharedString::from("Đổi tên"));
    ui.set_dialog_has_input(true);
    ui.set_dialog_allows_empty(false);
    ui.set_dialog_value(SharedString::from(name.as_str()));
    ui.set_dialog_open(true);
}

pub fn ask_new(ui: &AppWindow, kind: &str) {
    let (title, placeholder, label) = if kind == "new-folder" {
        ("Thư mục mới", "Thư mục mới", "Tạo")
    } else {
        ("Tệp mới", "Tệp mới.txt", "Tạo")
    };
    ui.set_dialog_kind(SharedString::from(kind));
    ui.set_dialog_title(SharedString::from(title));
    ui.set_dialog_text(SharedString::from(placeholder));
    ui.set_dialog_primary_label(SharedString::from(label));
    ui.set_dialog_has_input(true);
    // Tạo thư mục/tệp mới: để trống vẫn bấm được, Rust tự chọn tên chưa dùng.
    ui.set_dialog_allows_empty(true);
    ui.set_dialog_value(SharedString::default());
    ui.set_dialog_open(true);
}

/// Menu của nút "Mới" (phần mũi tên tách rời).
pub fn show_new_menu(ui: &AppWindow, x: f32, y: f32) {
    let items = vec![
        menu(
            "new-folder",
            "Thư mục mới",
            "folder",
            "Ctrl+Shift+N",
            true,
            false,
        ),
        menu("new-file", "Tệp văn bản mới", "document", "", true, false),
    ];
    ui.set_menu_items(ModelRc::new(VecModel::from(items)));
    ui.set_menu_x(x);
    ui.set_menu_y(y);
    ui.set_menu_open(true);
}

pub fn confirm_dialog(ui: &AppWindow, app: &Rc<RefCell<App>>, value: &str) {
    let kind = ui.get_dialog_kind().to_string();
    ui.set_dialog_open(false);
    let current = app.borrow().tab().current().to_path_buf();
    // Ghi lại để khi người dùng báo "không được" thì biết lệnh có tới Rust không,
    // và thư mục đích là đâu.
    logging::write(&format!(
        "xác nhận: kind={kind} value={value:?} tại {:?}",
        current
    ));

    match kind.as_str() {
        "rename" => {
            let from = {
                let s = app.borrow();
                s.tab()
                    .selected_entries()
                    .first()
                    .map(|e| PathBuf::from(&e.path))
            };
            if let Some(from) = from {
                app.borrow().worker.submit(Job::Rename {
                    from,
                    to: value.to_string(),
                });
                operations::begin(ui, app, "Đang đổi tên…");
            }
        }
        "new-folder" => {
            app.borrow().worker.submit(Job::CreateFolder {
                parent: current,
                name: value.to_string(),
            });
            operations::begin(ui, app, "Đang tạo thư mục…");
        }
        "new-file" => {
            app.borrow().worker.submit(Job::CreateFile {
                parent: current,
                name: value.to_string(),
            });
            operations::begin(ui, app, "Đang tạo tệp…");
        }
        "confirm-delete" => {
            let paths = app.borrow().tab().selected_paths();
            if !paths.is_empty() {
                app.borrow().worker.submit(Job::Delete { paths });
                operations::begin(ui, app, "Đang chuyển vào Thùng rác…");
            }
        }
        "confirm-default" => register_association(ui),
        // Nút chính của hộp thoại trùng tên là "Giữ cả hai".
        "confirm-empty-bin" => match files_fs::empty_recycle_bin() {
            Ok(()) => ui.set_status_message(SharedString::from("Đã làm rỗng Thùng rác")),
            Err(e) => {
                let text = format::explain_error(&e.code, &e.message);
                ui.set_error_text(SharedString::from(text.as_str()));
            }
        },
        "confirm-rotate" => {
            let paths = app.borrow().tab().selected_paths();
            if !paths.is_empty() {
                app.borrow()
                    .worker
                    .submit(crate::ops_worker::Job::Rotate { paths });
                operations::begin(ui, app, "Đang xoay ảnh…");
            }
        }
        "paste-conflict" => {
            crate::file_ops::do_paste_with(ui, app, files_fs::ConflictPolicy::KeepBoth)
        }
        "confirm-delete-permanent" => {
            let paths = app.borrow().tab().selected_paths();
            if !paths.is_empty() {
                app.borrow().worker.submit(Job::DeletePermanent { paths });
                operations::begin(ui, app, "Đang xoá vĩnh viễn…");
            }
        }
        _ => {}
    }
}

pub fn show_properties(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let s = app.borrow();
    let Some(entry) = s.tab().selected_entries().first().cloned() else {
        return;
    };
    preview::show(ui, entry);
}
