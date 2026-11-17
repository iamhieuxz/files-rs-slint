//! File operations: clipboard, paste, compress, extract, hash, folder size.
//!
//! Every operation is submitted to the worker and reported back as a status line
//! or an error; nothing here blocks the UI thread. This is also where an undo
//! stack would hook in later, which is why the operations are gathered in one
//! module instead of scattered through the callbacks.

use crate::slint_generated::*;
use crate::state::App;
use crate::ops_worker;
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

pub fn do_paste(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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
        Job::Move { sources, target }
    } else {
        Job::Copy { sources, target }
    };
    app.borrow().worker.submit(job);
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
