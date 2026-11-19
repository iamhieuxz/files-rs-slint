//! Selection: single click, Ctrl-click, Shift-range, and keyboard movement.
//!
//! Selection is stored as indices into `Tab::entries`, never into the filtered
//! `visible` list, so a filter change cannot silently retarget an operation.

use crate::preview;
use crate::slint_generated::*;
use crate::state::{App, Pane};
use crate::view::{grid_columns, push_status};
use slint::{Model, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

pub fn select_row(ui: &AppWindow, app: &Rc<RefCell<App>>, visible_index: usize, mode: i32) {
    {
        let mut s = app.borrow_mut();
        let tab = s.tab_mut();
        let Some(entry_index) = tab.visible.get(visible_index).copied() else {
            return;
        };
        match mode {
            // Shift: chọn cả khoảng từ mốc neo tới mục vừa bấm.
            1 => {
                let anchor = tab.anchor.unwrap_or(entry_index);
                let lo = anchor.min(entry_index);
                let hi = anchor.max(entry_index);
                tab.selection = tab
                    .visible
                    .iter()
                    .copied()
                    .filter(|i| *i >= lo && *i <= hi)
                    .collect();
            }
            // Ctrl: bật/tắt riêng mục này, giữ nguyên phần còn lại.
            2 => {
                if let Some(pos) = tab.selection.iter().position(|i| *i == entry_index) {
                    tab.selection.remove(pos);
                } else {
                    tab.selection.push(entry_index);
                }
                tab.anchor = Some(entry_index);
            }
            // Bấm thường: chọn duy nhất mục này.
            _ => {
                tab.selection = vec![entry_index];
                tab.anchor = Some(entry_index);
            }
        }
    }
    // Cập nhật TẠI CHỖ, không dựng lại model: `push_view` thay cả `VecModel`,
    // làm Slint phá và tạo lại mọi `TouchArea`. Khi đó cú click thứ hai rơi
    // vào một phần tử mới nên `double-clicked` không bao giờ chạy — đó chính
    // là lý do double-click không mở được thư mục/tệp.
    update_selection_flags(ui, app);
    push_status(ui, app);

    // Preview follows the last clicked row.
    let entry = {
        let s = app.borrow();
        let tab = s.tab();
        tab.visible
            .get(visible_index)
            .and_then(|i| tab.entries.get(*i))
            .cloned()
    };
    match entry {
        Some(e) => {
            preview::show(ui, &e);
            crate::panes::queue_preview(app, &e);
        }
        None => preview::clear(ui),
    }
}

/// Flip the `selected` flag on the existing row models.
///
/// `set_row_data` mutates one row in place, so the `TouchArea` instances and
/// their double-click state survive the click.
pub fn update_selection_flags(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let a = app.borrow();
    let tab = a.tab();
    let (rows, grid, grid_width) = match a.active_pane {
        Pane::Left => (ui.get_rows(), ui.get_grid_rows(), a.grid_width),
        Pane::Right => (
            ui.get_right_rows(),
            ui.get_right_grid_rows(),
            a.right_grid_width,
        ),
    };
    for (visible_index, entry_index) in tab.visible.iter().enumerate() {
        let selected = tab.selection.contains(entry_index);
        if let Some(mut row) = rows.row_data(visible_index) {
            if row.selected != selected {
                row.selected = selected;
                rows.set_row_data(visible_index, row);
            }
        }
    }

    // Grid là model lồng nhau: cập nhật từng hàng một.
    let per_row = grid_columns(grid_width);
    for (row_index, line) in tab.visible.chunks(per_row).enumerate() {
        if let Some(line_model) = grid.row_data(row_index) {
            if let Some(inner) = line_model.as_any().downcast_ref::<VecModel<FileRow>>() {
                for (col, entry_index) in line.iter().enumerate() {
                    let selected = tab.selection.contains(entry_index);
                    if let Some(mut tile) = inner.row_data(col) {
                        if tile.selected != selected {
                            tile.selected = selected;
                            inner.set_row_data(col, tile);
                        }
                    }
                }
            }
        }
    }

    // Nút "Giải nén" phụ thuộc lựa chọn hiện tại.
    let is_archive = tab.selection.len() == 1
        && tab
            .selected_entries()
            .first()
            .and_then(|e| e.extension.as_deref())
            .map(|x| x.eq_ignore_ascii_case("zip"))
            .unwrap_or(false);
    // `selection-count` điều khiển trạng thái bật/tắt của các nút trên command
    // bar (Copy, Cut, Delete, Nén…). Quên cập nhật nó thì các nút đứng im ở
    // trạng thái disable dù dòng đã được chọn.
    let count = tab.selection.len() as i32;
    drop(a);
    ui.set_selection_count(count);
    ui.set_selection_is_archive(is_archive);
}

pub fn first_selected_visible(app: &Rc<RefCell<App>>) -> Option<usize> {
    let s = app.borrow();
    let tab = s.tab();
    tab.visible.iter().position(|i| tab.selection.contains(i))
}

// ---------------------------------------------------------------- command palette

pub fn move_selection(ui: &AppWindow, app: &Rc<RefCell<App>>, down: bool) {
    let next = {
        let s = app.borrow();
        let tab = s.tab();
        let cur = first_selected_visible(app);
        match (cur, down) {
            (None, _) => Some(0usize),
            (Some(i), true) => {
                if i + 1 < tab.visible.len() {
                    Some(i + 1)
                } else {
                    Some(i)
                }
            }
            (Some(i), false) => Some(i.saturating_sub(1)),
        }
    };
    if let Some(i) = next {
        select_row(ui, app, i, 0);
    }
}
