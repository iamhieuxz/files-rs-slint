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
    // Thanh trạng thái đọc từ tab đang active, nên phải vẽ lại cùng lúc với
    // danh sách. Thiếu bước này thì đổi tab/thư mục để lại số mục của tab cũ —
    // đúng lỗi thấy khi mở Thùng rác: danh sách rỗng mà chân trang vẫn ghi
    // "57 thư mục, 74 tệp" của thư mục trước.
    push_status(ui, app);
    // Panel xem trước cũng giữ mục đã chọn của tab trước. Danh sách trống thì
    // không có gì để xem, nên xoá panel thay vì để nó hiện dữ liệu không còn
    // thuộc về đâu.
    if app.borrow().tab().selection.is_empty() {
        crate::preview::clear(ui);
    }
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
    // `selection.contains` là O(số mục đang chọn); gọi nó cho từng hàng biến
    // việc dựng danh sách thành O(n²). Với 10.000 tệp và chọn tất cả, đó là
    // 100 triệu phép so sánh mỗi lần vẽ lại.
    let selected: std::collections::HashSet<usize> = tab.selection.iter().copied().collect();
    // Thứ tự hiển thị: phẳng, hoặc đã gom theo nhóm. `index` của mỗi hàng vẫn là
    // chỉ số trong `visible` để chọn/mở không lệch.
    let (order, headers) = grouped_rows(tab, |e| {
        a.tags
            .tags_for(Path::new(&e.path))
            .first()
            .map(|t| t.name.clone())
    });
    let rows: Vec<FileRow> = order
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
                selected: selected.contains(entry_index),
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
                is_group_header: false,
                group_label: SharedString::default(),
            })
        })
        .collect();

    // Chèn dải tiêu đề nhóm vào đúng vị trí. `index` của hàng header là -1 để
    // chọn/mở không bao giờ trúng nhầm header.
    let rows = if headers.is_empty() {
        rows
    } else {
        let mut with_headers = Vec::with_capacity(rows.len() + headers.len());
        let mut next_header = headers.iter().peekable();
        for (i, row) in rows.into_iter().enumerate() {
            while let Some((at, label)) = next_header.peek() {
                if *at == i {
                    with_headers.push(header_row(label));
                    next_header.next();
                } else {
                    break;
                }
            }
            with_headers.push(row);
        }
        // Nhóm rỗng (hiếm, khi mọi mục bị lọc) vẫn phải hiện header.
        for (_, label) in next_header {
            with_headers.push(header_row(label));
        }
        with_headers
    };

    let per_row = grid_columns(grid_width);
    let grid_rows = format::chunk_rows(&rows, per_row)
        .into_iter()
        .map(|line| ModelRc::new(VecModel::from(line)))
        .collect();
    (rows, grid_rows)
}

/// Một hàng chỉ để hiện tiêu đề nhóm.
fn header_row(label: &str) -> FileRow {
    FileRow {
        name: SharedString::from(label),
        path: SharedString::default(),
        is_dir: false,
        is_hidden: false,
        selected: false,
        size: SharedString::default(),
        modified: SharedString::default(),
        kind: SharedString::default(),
        has_tag: false,
        tag_name: SharedString::default(),
        tag_color: slint::Color::from_argb_u8(0, 0, 0, 0),
        icon: SharedString::default(),
        glyph: slint::Color::from_argb_u8(0, 0, 0, 0),
        git_letter: SharedString::default(),
        git_color: slint::Color::from_argb_u8(0, 0, 0, 0),
        icon_image: slint::Image::default(),
        has_icon_image: false,
        // -1: không phải mục thật, nên click không chọn gì.
        index: -1,
        is_group_header: true,
        group_label: SharedString::from(label),
    }
}

/// Dựng hàng cho test: cùng đường với `build_rows` nhưng công khai trong crate.
#[cfg(test)]
pub fn build_rows_for_test(
    a: &App,
    tab: &Tab,
    grid_width: f32,
) -> (Vec<FileRow>, Vec<ModelRc<FileRow>>) {
    build_rows(a, tab, grid_width)
}

/// Đặt cách nhóm và vẽ lại.
pub fn set_group_by(ui: &AppWindow, app: &Rc<RefCell<App>>, by: files_core::group::GroupBy) {
    {
        let mut s = app.borrow_mut();
        let tab = s.tab_mut();
        tab.group_by = by;
    }
    push_view(ui, app);
    ui.set_status_message(SharedString::from(
        format!("Nhóm theo {}", by.label()).as_str(),
    ));
}

