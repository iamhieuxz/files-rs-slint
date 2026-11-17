//! Rendering the list and status bar from the current state.
//!
//! Everything here is "state in, pixels out": it reads [`App`] and pushes plain
//! values into the Slint model. No file operations and no navigation live in this
//! module, so it can be reasoned about without a filesystem.

use crate::format;
use crate::slint_generated::*;
use crate::state::App;
use files_core::{breadcrumbs, SortBy, SortDirection};
use slint::{Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;


pub fn color_from_u32(rgb: u32) -> slint::Color {
    slint::Color::from_rgb_u8(
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
    )
}

// ---------------------------------------------------------------- loading

pub fn push_view(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let a = app.borrow();
    let tab = a.tab();
    let rows: Vec<FileRow> = tab
        .visible
        .iter()
        .enumerate()
        .filter_map(|(visible_index, entry_index)| {
            let e = tab.entries.get(*entry_index)?;
            let (icon, default_glyph) = format::icon_for(e);
            // Có thẻ thì icon mang màu của thẻ: đây chính là "đổi màu" mà người
            // dùng thấy, với cả tệp lẫn thư mục.
            let glyph = a
                .tags
                .color_for(Path::new(&e.path))
                .unwrap_or(default_glyph);
            Some(FileRow {
                name: SharedString::from(format::display_name(&e.name, e.is_dir(), a.show_extensions).as_str()),
                path: SharedString::from(e.path.as_str()),
                is_dir: e.is_dir(),
                is_hidden: e.is_hidden,
                selected: tab.selection.contains(entry_index),
                size: SharedString::from(format::format_size(e.size).as_str()),
                modified: SharedString::from(format::format_time(e.modified_ms).as_str()),
                kind: SharedString::from(format::kind_label(e).as_str()),
                icon: SharedString::from(icon),
                glyph: color_from_u32(glyph),
                // Icon thật của Windows cho loại tệp có (exe, msi, lnk…).
                // Ảnh được nạp ở luồng nền rồi điền vào sau.
                icon_image: slint::Image::default(),
                has_icon_image: false,
                index: visible_index as i32,
            })
        })
        .collect();

    // Grid không tự xuống dòng, nên chia hàng ở đây theo bề rộng khả dụng.
    let per_row = grid_columns(a.grid_width);
    let grid_rows: Vec<ModelRc<FileRow>> = format::chunk_rows(&rows, per_row)
        .into_iter()
        .map(|line| ModelRc::new(VecModel::from(line)))
        .collect();

    // Giữ một `Rc<VecModel>` để `update_selection_flags` sửa được từng dòng
    // tại chỗ, không phải thay cả model (xem ghi chú ở hàm đó).
    let rows_model = Rc::new(VecModel::from(rows));
    ui.set_rows(ModelRc::from(rows_model));
    ui.set_grid_rows(ModelRc::new(VecModel::from(grid_rows)));
    ui.set_address_text(SharedString::from(tab.current().to_string_lossy().as_ref()));
    ui.set_can_back(tab.history.can_back());
    ui.set_can_forward(tab.history.can_forward());
    ui.set_selection_count(tab.selection.len() as i32);
    // Nút "Giải nén" chỉ bật khi mục đang chọn thật sự là tệp .zip.
    let is_archive = tab.selection.len() == 1
        && tab
            .selected_entries()
            .first()
            .and_then(|e| e.extension.as_deref())
            .map(|x| x.eq_ignore_ascii_case("zip"))
            .unwrap_or(false);
    ui.set_selection_is_archive(is_archive);
    ui.set_sort_column(SharedString::from(match tab.sort.by {
        SortBy::Size => "size",
        SortBy::DateModified => "modified",
        SortBy::Kind => "kind",
        _ => "name",
    }));
    ui.set_sort_desc(tab.sort.direction == SortDirection::Descending);

    let crumb_paths = breadcrumbs(tab.current());
    let labels: Vec<String> = crumb_paths.iter().map(|c| c.label.clone()).collect();
    // Breadcrumb là một dòng chữ có dấu ngăn cách: Slint không cho phần tử con
    // của layout tự quyết bề rộng, nên từng mảnh rời sẽ đè lên nhau.
    ui.set_crumb_text(SharedString::from(format::crumb_line(&labels).as_str()));
    let widths = format::layout_crumbs(&labels);
    let last = crumb_paths.len().saturating_sub(1);
    let crumbs: Vec<CrumbInfo> = crumb_paths
        .into_iter()
        .enumerate()
        .map(|(i, c)| CrumbInfo {
            label: SharedString::from(c.label.as_str()),
            path: SharedString::from(c.path.as_str()),
            w: if i < last { widths[i] + 12.0 } else { widths[i] },
        })
        .collect();
    ui.set_crumbs(ModelRc::new(VecModel::from(crumbs)));
}

/// How many grid tiles fit across `width`.
///
/// Tile + gap come from the design tokens (132px + 4px); the floor keeps a
/// partial tile from being cut off at the right edge.
pub fn grid_columns(width: f32) -> usize {
    const TILE: f32 = 132.0;
    const GAP: f32 = 4.0;
    const MIN: usize = 1;
    if width <= 0.0 {
        return MIN;
    }
    let cols = ((width + GAP) / (TILE + GAP)).floor() as usize;
    cols.max(MIN)
}

/// Convert a decoded RGBA icon into a Slint image.
pub fn to_slint_image(icon: &files_fs::RgbaIcon) -> slint::Image {
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        &icon.pixels,
        icon.width,
        icon.height,
    );
    slint::Image::from_rgba8(buffer)
}

