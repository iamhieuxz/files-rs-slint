//! Rendering the list and status bar from the current state.
//!
//! Everything here is "state in, pixels out": it reads [`App`] and pushes plain
//! values into the Slint model. No file operations and no navigation live in this
//! module, so it can be reasoned about without a filesystem.

use crate::format;
use crate::slint_generated::*;
use crate::state::{App, Pane, Tab};
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
    let pane = app.borrow().active_pane;
    push_pane(ui, app, pane);
    push_chrome(ui, app);
}

/// Rebuild both panes and the shared chrome. Use this for a global display
/// choice (view mode, extensions) that affects both panes at once.
pub fn push_all(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    push_pane(ui, app, Pane::Left);
    if app.borrow().is_dual() {
        push_pane(ui, app, Pane::Right);
    }
    push_chrome(ui, app);
}

/// Build and push the row models of one pane.
///
/// The two panes own separate models so selection, scroll and hover in one pane
/// can never rebuild the other pane's rows.
pub fn push_pane(ui: &AppWindow, app: &Rc<RefCell<App>>, pane: Pane) {
    let a = app.borrow();
    let Some(tab) = a.tab_for(pane) else { return };
    let grid_width = match pane {
        Pane::Left => a.grid_width,
        Pane::Right => a.right_grid_width,
    };
    let (rows, grid_rows) = build_rows(&a, tab, grid_width);
    match pane {
        Pane::Left => {
            ui.set_rows(ModelRc::new(VecModel::from(rows)));
            ui.set_grid_rows(ModelRc::new(VecModel::from(grid_rows)));
            ui.set_left_path(SharedString::from(tab.current().to_string_lossy().as_ref()));
        }
        Pane::Right => {
            ui.set_right_rows(ModelRc::new(VecModel::from(rows)));
            ui.set_right_grid_rows(ModelRc::new(VecModel::from(grid_rows)));
            ui.set_right_path(SharedString::from(tab.current().to_string_lossy().as_ref()));
            ui.set_right_sort_column(SharedString::from(sort_id(tab)));
            ui.set_right_sort_desc(tab.sort.direction == SortDirection::Descending);
        }
    }
}

fn sort_id(tab: &Tab) -> &'static str {
    match tab.sort.by {
        SortBy::Size => "size",
        SortBy::DateModified => "modified",
        SortBy::Kind => "kind",
        _ => "name",
    }
}

fn build_rows(a: &App, tab: &Tab, grid_width: f32) -> (Vec<FileRow>, Vec<ModelRc<FileRow>>) {
    let rows: Vec<FileRow> = tab
        .visible
        .iter()
        .enumerate()
        .filter_map(|(visible_index, entry_index)| {
            let e = tab.entries.get(*entry_index)?;
            let (icon, default_glyph) = format::icon_for(e);
            let glyph = a
                .tags
                .color_for(Path::new(&e.path))
                .unwrap_or(default_glyph);
            let attached = a.tags.tags_for(Path::new(&e.path));
            let first_tag = attached.first();
            let tag_color = first_tag
                .and_then(|t| crate::tags::parse_hex(&t.color))
                .unwrap_or(0);
            Some(FileRow {
                name: SharedString::from(
                    format::display_name(&e.name, e.is_dir(), a.show_extensions).as_str(),
                ),
                path: SharedString::from(e.path.as_str()),
                is_dir: e.is_dir(),
                is_hidden: e.is_hidden,
                selected: tab.selection.contains(entry_index),
                size: SharedString::from(format::format_size(e.size).as_str()),
                modified: SharedString::from(format::format_time(e.modified_ms).as_str()),
                kind: SharedString::from(format::kind_label(e).as_str()),
                has_tag: first_tag.is_some(),
                tag_name: SharedString::from(first_tag.map(|t| t.name.as_str()).unwrap_or("")),
                tag_color: color_from_u32(tag_color),
                icon: SharedString::from(icon),
                glyph: color_from_u32(glyph),
                git_letter: SharedString::from(
                    a.git_status
                        .get(Path::new(&e.path))
                        .map(|g| g.letter())
                        .unwrap_or(""),
                ),
                git_color: color_from_u32(
                    a.git_status
                        .get(Path::new(&e.path))
                        .map(|g| g.color())
                        .unwrap_or(0),
                ),
                icon_image: slint::Image::default(),
                has_icon_image: false,
                index: visible_index as i32,
            })
        })
        .collect();

    let per_row = grid_columns(grid_width);
    let grid_rows = format::chunk_rows(&rows, per_row)
        .into_iter()
        .map(|line| ModelRc::new(VecModel::from(line)))
        .collect();
    (rows, grid_rows)
}