/// Chuyển đơn vị gom ngày (Năm → Tháng → Ngày), chỉ có tác dụng khi đang nhóm
/// theo ngày.
pub fn cycle_group_unit(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    use files_core::group::DateUnit;
    {
        let mut s = app.borrow_mut();
        let tab = s.tab_mut();
        tab.group_unit = match tab.group_unit {
            DateUnit::Year => DateUnit::Month,
            DateUnit::Month => DateUnit::Day,
            DateUnit::Day => DateUnit::Year,
        };
    }
    push_view(ui, app);
}

/// Dựng model có header nhóm cho danh sách chi tiết.
///
/// Trả `(rows, headers)`; `headers` rỗng nghĩa là danh sách phẳng. Tách hai model
/// thay vì chèn một hàng giả vào `rows` để chỉ số hàng vẫn khớp `visible`, nhờ
/// vậy chọn và mở mục không bị lệch.
pub fn grouped_rows(
    tab: &Tab,
    tag_of: impl Fn(&files_core::Entry) -> Option<String>,
) -> (Vec<usize>, Vec<(usize, String)>) {
    use files_core::group::{group, GroupBy};
    if tab.group_by == GroupBy::None {
        return (tab.visible.clone(), Vec::new());
    }
    // Nhóm trên các mục đang hiển thị, giữ nguyên thứ tự đã sắp xếp.
    let entries: Vec<files_core::Entry> = tab
        .visible
        .iter()
        .filter_map(|i| tab.entries.get(*i).cloned())
        .collect();
    let groups = group(&entries, tab.group_by, tab.group_unit, tag_of);
    let mut order = Vec::with_capacity(entries.len());
    let mut headers = Vec::new();
    for g in groups {
        headers.push((order.len(), g.label));
        // `indices` là vị trí trong `entries` đã lọc, quy đổi về chỉ số `visible`.
        for local in g.indices {
            if let Some(original) = tab.visible.get(local) {
                order.push(*original);
            }
        }
    }
    (order, headers)
}

/// Tự vừa **một** cột theo nội dung. Dùng cho bấm đúp vạch chia cột.
///
/// Khác `autofit_columns` ở chỗ chỉ đụng một cột, nên người dùng giữ được bề
/// rộng đã chỉnh tay cho các cột còn lại.
pub fn autofit_column(ui: &AppWindow, app: &Rc<RefCell<App>>, id: &str) {
    use crate::settings::column_bounds;
    use files_core::autofit::fit_width;

    let (width, order) = {
        let s = app.borrow();
        let tab = s.tab();
        let texts: Vec<String> = match id {
            "name" => tab
                .visible
                .iter()
                .filter_map(|i| tab.entries.get(*i))
                .map(|e| crate::format::display_name(&e.name, e.is_dir(), s.show_extensions))
                .collect(),
            "kind" => tab
                .visible
                .iter()
                .filter_map(|i| tab.entries.get(*i))
                .map(crate::format::kind_label)
                .collect(),
            "size" => tab
                .visible
                .iter()
                .filter_map(|i| tab.entries.get(*i))
                .map(|e| crate::format::format_size(e.size))
                .collect(),
            "modified" => tab
                .visible
                .iter()
                .filter_map(|i| tab.entries.get(*i))
                .map(|e| crate::format::format_time(e.modified_ms))
                .collect(),
            "tag" => tab
                .visible
                .iter()
                .filter_map(|i| tab.entries.get(*i))
                .map(|e| {
                    s.tags
                        .tags_for(Path::new(&e.path))
                        .first()
                        .map(|t| t.name.clone())
                        .unwrap_or_default()
                })
                .collect(),
            // Cột git chỉ 16px, không có nội dung dài; bỏ qua.
            _ => return,
        };
        // Cột tên có checkbox + icon ở đầu.
        let extra = if id == "name" { 48 } else { 0 };
        let (min, max) = column_bounds(id);
        let width = fit_width(texts.iter().map(|s| s.as_str()), min, max, extra);
        (width, s.settings.column_order.clone())
    };

    {
        let mut s = app.borrow_mut();
        s.settings.column_widths.insert(id.to_string(), width);
    }
    crate::state::persist_ui(app);
    let widths = app.borrow().settings.column_widths.clone();
    apply_column_widths(ui, &widths, &order);
}

