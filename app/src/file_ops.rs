//! File operations: clipboard, paste, compress, extract, hash, folder size.
//!
//! Every operation is submitted to the worker and reported back as a status line
//! or an error; nothing here blocks the UI thread. This is also where an undo
//! stack would hook in later, which is why the operations are gathered in one
//! module instead of scattered through the callbacks.

use crate::slint_generated::*;
use crate::navigation::reload;
use crate::state::App;
use crate::{format, logging, ops_worker};
use ops_worker::Job;
use slint::SharedString;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Put the selected paths on the clipboard as text, one per line.
pub fn copy_selected_path(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let paths = app.borrow().tab().selected_paths();
    // Nothing selected means "copy where I am", which is what Files does.
    let text = if paths.is_empty() {
        app.borrow().tab().current().to_string_lossy().into_owned()
    } else {
        paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\r\n")
    };
    match files_fs::copy_text_to_clipboard(&text) {
        Ok(()) => ui.set_status_tertiary(SharedString::from("Đã sao chép đường dẫn")),
        Err(e) => ui.set_error_text(SharedString::from(e.message.as_str())),
    }
}

pub fn open_terminal_here(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let dir = app.borrow().tab().current().to_path_buf();
    if let Err(e) = files_fs::open_terminal(&dir) {
        ui.set_error_text(SharedString::from(e.message.as_str()));
    }
}

// ---------------------------------------------------------------- tab commands

pub fn set_clipboard(ui: &AppWindow, app: &Rc<RefCell<App>>, cut: bool) {
    let paths = app.borrow().tab().selected_paths();
    if paths.is_empty() {
        return;
    }
    let count = paths.len() as i32;
    {
        let mut s = app.borrow_mut();
        s.clipboard = paths;
        s.clipboard_cut = cut;
    }
    // Chip trên command bar chỉ có ý nghĩa khi biết đang giữ gì.
    ui.set_clipboard_mode(SharedString::from(if cut { "cut" } else { "copy" }));
    ui.set_clipboard_count(count);
    ui.set_status_tertiary(SharedString::from(
        format!(
            "Đã {} {count} mục",
            if cut { "cắt" } else { "sao chép" }
        )
        .as_str(),
    ));
}

/// Dán với cách xử lý trùng tên do người dùng chọn.
pub fn do_paste_with(
    ui: &AppWindow,
    app: &Rc<RefCell<App>>,
    policy: files_fs::ConflictPolicy,
) {
    let (sources, target, cut) = {
        let s = app.borrow();
        (s.clipboard.clone(), s.tab().current().to_path_buf(), s.clipboard_cut)
    };
    if sources.is_empty() {
        return;
    }
    ui.set_busy(true);
    ui.set_busy_text(SharedString::from(if cut { "Đang di chuyển…" } else { "Đang sao chép…" }));
    let job = if cut {
        Job::Move { sources, target, policy }
    } else {
        Job::Copy { sources, target, policy }
    };
    app.borrow().worker.submit(job);
}

/// Kiểm tra trước khi dán: nếu có tên trùng thì hỏi người dùng.
///
/// Chỉ hỏi khi thật sự cần — một lần dán không trùng tên mà vẫn bật hộp thoại
/// là làm phiền vô ích.
pub fn ask_paste(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let (sources, target) = {
        let s = app.borrow();
        (s.clipboard.clone(), s.tab().current().to_path_buf())
    };
    if sources.is_empty() {
        return;
    }
    let conflicts = sources
        .iter()
        .filter(|src| {
            src.file_name()
                .map(|n| target.join(n).exists())
                .unwrap_or(false)
        })
        .count();
    if conflicts == 0 {
        do_paste_with(ui, app, files_fs::ConflictPolicy::Fail);
        return;
    }
    ui.set_dialog_kind(SharedString::from("paste-conflict"));
    ui.set_dialog_title(SharedString::from("Đã có mục cùng tên"));
    ui.set_dialog_text(SharedString::from(
        format!(
            "{conflicts} mục đã tồn tại trong thư mục này. Chọn cách xử lý:              giữ cả hai (đổi tên mục mới), thay thế mục cũ, hoặc bỏ qua."
        )
        .as_str(),
    ));
    ui.set_dialog_primary_label(SharedString::from("Giữ cả hai"));
    ui.set_dialog_secondary_label(SharedString::from("Thay thế"));
    ui.set_dialog_has_input(false);
    ui.set_dialog_allows_empty(false);
    ui.set_dialog_open(true);
}

/// Compress the selection into `<name>.zip` in the current folder.
pub fn compress_selection(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let (sources, dir) = {
        let s = app.borrow();
        (s.tab().selected_paths(), s.tab().current().to_path_buf())
    };
    if sources.is_empty() {
        return;
    }
    let name = files_fs::archive_name(&sources);
    let dest = dir.join(format!("{name}.zip"));
    ui.set_busy(true);
    ui.set_busy_text(SharedString::from("Đang nén…"));
    app.borrow().worker.submit(Job::Zip { sources, dest });
}