/// Write a shell icon into the row for `path`, in both views.
pub fn apply_shell_icon(ui: &AppWindow, app: &Rc<RefCell<App>>, path: &Path, image: slint::Image) {
    let a = app.borrow();
    let tab = a.tab();
    let path_str = path.to_string_lossy();
    let Some(visible_index) = tab
        .visible
        .iter()
        .position(|i| tab.entries.get(*i).map(|e| e.path.as_str()) == Some(path_str.as_ref()))
    else {
        return;
    };

    let rows = ui.get_rows();
    if let Some(mut row) = rows.row_data(visible_index) {
        row.icon_image = image.clone();
        row.has_icon_image = true;
        rows.set_row_data(visible_index, row);
    }

    let per_row = grid_columns(a.grid_width);
    let grid = ui.get_grid_rows();
    if let Some(line) = grid.row_data(visible_index / per_row) {
        if let Some(inner) = line.as_any().downcast_ref::<VecModel<FileRow>>() {
            let col = visible_index % per_row;
            if let Some(mut tile) = inner.row_data(col) {
                tile.icon_image = image;
                tile.has_icon_image = true;
                inner.set_row_data(col, tile);
            }
        }
    }
}

/// Ask the worker for real Windows icons for the rows that have one.
///
/// Only formats where a publisher icon exists are requested; see
/// `files_fs::wants_shell_icon`.
pub fn queue_shell_icons(app: &Rc<RefCell<App>>) {
    let (paths, token) = {
        let s = app.borrow();
        let tab = s.tab();
        let paths: Vec<PathBuf> = tab
            .entries
            .iter()
            .filter(|e| files_fs::wants_shell_icon(Path::new(&e.path)))
            .map(|e| PathBuf::from(&e.path))
            .collect();
        (paths, s.load_token)
    };
    let worker = &app.borrow().worker;
    for path in paths {
        worker.submit_icon(path, 20, token);
    }
}

/// Queue a size walk for every visible folder, tagged with the current token.
/// The token is the one from the listing that produced these rows: a reply that
/// arrives after the user navigated away is dropped instead of writing a stale
/// size into a different folder's row.
pub fn queue_folder_sizes(app: &Rc<RefCell<App>>) {
    let (paths, token) = {
        let s = app.borrow();
        let tab = s.tab();
        let paths: Vec<PathBuf> = tab
            .entries
            .iter()
            .filter(|e| e.is_dir())
            .map(|e| PathBuf::from(&e.path))
            .collect();
        (paths, s.load_token)
    };
    let worker = &app.borrow().worker;
    for path in paths {
        worker.submit_size(path, token);
    }
}

/// Write one folder's size into its row without rebuilding the list.
///
/// A full `push_view` would reset every row's `TouchArea` mid-scroll; updating
/// the single cell keeps hover, selection and scroll position intact.
pub fn fill_size_cell(ui: &AppWindow, app: &Rc<RefCell<App>>, path: &Path, bytes: u64) {
    let a = app.borrow();
    let tab = a.tab();
    let path_str = path.to_string_lossy();
    let Some(visible_index) = tab
        .visible
        .iter()
        .position(|i| tab.entries.get(*i).map(|e| e.path.as_str()) == Some(path_str.as_ref()))
    else {
        return;
    };

    let text = SharedString::from(format::format_size(Some(bytes)).as_str());
    let rows = ui.get_rows();
    if let Some(mut row) = rows.row_data(visible_index) {
        row.size = text.clone();
        rows.set_row_data(visible_index, row);
    }

    // Grid view shares the same entries; keep it in sync too.
    let per_row = grid_columns(a.grid_width);
    let grid = ui.get_grid_rows();
    let grid_row = visible_index / per_row;
    let grid_col = visible_index % per_row;
    if let Some(line) = grid.row_data(grid_row) {
        if let Some(inner) = line.as_any().downcast_ref::<VecModel<FileRow>>() {
            if let Some(mut tile) = inner.row_data(grid_col) {
                tile.size = text;
                inner.set_row_data(grid_col, tile);
            }
        }
    }
}

pub fn push_status(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let a = app.borrow();
    let tab = a.tab();
    let folders = tab.entries.iter().filter(|e| e.is_dir()).count();
    let files = tab.entries.len() - folders;
    let selected_bytes: u64 = tab.selected_entries().iter().filter_map(|e| e.size).sum();

    ui.set_status_primary(SharedString::from(
        format!("{folders} thư mục, {files} tệp").as_str(),
    ));
    let secondary = if tab.selection.is_empty() {
        String::new()
    } else {
        format!("Đã chọn {}", tab.selection.len())
    };
    ui.set_status_secondary(SharedString::from(secondary.as_str()));
    let tertiary = if tab.selection.is_empty() {
        String::new()
    } else if selected_bytes > 0 {
        format::format_size(Some(selected_bytes))
    } else {
        String::new()
    };
    ui.set_status_tertiary(SharedString::from(tertiary.as_str()));
}

// ---------------------------------------------------------------- tabs

pub fn refresh_theme(ui: &AppWindow, dark: bool) {
    let theme = ui.global::<Theme>();
    theme.set_dark(dark);
}

// ---------------------------------------------------------------- callbacks

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_columns_never_returns_zero() {
        // A zero column count would divide by zero when chunking rows.
        assert_eq!(grid_columns(0.0), 1);
        assert_eq!(grid_columns(-50.0), 1);
        assert_eq!(grid_columns(132.0), 1);
        assert_eq!(grid_columns(280.0), 2);
    }
}