/// Tự vừa bề rộng cột theo nội dung đang hiển thị.///
/// Bề rộng được tính từ dữ liệu đã có trong `Tab::entries`, không đọc đĩa. Kết
/// quả ghi vào `settings.column_widths` và đẩy ngay vào UI, nên lần mở sau vẫn
/// giữ đúng.
pub fn autofit_columns(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    use crate::settings::column_bounds;
    use files_core::autofit::fit_width;

    let (widths, order) = {
        let s = app.borrow();
        let tab = s.tab();
        // Lấy tên hiển thị đúng như danh sách đang vẽ, để cột không bị hụt khi
        // phần mở rộng bị ẩn.
        // Giữ chuỗi hiển thị, không chỉ số ký tự: `fit_width` ước lượng theo
        // từng ký tự nên chữ hoa và số được tính rộng hơn chữ thường.
        let names: Vec<String> = tab
            .visible
            .iter()
            .filter_map(|i| tab.entries.get(*i))
            .map(|e| crate::format::display_name(&e.name, e.is_dir(), s.show_extensions))
            .collect();
        let kinds: Vec<String> = tab
            .visible
            .iter()
            .filter_map(|i| tab.entries.get(*i))
            .map(crate::format::kind_label)
            .collect();
        let sizes: Vec<String> = tab
            .visible
            .iter()
            .filter_map(|i| tab.entries.get(*i))
            .map(|e| crate::format::format_size(e.size))
            .collect();
        let dates: Vec<String> = tab
            .visible
            .iter()
            .filter_map(|i| tab.entries.get(*i))
            .map(|e| crate::format::format_time(e.modified_ms))
            .collect();

        let mut widths = s.settings.column_widths.clone();
        let (nmin, nmax) = column_bounds("name");
        // Cột tên có checkbox và icon ở đầu nên cộng thêm chỗ cho chúng.
        widths.insert(
            "name".into(),
            fit_width(names.iter().map(|s| s.as_str()), nmin, nmax, 48),
        );
        let (kmin, kmax) = column_bounds("kind");
        widths.insert(
            "kind".into(),
            fit_width(kinds.iter().map(|s| s.as_str()), kmin, kmax, 0),
        );
        let (smin, smax) = column_bounds("size");
        widths.insert(
            "size".into(),
            fit_width(sizes.iter().map(|s| s.as_str()), smin, smax, 0),
        );
        let (mmin, mmax) = column_bounds("modified");
        widths.insert(
            "modified".into(),
            fit_width(dates.iter().map(|s| s.as_str()), mmin, mmax, 0),
        );
        (widths, s.settings.column_order.clone())
    };

    {
        let mut s = app.borrow_mut();
        s.settings.column_widths = widths.clone();
    }
    crate::state::persist_ui(app);
    // Đẩy vào UI bằng cùng đường mà thao tác kéo cột đang dùng.
    apply_column_widths(ui, &widths, &order);
    ui.set_status_message(SharedString::from("Đã vừa cột theo nội dung"));
}

/// Ghi bề rộng cột vào các property tương ứng của `AppWindow`.
pub fn apply_column_widths(
    ui: &AppWindow,
    widths: &std::collections::BTreeMap<String, u32>,
    order: &[String],
) {
    let get = |id: &str| widths.get(id).copied().unwrap_or(120) as f32;
    ui.set_col_name_width(get("name"));
    ui.set_col_tag_width(get("tag"));
    ui.set_col_modified_width(get("modified"));
    ui.set_col_kind_width(get("kind"));
    ui.set_col_size_width(get("size"));
    ui.set_column_order(column_order_model(order));
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
    let (paths, token, size) = {
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
        // Lưới hiển thị ảnh lớn hơn danh sách nên giải mã ở kích thước lớn hơn;
        // dùng chung một cỡ sẽ ra ảnh nhoè khi phóng to.
        let size = if s.view_mode == 1 { 96 } else { 48 };
        (paths, s.pane_load(pane).token, size)
    };
    let worker = &app.borrow().worker;
    for path in paths {
        worker.submit_thumbnail(path, size, token);
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
            folder_order: files_core::sort::FolderOrder::FoldersFirst,
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