/// Update the chrome shared by both panes (address bar, status, sort indicator).
pub fn push_chrome(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let a = app.borrow();
    let tab = a.tab();
    ui.set_address_text(SharedString::from(tab.current().to_string_lossy().as_ref()));
    ui.set_left_path(SharedString::from(
        a.tab_for(Pane::Left)
            .map(|t| t.current().to_string_lossy().into_owned())
            .unwrap_or_default()
            .as_str(),
    ));
    ui.set_right_path(SharedString::from(
        a.tab_for(Pane::Right)
            .map(|t| t.current().to_string_lossy().into_owned())
            .unwrap_or_default()
            .as_str(),
    ));
    ui.set_can_back(tab.history.can_back());
    ui.set_can_forward(tab.history.can_forward());
    ui.set_selection_count(tab.selection.len() as i32);
    let is_archive = tab.selection.len() == 1
        && tab
            .selected_entries()
            .first()
            .and_then(|e| e.extension.as_deref())
            .map(|x| x.eq_ignore_ascii_case("zip"))
            .unwrap_or(false);
    ui.set_selection_is_archive(is_archive);
    ui.set_sort_column(SharedString::from(sort_id(tab)));
    ui.set_sort_desc(tab.sort.direction == SortDirection::Descending);

    let crumb_paths = breadcrumbs(tab.current());
    let labels: Vec<String> = crumb_paths.iter().map(|c| c.label.clone()).collect();
    ui.set_crumb_text(SharedString::from(format::crumb_line(&labels).as_str()));
    let widths = format::layout_crumbs(&labels);
    let last = crumb_paths.len().saturating_sub(1);
    let crumbs: Vec<CrumbInfo> = crumb_paths
        .into_iter()
        .enumerate()
        .map(|(i, c)| CrumbInfo {
            label: SharedString::from(c.label.as_str()),
            path: SharedString::from(c.path.as_str()),
            w: if i < last {
                widths[i] + 12.0
            } else {
                widths[i]
            },
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

/// Whether a width change needs the grid model rebuilt.
///
/// Slint reports every pixel while the user drags a window; rebuilding the rows
/// on each one is wasteful. The tile size is fixed, so only a column-count
/// change can alter the chunking.
pub fn grid_width_change_needs_view(old_width: f32, new_width: f32, view_mode: i32) -> bool {
    view_mode == 1 && grid_columns(old_width) != grid_columns(new_width)
}

/// Model cho thứ tự cột trong details view.
pub fn column_order_model(order: &[String]) -> ModelRc<SharedString> {
    let items: Vec<SharedString> = order
        .iter()
        .map(|id| SharedString::from(id.as_str()))
        .collect();
    ModelRc::new(VecModel::from(items))
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
pub fn apply_shell_icon(
    ui: &AppWindow,
    app: &Rc<RefCell<App>>,
    pane: Pane,
    path: &Path,
    image: slint::Image,
) {
    let a = app.borrow();
    let Some(tab) = a.tab_for(pane) else { return };
    let path_str = path.to_string_lossy();
    let Some(visible_index) = tab
        .visible
        .iter()
        .position(|i| tab.entries.get(*i).map(|e| e.path.as_str()) == Some(path_str.as_ref()))
    else {
        return;
    };

    let (rows, grid, grid_width) = match pane {
        Pane::Left => (ui.get_rows(), ui.get_grid_rows(), a.grid_width),
        Pane::Right => (
            ui.get_right_rows(),
            ui.get_right_grid_rows(),
            a.right_grid_width,
        ),
    };
    if let Some(mut row) = rows.row_data(visible_index) {
        row.icon_image = image.clone();
        row.has_icon_image = true;
        rows.set_row_data(visible_index, row);
    }

    let per_row = grid_columns(grid_width);
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
pub fn queue_shell_icons(app: &Rc<RefCell<App>>, pane: Pane) {
    let (paths, token) = {
        let s = app.borrow();
        let Some(tab) = s.tab_for(pane) else { return };
        let paths: Vec<PathBuf> = tab
            .entries
            .iter()
            .filter(|e| files_fs::wants_shell_icon(Path::new(&e.path)))
            .map(|e| PathBuf::from(&e.path))
            .collect();
        (paths, s.pane_load(pane).token)
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
pub fn queue_folder_sizes(app: &Rc<RefCell<App>>, pane: Pane) {
    let (paths, token) = {
        let s = app.borrow();
        let Some(tab) = s.tab_for(pane) else { return };
        let paths: Vec<PathBuf> = tab
            .entries
            .iter()
            .filter(|e| e.is_dir())
            .map(|e| PathBuf::from(&e.path))
            .collect();
        (paths, s.pane_load(pane).token)
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
pub fn fill_size_cell(ui: &AppWindow, app: &Rc<RefCell<App>>, pane: Pane, path: &Path, bytes: u64) {
    let a = app.borrow();
    let Some(tab) = a.tab_for(pane) else { return };
    let path_str = path.to_string_lossy();
    let Some(visible_index) = tab
        .visible
        .iter()
        .position(|i| tab.entries.get(*i).map(|e| e.path.as_str()) == Some(path_str.as_ref()))
    else {
        return;
    };

    let text = SharedString::from(format::format_size(Some(bytes)).as_str());
    let (rows, grid, grid_width) = match pane {
        Pane::Left => (ui.get_rows(), ui.get_grid_rows(), a.grid_width),
        Pane::Right => (
            ui.get_right_rows(),
            ui.get_right_grid_rows(),
            a.right_grid_width,
        ),
    };
    if let Some(mut row) = rows.row_data(visible_index) {
        row.size = text.clone();
        rows.set_row_data(visible_index, row);
    }

    // Grid view shares the same entries; keep it in sync too.
    let per_row = grid_columns(grid_width);
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

/// Hỏi ảnh thu nhỏ cho các tệp ảnh đang hiển thị.
///
/// Chỉ ảnh mới được hỏi: giải mã một tệp văn bản để tìm ảnh thu nhỏ là lãng phí,
/// và với một thư mục 5000 tệp thì đó là hàng nghìn lời gọi vô ích.
pub fn queue_thumbnails(app: &Rc<RefCell<App>>, pane: Pane) {
    let (paths, token) = {
        let s = app.borrow();
        let Some(tab) = s.tab_for(pane) else { return };
        let paths: Vec<std::path::PathBuf> = tab
            .entries
            .iter()
            .filter(|e| {
                e.extension
                    .as_deref()
                    .map(|x| x.to_ascii_lowercase())
                    .is_some_and(|x| {
                        matches!(
                            x.as_str(),
                            "jpg"
                                | "jpeg"
                                | "png"
                                | "bmp"
                                | "gif"
                                | "tif"
                                | "tiff"
                                | "webp"
                                | "ico"
                        )
                    })
            })
            .map(|e| std::path::PathBuf::from(&e.path))
            .collect();
        (paths, s.pane_load(pane).token)
    };
    let worker = &app.borrow().worker;
    for path in paths {
        worker.submit_thumbnail(path, 48, token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_listing_produces_no_rows() {
        // Trang thai rong cua UI phu thuoc vao `rows.length == 0`; neu mot thu
        // muc rong lai sinh ra mot dong thi thong bao se khong bao gio hien.
        use crate::state::{App, Pane, Tab};
        let (worker, results) = crate::ops_worker::Worker::spawn().expect("worker");
        let app = App {
            tabs: vec![Tab::new(std::path::PathBuf::from("C:\\"))],
            active: 0,
            right: None,
            active_pane: Pane::Left,
            loads: Default::default(),
            next_load_token: 1,
            show_hidden: false,
            show_sidebar: true,
            show_preview: true,
            view_mode: 0,
            dark: false,
            clipboard: Vec::new(),
            clipboard_cut: false,
            grid_width: 900.0,
            right_grid_width: 900.0,
            worker,
            results,
            watch: None,
            watched_path: None,
            watch_dirty: false,
            closed_tabs: Vec::new(),
            show_extensions: true,
            folders_first: true,
            tags: crate::tags::TagStore::default(),
            tag_path: std::path::PathBuf::from("C:\\t.json"),
            settings: crate::settings::Settings::default(),
            settings_path: std::path::PathBuf::from("C:\\s.json"),
            undo: Default::default(),
            git_status: Default::default(),
            open_with: Vec::new(),
            operation_started: None,
            drag: Default::default(),
        };
        // Tab chua doc lan nao: khong co entry nao.
        assert!(app.tab().entries.is_empty());
        assert!(app.tab().visible.is_empty());
    }

    #[test]
    fn grid_rebuilds_only_when_the_column_count_changes() {
        // 300 và 400 px đều xếp 2 cột; 420 px thành 3 cột.
        assert!(!grid_width_change_needs_view(300.0, 400.0, 1));
        assert!(grid_width_change_needs_view(300.0, 420.0, 1));
        // Đang ở details: chưa cần dựng lại lưới.
        assert!(!grid_width_change_needs_view(300.0, 420.0, 0));
    }

    #[test]
    fn grid_columns_never_returns_zero() {
        // A zero column count would divide by zero when chunking rows.
        assert_eq!(grid_columns(0.0), 1);
        assert_eq!(grid_columns(-50.0), 1);
        assert_eq!(grid_columns(132.0), 1);
        assert_eq!(grid_columns(280.0), 2);
    }
}