/// Extract the selected archive into a folder named after it.
pub fn extract_selection(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let archive = {
        let s = app.borrow();
        s.tab()
            .selected_entries()
            .first()
            .map(|e| PathBuf::from(&e.path))
    };
    let Some(archive) = archive else { return };
    // `photos.zip` extracts into `photos`, and `photos (2)` if that exists.
    let stem = archive
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "extracted".into());
    let parent = archive.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut target = parent.join(&stem);
    let mut n = 2;
    while target.exists() {
        target = parent.join(format!("{stem} ({n})"));
        n += 1;
    }
    ui.set_busy(true);
    ui.set_busy_text(SharedString::from("Đang giải nén…"));
    app.borrow().worker.submit(Job::Unzip { archive, target });
}

pub fn hash_selection(ui: &AppWindow, app: &Rc<RefCell<App>>, kind: files_fs::HashKind) {
    let path = {
        let s = app.borrow();
        s.tab()
            .selected_entries()
            .first()
            .map(|e| PathBuf::from(&e.path))
    };
    if let Some(path) = path {
        ui.set_busy(true);
        ui.set_busy_text(SharedString::from("Đang tính mã băm…"));
        app.borrow().worker.submit(Job::Hash { path, kind });
    }
}

/// Walk the selected folder to learn its size, then fill the size column.
pub fn view_folder_size(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let path = {
        let s = app.borrow();
        s.tab()
            .selected_entries()
            .first()
            .filter(|e| e.is_dir())
            .map(|e| PathBuf::from(&e.path))
    };
    if let Some(path) = path {
        ui.set_busy(true);
        ui.set_busy_text(SharedString::from("Đang tính kích thước thư mục…"));
        app.borrow().worker.submit(Job::FolderSize { path });
    }
}

// ---------------------------------------------------------------- pumps

/// Hoàn tác thao tác gần nhất.
///
/// Chỉ ghi lại vào ngăn xếp làm lại khi thao tác ngược thật sự thành công: nếu
/// thất bại, mục đó biến mất khỏi lịch sử thay vì lặp lại lỗi mãi.
pub fn undo_last(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let action = app.borrow_mut().undo.take_undo();
    let Some(action) = action else {
        // Phân biệt "chưa làm gì" với "đã hoàn tác hết": người dùng cần biết
        // có thể bấm Ctrl+Y thay vì tưởng tính năng hỏng.
        let hint = if app.borrow().undo.can_redo() {
            "Đã hoàn tác hết — Ctrl+Y để làm lại"
        } else {
            "Không có gì để hoàn tác"
        };
        ui.set_status_tertiary(SharedString::from(hint));
        return;
    };
    match crate::undo::undo(&action) {
        Ok(()) => {
            ui.set_status_tertiary(SharedString::from(action.undo_label().as_str()));
            app.borrow_mut().undo.undone(action);
        }
        Err(e) => {
            logging::write(&format!("hoàn tác lỗi [{}]: {}", e.code, e.message));
            let text = format::explain_error(&e.code, &e.message);
            ui.set_error_text(SharedString::from(text.as_str()));
        }
    }
    reload(ui, app);
}

/// Làm lại thao tác vừa hoàn tác.
pub fn redo_last(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let action = app.borrow_mut().undo.take_redo();
    let Some(action) = action else {
        let hint = if app.borrow().undo.can_undo() {
            "Đã làm lại hết — Ctrl+Z để hoàn tác"
        } else {
            "Không có gì để làm lại"
        };
        ui.set_status_tertiary(SharedString::from(hint));
        return;
    };
    match crate::undo::redo(&action) {
        Ok(()) => {
            ui.set_status_tertiary(SharedString::from(action.redo_label().as_str()));
            app.borrow_mut().undo.redone(action);
        }
        Err(e) => {
            logging::write(&format!("làm lại lỗi [{}]: {}", e.code, e.message));
            let text = format::explain_error(&e.code, &e.message);
            ui.set_error_text(SharedString::from(text.as_str()));
        }
    }
    reload(ui, app);
}

/// Tìm đệ quy dưới thư mục hiện tại.
///
/// Tìm ngay trong thư mục đã có sẵn (bộ lọc nhanh); đây là bước mở rộng xuống
/// các thư mục con, có giới hạn độ sâu để không quét cả ổ đĩa.
pub fn search_recursive(ui: &AppWindow, app: &Rc<RefCell<App>>, query: &str) {
    let q = query.trim().to_string();
    if q.is_empty() {
        ui.set_status_tertiary(SharedString::from("Gõ tên cần tìm trước"));
        return;
    }
    let (root, token) = {
        let s = app.borrow();
        (s.tab().current().to_path_buf(), s.load_token)
    };
    ui.set_busy(true);
    ui.set_busy_text(SharedString::from("Đang tìm trong thư mục con…"));
    app.borrow().worker.submit(Job::Search {
        root,
        query: q,
        max_depth: 8,
        limit: 500,
        token,
    });
}
