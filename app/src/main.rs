// Không mở cửa sổ console: app gốc không có console. Thuộc tính phải đứng
// trước mọi item khác, nên đặt ngay đầu tệp.
#![cfg_attr(windows, windows_subsystem = "windows")]

slint::include_modules!();

mod format;
mod history;
mod logging;
mod ops_worker;
mod preview;
mod tags;
mod window;

use files_core::sort::ListOptions;
use files_core::{breadcrumbs, Entry, SortBy, SortDirection, SortSpec};
use history::History;
use ops_worker::{Job, JobDone, Worker};
use slint::{Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

/// Everything one tab owns.
struct Tab {
    history: History,
    entries: Vec<Entry>,
    /// Indices into `entries` that are currently selected.
    selection: Vec<usize>,
    /// Last row clicked, so Shift+click can extend from it.
    anchor: Option<usize>,
    sort: SortSpec,
    /// Text the user typed into the filter box, lowercased once.
    filter: String,
    /// Row indices after filtering; `entries` stays the source of truth.
    visible: Vec<usize>,
    /// Entries dropped by the hidden filter, reported in the status bar.
    hidden_count: usize,
}

impl Tab {
    fn new(start: PathBuf) -> Self {
        Self {
            history: History::new(start),
            entries: Vec::new(),
            selection: Vec::new(),
            anchor: None,
            sort: SortSpec::default(),
            filter: String::new(),
            visible: Vec::new(),
            hidden_count: 0,
        }
    }

    fn current(&self) -> &Path {
        self.history.current()
    }

    /// Recompute `visible` from `entries` and the active filter.
    fn rebuild_visible(&mut self) {
        self.visible.clear();
        if self.filter.is_empty() {
            self.visible.extend(0..self.entries.len());
        } else {
            for (i, e) in self.entries.iter().enumerate() {
                if e.name.to_lowercase().contains(&self.filter) {
                    self.visible.push(i);
                }
            }
        }
        // Selection can only reference rows the user can see.
        self.selection.retain(|i| self.visible.contains(i));
    }

    fn selected_entries(&self) -> Vec<&Entry> {
        self.selection
            .iter()
            .filter_map(|i| self.entries.get(*i))
            .collect()
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected_entries()
            .into_iter()
            .map(|e| PathBuf::from(&e.path))
            .collect()
    }
}

struct App {
    tabs: Vec<Tab>,
    active: usize,
    show_hidden: bool,
    show_sidebar: bool,
    show_preview: bool,
    view_mode: i32,
    dark: bool,
    clipboard: Vec<PathBuf>,
    clipboard_cut: bool,
    /// Bumped on every navigation so a stale listing reply is ignored.
    load_token: u64,
    /// True while a directory read is in flight, so the watcher cannot stack
    /// another reload on top of it.
    load_in_flight: bool,
    /// Width available to the file list, so the grid can pick a column count.
    grid_width: f32,
    worker: Worker,
    results: Receiver<JobDone>,
    /// Paths whose change should trigger a refresh, from the directory watcher.
    watch: Option<files_fs::WatchHandle>,
    watched_path: Option<PathBuf>,
    /// Tabs the user closed, newest last. `Ctrl+Shift+T` pops from here, which
    /// is what Files and every browser do — the path alone is enough to restore.
    closed_tabs: Vec<PathBuf>,
    /// Hide `.txt`-style extensions in the name column.
    show_extensions: bool,
    /// Group folders above files, or sort them together.
    folders_first: bool,
    /// Thẻ màu của tệp và thư mục, cùng nơi lưu để ghi lại khi thay đổi.
    tags: tags::TagStore,
    tag_path: PathBuf,
}

impl App {
    fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    fn list_options(&self) -> ListOptions {
        ListOptions { show_hidden: self.show_hidden, show_dotfiles: self.show_hidden }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    // Không có console nên stderr mất hút; ghi log ra tệp để còn chẩn đoán.
    logging::install_panic_hook();
    logging::write(&format!(
        "khởi động, log tại {:?}",
        logging::log_path()
    ));

    let ui = AppWindow::new()?;
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\"));

    let (worker, results) = Worker::spawn();
    let app = Rc::new(RefCell::new(App {
        tabs: vec![Tab::new(home.clone())],
        active: 0,
        show_hidden: false,
        show_sidebar: true,
        show_preview: true,
        view_mode: 0,
        dark: false,
        clipboard: Vec::new(),
        clipboard_cut: false,
        load_token: 0,
        load_in_flight: false,
        grid_width: 900.0,
        worker,
        results,
        watch: None,
        watched_path: None,
        closed_tabs: Vec::new(),
        show_extensions: true,
        folders_first: true,
        tags: {
            let path = tag_store_path();
            tags::TagStore::load(&path)
        },
        tag_path: tag_store_path(),
    }));

    build_sidebar(&ui);
    refresh_theme(&ui, false);
    reload(&ui, &app);
    sync_tabs(&ui, &app.borrow());

    wire_callbacks(&ui, &app);
    install_result_pump(&ui, &app);
    install_watch_pump(&ui, &app);

    // Cửa sổ không khung: nút thu nhỏ/phóng/đóng do app tự vẽ, nên phải tự
    // chuyển chúng thành lệnh Win32.
    window::wire(&ui);
    window::install_state_poller(&ui);

    ui.run()
}

/// Where the tag database lives.
///
/// `%LOCALAPPDATA%` is the right home, but a confined profile can refuse writes;
/// in that case the store sits beside the executable, the same fallback the
/// logger uses, so tagging still works instead of silently doing nothing.
fn tag_store_path() -> PathBuf {
    let preferred = tags::TagStore::default_path();
    if let Some(dir) = preferred.parent() {
        if std::fs::create_dir_all(dir).is_ok()
            && std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&preferred)
                .is_ok()
        {
            return preferred;
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .map(|d| d.join("files-rs-tags.json"))
        .unwrap_or(preferred)
}

// ---------------------------------------------------------------- sidebar

fn build_sidebar(ui: &AppWindow) {
    let pinned: Vec<NavEntry> = files_fs::drives::quick_access()
        .into_iter()
        .map(|(label, path)| {
            let (icon, glyph) = format::folder_glyph(&label);
            NavEntry {
                id: SharedString::from(format!("pin:{path}").as_str()),
                label: SharedString::from(label.as_str()),
                path: SharedString::from(path.as_str()),
                icon: SharedString::from(icon),
                glyph: color_from_u32(glyph),
                indent: 0,
                expandable: false,
                expanded: false,
            }
        })
        .collect();
    ui.set_pinned(ModelRc::new(VecModel::from(pinned)));

    let drives: Vec<NavEntry> = files_fs::list_drives()
        .into_iter()
        .map(|d| {
            let label = match &d.name {
                Some(name) if !name.is_empty() => format!("{} ({})", name, d.label),
                _ => d.label.clone(),
            };
            NavEntry {
                id: SharedString::from(format!("drive:{}", d.path).as_str()),
                label: SharedString::from(label.as_str()),
                path: SharedString::from(d.path.as_str()),
                icon: SharedString::from("hard_drive"),
                glyph: color_from_u32(0x6b7280),
                indent: 0,
                expandable: false,
                expanded: false,
            }
        })
        .collect();
    ui.set_drives(ModelRc::new(VecModel::from(drives)));
}

fn color_from_u32(rgb: u32) -> slint::Color {
    slint::Color::from_rgb_u8(
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
    )
}

// ---------------------------------------------------------------- loading

/// Ask the worker for the current directory, and start watching it.
fn reload(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let (path, sort, opts, token) = {
        let mut a = app.borrow_mut();
        a.load_token += 1;
        a.load_in_flight = true;
        let token = a.load_token;
        let path = a.tab().current().to_path_buf();
        let sort = a.tab().sort;
        let opts = a.list_options();
        (path, sort, opts, token)
    };

    ui.set_busy(true);
    ui.set_busy_text(SharedString::from("Đang đọc thư mục…"));
    app.borrow().worker.submit(Job::Load { path, sort, opts, token });
}

/// Point the watcher at `path`, replacing any previous watch.
fn retarget_watch(app: &mut App, path: &Path) {
    if app.watched_path.as_deref() == Some(path) {
        return;
    }
    app.watch = files_fs::watch_directory(path).ok();
    app.watched_path = Some(path.to_path_buf());
}

fn apply_listing(ui: &AppWindow, app: &Rc<RefCell<App>>, listing: files_core::Listing) {
    {
        let mut a = app.borrow_mut();
        let active = a.active;
        let path = a.tabs[active].current().to_path_buf();
        a.tabs[active].entries = listing.entries;
        a.tabs[active].selection.clear();
        a.tabs[active].anchor = None;
        a.tabs[active].rebuild_visible();
        retarget_watch(&mut a, &path);
        a.tabs[active].hidden_count = listing.hidden_count;
    }
    // Dọn thẻ của những mục đã biến mất trong thư mục này, để tệp cấu hình
    // không phình ra và đường dẫn bị tái sử dụng không thừa hưởng màu cũ.
    {
        let mut a = app.borrow_mut();
        let current = a.tab().current().to_path_buf();
        let before = a.tags.assignments.len();
        a.tags.prune_missing(&current);
        if a.tags.assignments.len() != before {
            let path = a.tag_path.clone();
            let store = a.tags.clone();
            drop(a);
            if let Err(e) = store.save(&path) {
                logging::write(&format!("không lưu được thẻ sau khi dọn: {e}"));
            }
        }
    }
    ui.set_error_text(SharedString::default());
    ui.set_busy(false);
    push_view(ui, app);
    push_status(ui, app);

    // Tính kích thước mọi thư mục con ở luồng riêng: người dùng thấy "—" rồi
    // từng ô tự điền, thay vì phải bấm "Tính kích thước" cho từng cái.
    queue_folder_sizes(app);
    // Icon thật của Windows cho exe/msi/lnk… — chạy sau khi bảng đã hiện để
    // không làm chậm lần vẽ đầu.
    queue_shell_icons(app);

    // Bảng thuộc tính của mục đầu tiên: bỏ trống thì panel trông như hỏng.
    let first = {
        let s = app.borrow();
        let tab = s.tab();
        tab.visible
            .first()
            .and_then(|i| tab.entries.get(*i))
            .cloned()
    };
    if let Some(entry) = first {
        preview::show(ui, &entry);
    } else {
        preview::clear(ui);
    }
}

fn push_view(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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
fn grid_columns(width: f32) -> usize {
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
fn to_slint_image(icon: &files_fs::RgbaIcon) -> slint::Image {
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        &icon.pixels,
        icon.width,
        icon.height,
    );
    slint::Image::from_rgba8(buffer)
}

/// Write a shell icon into the row for `path`, in both views.
fn apply_shell_icon(ui: &AppWindow, app: &Rc<RefCell<App>>, path: &Path, image: slint::Image) {
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
fn queue_shell_icons(app: &Rc<RefCell<App>>) {
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
fn queue_folder_sizes(app: &Rc<RefCell<App>>) {
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
fn fill_size_cell(ui: &AppWindow, app: &Rc<RefCell<App>>, path: &Path, bytes: u64) {
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

fn push_status(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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

fn sync_tabs(ui: &AppWindow, app: &App) {
    let tabs: Vec<TabInfo> = app
        .tabs
        .iter()
        .map(|t| {
            let title = t
                .current()
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| t.current().to_string_lossy().into_owned());
            TabInfo {
                title: SharedString::from(title.as_str()),
                path: SharedString::from(t.current().to_string_lossy().as_ref()),
                icon: SharedString::from("folder"),
            }
        })
        .collect();
    ui.set_tabs(ModelRc::new(VecModel::from(tabs)));
    ui.set_active_tab(app.active as i32);
}

// ---------------------------------------------------------------- theme

fn refresh_theme(ui: &AppWindow, dark: bool) {
    let theme = ui.global::<Theme>();
    theme.set_dark(dark);
}

// ---------------------------------------------------------------- callbacks

fn wire_callbacks(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_navigate(move |p| {
            let Some(ui) = w.upgrade() else { return };
            navigate_to(&ui, &a, PathBuf::from(p.as_str()));
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_sidebar_activated(move |id| {
            let Some(ui) = w.upgrade() else { return };
            let path = id.split_once(':').map(|(_, p)| p.to_string()).unwrap_or_default();
            if !path.is_empty() {
                navigate_to(&ui, &a, PathBuf::from(path));
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_open_row(move |i| {
            let Some(ui) = w.upgrade() else { return };
            open_row(&ui, &a, i as usize);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_select_row(move |i, mode| {
            let Some(ui) = w.upgrade() else { return };
            select_row(&ui, &a, i as usize, mode);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_go_back(move || {
            let Some(ui) = w.upgrade() else { return };
            let changed = {
                let mut s = a.borrow_mut();
                let next = s.tab_mut().history.back().map(|p| p.to_path_buf());
                next.is_some()
            };
            if changed {
                reload(&ui, &a);
                sync_tabs(&ui, &a.borrow());
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_go_forward(move || {
            let Some(ui) = w.upgrade() else { return };
            let changed = {
                let mut s = a.borrow_mut();
                let next = s.tab_mut().history.forward().map(|p| p.to_path_buf());
                next.is_some()
            };
            if changed {
                reload(&ui, &a);
                sync_tabs(&ui, &a.borrow());
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_go_up(move || {
            let Some(ui) = w.upgrade() else { return };
            let parent = {
                let s = a.borrow();
                s.tab().current().parent().map(Path::to_path_buf)
            };
            if let Some(p) = parent {
                navigate_to(&ui, &a, p);
            }
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_refresh(move || {
            let Some(ui) = w.upgrade() else { return };
            reload(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_address_submitted(move |text| {
            let Some(ui) = w.upgrade() else { return };
            let p = files_core::path::normalize(text.as_str());
            ui.set_editing_address(false);
            if p.is_dir() {
                navigate_to(&ui, &a, p);
            } else {
                ui.set_error_text(SharedString::from(
                    format!("Không tìm thấy thư mục: {}", p.display()).as_str(),
                ));
                ui.set_busy(false);
            }
        });
    }
    {
        let w = ui.as_weak();
        ui.on_start_edit_address(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_editing_address(true);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_sort_by(move |column| {
            let Some(ui) = w.upgrade() else { return };
            {
                let mut s = a.borrow_mut();
                let by = match column.as_str() {
                    "size" => SortBy::Size,
                    "modified" => SortBy::DateModified,
                    "kind" => SortBy::Kind,
                    _ => SortBy::Name,
                };
                let t = s.tab_mut();
                let direction = if t.sort.by == by && t.sort.direction == SortDirection::Ascending {
                    SortDirection::Descending
                } else {
                    SortDirection::Ascending
                };
                t.sort = SortSpec { by, direction, folders_first: t.sort.folders_first };
                let spec = t.sort;
                spec.sort(&mut t.entries);
                t.rebuild_visible();
            }
            push_view(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_hidden(move || {
            let Some(ui) = w.upgrade() else { return };
            // Phải đọc rồi mới ghi trong hai lần mượn riêng: viết
            // `a.borrow_mut().x = !a.borrow().x` là mượn mutable và immutable
            // cùng lúc, và `RefCell` panic ngay — app chết ngay cú click đầu.
            let v = !a.borrow().show_hidden;
            a.borrow_mut().show_hidden = v;
            ui.set_show_hidden(v);
            reload(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_sidebar(move || {
            let Some(ui) = w.upgrade() else { return };
            let v = !a.borrow().show_sidebar;
            a.borrow_mut().show_sidebar = v;
            ui.set_show_sidebar(v);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_preview(move || {
            let Some(ui) = w.upgrade() else { return };
            let v = !a.borrow().show_preview;
            a.borrow_mut().show_preview = v;
            ui.set_show_preview(v);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_set_view_mode(move |mode| {
            let Some(ui) = w.upgrade() else { return };
            a.borrow_mut().view_mode = mode;
            ui.set_view_mode(mode);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_toggle_theme(move || {
            let Some(ui) = w.upgrade() else { return };
            let dark = !a.borrow().dark;
            a.borrow_mut().dark = dark;
            refresh_theme(&ui, dark);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_new_tab(move || {
            let Some(ui) = w.upgrade() else { return };
            {
                let mut s = a.borrow_mut();
                let start = s.tab().current().to_path_buf();
                s.tabs.push(Tab::new(start));
                s.active = s.tabs.len() - 1;
            }
            sync_tabs(&ui, &a.borrow());
            reload(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_close_tab(move |i| {
            let Some(ui) = w.upgrade() else { return };
            // Dùng chung một đường với menu chuột phải, để nút × và menu không
            // bao giờ lệch hành vi.
            close_tab_at(&ui, &a, (i >= 0).then_some(i as usize));
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_select_tab(move |i| {
            let Some(ui) = w.upgrade() else { return };
            {
                let mut s = a.borrow_mut();
                if (i as usize) < s.tabs.len() {
                    s.active = i as usize;
                }
            }
            sync_tabs(&ui, &a.borrow());
            reload(&ui, &a);
            push_view(&ui, &a);
        });
    }
    {
        let w = ui.as_weak();
        ui.on_start_filter(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_filtering(true);
            ui.set_filter_text(SharedString::default());
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_filter_changed(move |t| {
            let Some(ui) = w.upgrade() else { return };
            let mut s = a.borrow_mut();
            s.tab_mut().filter = t.to_lowercase();
            s.tab_mut().rebuild_visible();
            drop(s);
            push_view(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_end_filter(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_filtering(false);
            ui.set_filter_text(SharedString::default());
            {
                let mut s = a.borrow_mut();
                s.tab_mut().filter.clear();
                s.tab_mut().rebuild_visible();
            }
            push_view(&ui, &a);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_command(move |id| {
            let Some(ui) = w.upgrade() else { return };
            run_command(&ui, &a, id.as_str());
        });
    }
    {
        let w = ui.as_weak();
        ui.on_dialog_cancel(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_dialog_open(false);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_dialog_confirm(move |value| {
            let Some(ui) = w.upgrade() else { return };
            confirm_dialog(&ui, &a, value.as_str());
        });
    }
    {
        let w = ui.as_weak();
        ui.on_dismiss_menu(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_menu_open(false);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_menu_activated(move |id| {
            let Some(ui) = w.upgrade() else { return };
            ui.set_menu_open(false);
            run_command(&ui, &a, id.as_str());
        });
    }
    {
        let w = ui.as_weak();
        let a = app.clone();
        ui.on_tab_context_menu(move |i, x, y| {
            let Some(ui) = w.upgrade() else { return };
            logging::write(&format!("menu tab: i={i} x={x} y={y}"));
            let count = a.borrow().tabs.len();
            let idx = i as usize;
            if idx >= count {
                return;
            }
            // Nhớ tab đang nhắm tới: các lệnh bên dưới đọc lại `menu-tab`.
            ui.set_menu_tab(i);
            let has_others = count > 1;
            ui.set_menu_items(ModelRc::new(VecModel::from(vec![
                menu("tab-duplicate", "Nhân bản tab", "window", "", true, false),
                menu("", "", "", "", false, true),
                menu("tab-close", "Đóng tab", "dismiss", "Ctrl+W", true, false),
                menu("tab-close-others", "Đóng các tab khác", "dismiss", "", has_others, false),
                menu("tab-close-right", "Đóng tab bên phải", "dismiss", "", idx + 1 < count, false),
                menu("tab-close-left", "Đóng tab bên trái", "dismiss", "", idx > 0, false),
                menu("", "", "", "", false, true),
                menu("tab-close-all", "Đóng tất cả tab", "dismiss", "", true, false),
            ])));
            ui.set_menu_x(x);
            ui.set_menu_y(y);
            ui.set_menu_open(true);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_row_context_menu(move |i, x, y| {
            let Some(ui) = w.upgrade() else { return };
            // Lệnh nén/giải nén chỉ có nghĩa với đúng loại mục.
            let (is_dir, is_archive) = {
                let s = a.borrow();
                match s.tab().selected_entries().first() {
                    Some(e) => (
                        e.is_dir(),
                        e.extension
                            .as_deref()
                            .map(|x| x.eq_ignore_ascii_case("zip"))
                            .unwrap_or(false),
                    ),
                    None => (false, false),
                }
            };
            // Thẻ hiện có của mục đang chọn, để menu hiện dấu ✓ và bật/tắt
            // mục "Bỏ thẻ" cho đúng.
            let (tag_list, attached) = {
                let s = a.borrow();
                let mut attached = [false; 4];
                if let Some(first) = s.tab().selected_entries().first() {
                    let owned = s.tags.tags_for(Path::new(&first.path));
                    for (slot, tag) in s.tags.tags.iter().take(4).enumerate() {
                        attached[slot] = owned.iter().any(|t| t.uid == tag.uid);
                    }
                }
                (s.tags.tags.clone(), attached)
            };
            let items = vec![
                menu("open", "Mở", "open", "Enter", i >= 0, false),
                menu("open-new-tab", "Mở trong tab mới", "window", "", i >= 0, false),
                menu("", "", "", "", false, true),
                menu("copy", "Sao chép", "copy", "Ctrl+C", i >= 0, false),
                menu("cut", "Cắt", "cut", "Ctrl+X", i >= 0, false),
                menu("rename", "Đổi tên", "rename", "F2", i >= 0, false),
                menu("delete", "Xoá", "delete", "Del", i >= 0, false),
                menu("delete-permanent", "Xoá vĩnh viễn", "delete", "Shift+Del", i >= 0, false),
                menu("", "", "", "", false, true),
                menu("copy-path", "Sao chép đường dẫn", "link", "Ctrl+Shift+C", i >= 0, false),
                menu("", "", "", "", false, true),
                // Thẻ màu: đổi màu icon của tệp hoặc thư mục.
                menu("", "Thẻ màu", "", "", i >= 0, false),
                menu_tag(&tag_list[0], i >= 0 && attached[0]),
                menu_tag(&tag_list[1], i >= 0 && attached[1]),
                menu_tag(&tag_list[2], i >= 0 && attached[2]),
                menu_tag(&tag_list[3], i >= 0 && attached[3]),
                menu("tag-clear", "Bỏ thẻ", "dismiss", "", i >= 0 && attached.iter().any(|x| *x), false),
                menu("", "", "", "", false, true),
                menu("compress", "Nén thành ZIP", "folder_zip", "", i >= 0, false),
                menu("extract", "Giải nén", "folder_open", "", is_archive, false),
                menu("", "", "", "", false, true),
                menu("hash-sha256", "Mã băm SHA-256", "shield", "", i >= 0 && !is_dir, false),
                menu("hash-md5", "Mã băm MD5", "shield", "", i >= 0 && !is_dir, false),
                menu("", "", "", "", false, true),
                menu("folder-size", "Tính kích thước", "calculator", "", is_dir, false),
                menu("properties", "Thuộc tính", "info", "", i >= 0, false),
            ];
            ui.set_menu_items(ModelRc::new(VecModel::from(items)));
            ui.set_menu_x(x);
            ui.set_menu_y(y);
            ui.set_menu_open(true);
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_background_context_menu(move |x, y| {
            let Some(ui) = w.upgrade() else { return };
            let can_paste = !a.borrow().clipboard.is_empty();
            let items = vec![
                menu("new-folder", "Thư mục mới", "folder", "Ctrl+N", true, false),
                menu("new-file", "Tệp mới", "document", "", true, false),
                menu("", "", "", "", false, true),
                menu("paste", "Dán", "clipboard_paste", "Ctrl+V", can_paste, false),
                menu("select-all", "Chọn tất cả", "checkmark", "Ctrl+A", true, false),
                menu("", "", "", "", false, true),
                menu("refresh", "Làm mới", "arrow_clockwise", "F5", true, false),
                menu("open-terminal", "Mở Terminal tại đây", "code", "", true, false),
                menu("", "", "", "", false, true),
                menu("toggle-extensions", "Ẩn/hiện phần mở rộng", "document", "", true, false),
                menu("toggle-folders-first", "Thư mục trước tệp", "arrow_sort", "", true, false),
                menu("toggle-hidden", if a.borrow().show_hidden { "Ẩn mục ẩn" } else { "Hiện mục ẩn" }, "eye", "Ctrl+H", true, false),
            ];
            ui.set_menu_items(ModelRc::new(VecModel::from(items)));
            ui.set_menu_x(x);
            ui.set_menu_y(y);
            ui.set_menu_open(true);
        });
    }
    {
        let w = ui.as_weak();
        ui.on_open_palette(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_palette_query(SharedString::default());
            ui.set_palette_items(palette_model(""));
            ui.set_palette_open(true);
        });
    }
    {
        let w = ui.as_weak();
        ui.on_close_palette(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_palette_open(false);
        });
    }
    {
        let w = ui.as_weak();
        ui.on_palette_changed(move |q| {
            let Some(ui) = w.upgrade() else { return };
            ui.set_palette_items(palette_model(q.as_str()));
        });
    }
    {
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_palette_accepted(move |id| {
            let Some(ui) = w.upgrade() else { return };
            ui.set_palette_open(false);
            run_command(&ui, &a, id.as_str());
        });
    }
    {
        let w = ui.as_weak();
        ui.on_sidebar_toggled(move |_id| {
            let Some(ui) = w.upgrade() else { return };
            let _ = &ui;
        });
    }
}

/// Build the palette model for `query`.
fn palette_model(query: &str) -> ModelRc<PaletteEntry> {
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

fn menu(
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
    }
}

// ---------------------------------------------------------------- actions

fn navigate_to(ui: &AppWindow, app: &Rc<RefCell<App>>, target: PathBuf) {
    if !target.is_dir() {
        ui.set_error_text(SharedString::from(
            format!("Không tìm thấy thư mục: {}", target.display()).as_str(),
        ));
        ui.set_busy(false);
        return;
    }
    {
        let mut s = app.borrow_mut();
        s.tab_mut().history.push(target);
        s.tab_mut().filter.clear();
        s.tab_mut().selection.clear();
    }
    ui.set_filtering(false);
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

fn open_row(ui: &AppWindow, app: &Rc<RefCell<App>>, visible_index: usize) {
    let target = {
        let s = app.borrow();
        let tab = s.tab();
        tab.visible
            .get(visible_index)
            .and_then(|i| tab.entries.get(*i))
            .map(|e| (e.is_dir(), PathBuf::from(&e.path)))
    };
    let Some((is_dir, path)) = target else { return };
    if is_dir {
        navigate_to(ui, app, path);
    } else if let Err(e) = open::that_detached(&path) {
        ui.set_error_text(SharedString::from(
            format!("Không mở được tệp: {e}").as_str(),
        ));
    }
}

fn select_row(ui: &AppWindow, app: &Rc<RefCell<App>>, visible_index: usize, mode: i32) {
    {
        let mut s = app.borrow_mut();
        let active = s.active;
        let tab = &mut s.tabs[active];
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
        Some(e) => preview::show(ui, &e),
        None => preview::clear(ui),
    }
}

/// Flip the `selected` flag on the existing row models.
///
/// `set_row_data` mutates one row in place, so the `TouchArea` instances and
/// their double-click state survive the click.
fn update_selection_flags(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let a = app.borrow();
    let tab = a.tab();
    let rows = ui.get_rows();
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
    let per_row = grid_columns(a.grid_width);
    let grid = ui.get_grid_rows();
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

fn run_command(ui: &AppWindow, app: &Rc<RefCell<App>>, id: &str) {
    // Mục thẻ mang uid trong mã lệnh, nên phải bắt trước khi so khớp chuỗi.
    if let Some(uid) = id.strip_prefix("tag-set:") {
        set_tag_on_selection(ui, app, Some(uid));
        return;
    }
    match id {
        "tag-clear" => set_tag_on_selection(ui, app, None),
        "refresh" => reload(ui, app),
        "copy" => set_clipboard(ui, app, false),
        "cut" => set_clipboard(ui, app, true),
        "paste" => do_paste(ui, app),
        "delete" => ask_delete(ui, app),
        "rename" => ask_rename(ui, app),
        "new-folder" => ask_new(ui, app, "new-folder"),
        "new-file" => ask_new(ui, app, "new-file"),
        "new-menu" => show_new_menu(ui, 12.0, 40.0),
        "select-all" => {
            {
                let mut s = app.borrow_mut();
                let active = s.active;
                let tab = &mut s.tabs[active];
                tab.selection = tab.visible.clone();
            }
            push_view(ui, app);
            push_status(ui, app);
        }
        "clear-selection" => {
            {
                let mut s = app.borrow_mut();
                let active = s.active;
                s.tabs[active].selection.clear();
            }
            push_view(ui, app);
            push_status(ui, app);
            preview::clear(ui);
        }
        "toggle-hidden" => {
            let v = !app.borrow().show_hidden;
            app.borrow_mut().show_hidden = v;
            ui.set_show_hidden(v);
            reload(ui, app);
        }
        "open-selected" => {
            let first = app.borrow().tab().visible.iter().position(|i| {
                app.borrow().tab().selection.contains(i)
            });
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
                s.tab().selected_entries().first().map(|e| (e.is_dir(), PathBuf::from(&e.path)))
            };
            if let Some((true, path)) = target {
                {
                    let mut s = app.borrow_mut();
                    s.tabs.push(Tab::new(path));
                    s.active = s.tabs.len() - 1;
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
        "register-assoc" => ask_register_association(ui),
        "open-default-apps" => open_default_apps(ui),
        // ---- hiển thị ----
        "toggle-extensions" => {
            let v = !app.borrow().show_extensions;
            app.borrow_mut().show_extensions = v;
            push_view(ui, app);
            ui.set_status_tertiary(SharedString::from(
                if v { "Đang hiện phần mở rộng" } else { "Đang ẩn phần mở rộng" },
            ));
        }
        "toggle-folders-first" => {
            let v = !app.borrow().folders_first;
            app.borrow_mut().folders_first = v;
            // Sắp xếp lại ngay, không cần đọc lại thư mục.
            {
                let mut s = app.borrow_mut();
                let active = s.active;
                let tab = &mut s.tabs[active];
                tab.sort.folders_first = v;
                let spec = tab.sort;
                spec.sort(&mut tab.entries);
                tab.rebuild_visible();
            }
            push_view(ui, app);
            ui.set_status_tertiary(SharedString::from(
                if v { "Thư mục trước tệp" } else { "Sắp xếp chung" },
            ));
        }
        // ---- tab nâng cao ----
        "reopen-tab" => reopen_closed_tab(ui, app),
        "duplicate-tab" => duplicate_tab(ui, app),
        "next-tab" => cycle_tab(ui, app, 1),
        "prev-tab" => cycle_tab(ui, app, -1),
        "close-other-tabs" | "tab-close-others" => {
            focus_menu_tab(ui, app);
            close_other_tabs(ui, app)
        }
        "close-tabs-left" | "tab-close-left" => {
            focus_menu_tab(ui, app);
            close_tabs_side(ui, app, true)
        }
        "close-tabs-right" | "tab-close-right" => {
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

fn first_selected_visible(app: &Rc<RefCell<App>>) -> Option<usize> {
    let s = app.borrow();
    let tab = s.tab();
    tab.visible
        .iter()
        .position(|i| tab.selection.contains(i))
}

// ---------------------------------------------------------------- command palette

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

pub const PALETTE: &[PaletteCommand] = &[
    PaletteCommand { id: "new-folder", label: "Thư mục mới", icon: "folder", shortcut: "Ctrl+Shift+N" },
    PaletteCommand { id: "new-file", label: "Tệp mới", icon: "document", shortcut: "" },
    PaletteCommand { id: "copy", label: "Sao chép", icon: "copy", shortcut: "Ctrl+C" },
    PaletteCommand { id: "cut", label: "Cắt", icon: "cut", shortcut: "Ctrl+X" },
    PaletteCommand { id: "paste", label: "Dán", icon: "clipboard_paste", shortcut: "Ctrl+V" },
    PaletteCommand { id: "rename", label: "Đổi tên", icon: "rename", shortcut: "F2" },
    PaletteCommand { id: "delete", label: "Xoá vào Thùng rác", icon: "delete", shortcut: "Del" },
    PaletteCommand { id: "delete-permanent", label: "Xoá vĩnh viễn", icon: "delete", shortcut: "Shift+Del" },
    PaletteCommand { id: "copy-path", label: "Sao chép đường dẫn", icon: "link", shortcut: "Ctrl+Shift+C" },
    PaletteCommand { id: "compress", label: "Nén thành ZIP", icon: "folder_zip", shortcut: "" },
    PaletteCommand { id: "extract", label: "Giải nén", icon: "folder_open", shortcut: "" },
    PaletteCommand { id: "folder-size", label: "Tính kích thước thư mục", icon: "calculator", shortcut: "" },
    PaletteCommand { id: "open-terminal", label: "Mở Terminal tại đây", icon: "code", shortcut: "" },
    PaletteCommand { id: "toggle-hidden", label: "Ẩn/hiện mục ẩn", icon: "eye", shortcut: "Ctrl+H" },
    PaletteCommand { id: "toggle-extensions", label: "Ẩn/hiện phần mở rộng", icon: "document", shortcut: "" },
    PaletteCommand { id: "toggle-folders-first", label: "Thư mục trước tệp", icon: "arrow_sort", shortcut: "" },
    PaletteCommand { id: "toggle-sidebar", label: "Ẩn/hiện sidebar", icon: "panel_left", shortcut: "" },
    PaletteCommand { id: "toggle-preview", label: "Ẩn/hiện panel xem trước", icon: "panel_right", shortcut: "" },
    PaletteCommand { id: "view-details", label: "Xem dạng danh sách", icon: "list", shortcut: "" },
    PaletteCommand { id: "view-grid", label: "Xem dạng lưới", icon: "grid", shortcut: "" },
    PaletteCommand { id: "reopen-tab", label: "Mở lại tab đã đóng", icon: "window", shortcut: "Ctrl+Shift+T" },
    PaletteCommand { id: "duplicate-tab", label: "Nhân bản tab", icon: "window", shortcut: "Ctrl+D" },
    PaletteCommand { id: "next-tab", label: "Tab kế tiếp", icon: "chevron_right", shortcut: "Ctrl+Tab" },
    PaletteCommand { id: "prev-tab", label: "Tab trước", icon: "chevron_left", shortcut: "Ctrl+Shift+Tab" },
    PaletteCommand { id: "close-other-tabs", label: "Đóng các tab khác", icon: "dismiss", shortcut: "" },
    PaletteCommand { id: "close-all-tabs", label: "Đóng tất cả tab", icon: "dismiss", shortcut: "" },
    PaletteCommand { id: "refresh", label: "Làm mới", icon: "arrow_clockwise", shortcut: "F5" },
    PaletteCommand { id: "go-up", label: "Lên một cấp", icon: "arrow_upload", shortcut: "Backspace" },
    PaletteCommand { id: "toggle-theme", label: "Đổi sáng/tối", icon: "weather_moon", shortcut: "" },
    PaletteCommand { id: "tag-set:f7e0e137-2eb5-4fa4-a50d-ddd65df17c34", label: "Thẻ: Home (xanh dương)", icon: "tag", shortcut: "" },
    PaletteCommand { id: "tag-set:c84a8131-c4de-47d9-9440-26e859d14b3d", label: "Thẻ: Work (cam)", icon: "tag", shortcut: "" },
    PaletteCommand { id: "tag-set:d4b8d4bd-ceaf-4e58-ac61-a185fcf96c5d", label: "Thẻ: Photos (vàng)", icon: "tag", shortcut: "" },
    PaletteCommand { id: "tag-set:79376daf-c44a-4fe4-aa3b-8b30baea453e", label: "Thẻ: Important (xanh lá)", icon: "tag", shortcut: "" },
    PaletteCommand { id: "tag-clear", label: "Bỏ thẻ màu", icon: "dismiss", shortcut: "" },
    PaletteCommand { id: "register-assoc", label: "Đặt làm trình duyệt tệp mặc định", icon: "link", shortcut: "" },
    PaletteCommand { id: "open-default-apps", label: "Mở Settings ứng dụng mặc định", icon: "settings", shortcut: "" },
];

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

/// Ask before a permanent delete: there is no undo, so the dialog is the guard.
fn ask_delete_permanent(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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

/// Put the selected paths on the clipboard as text, one per line.
fn copy_selected_path(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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

/// Đăng ký app làm lựa chọn mở thư mục, rồi mở Settings để người dùng chọn.
///
/// Windows không cho đặt mặc định bằng code; bước này chỉ làm app xuất hiện
/// trong danh sách, còn quyết định cuối cùng là của người dùng.
/// Hỏi trước khi đăng ký: đây là thay đổi có thể thấy được trên toàn hệ thống.
fn ask_register_association(ui: &AppWindow) {
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
fn register_association(ui: &AppWindow) {
    let exe = std::env::current_exe().unwrap_or_default();
    match files_fs::register_file_assoc(&exe) {
        Ok(()) => {
            ui.set_status_tertiary(SharedString::from(
                "Đã đăng ký. Chọn Files RS trong trang Settings vừa mở.",
            ));
            let _ = files_fs::open_settings_page();
        }
        Err(e) => ui.set_error_text(SharedString::from(e.message.as_str())),
    }
}

fn open_default_apps(ui: &AppWindow) {
    if let Err(e) = files_fs::open_settings_page() {
        ui.set_error_text(SharedString::from(e.message.as_str()));
    }
}

fn open_terminal_here(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let dir = app.borrow().tab().current().to_path_buf();
    if let Err(e) = files_fs::open_terminal(&dir) {
        ui.set_error_text(SharedString::from(e.message.as_str()));
    }
}

// ---------------------------------------------------------------- tab commands

/// Restore the most recently closed tab.
fn reopen_closed_tab(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let path = app.borrow_mut().closed_tabs.pop();
    let Some(path) = path else { return };
    // The folder may have been deleted while the tab was closed.
    if !path.is_dir() {
        ui.set_error_text(SharedString::from(
            format!("Thư mục đã đóng không còn tồn tại: {}", path.display()).as_str(),
        ));
        return;
    }
    {
        let mut s = app.borrow_mut();
        s.tabs.push(Tab::new(path));
        s.active = s.tabs.len() - 1;
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

/// Open a copy of the active tab, keeping its folder and history.
fn duplicate_tab(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let start = app.borrow().tab().current().to_path_buf();
    {
        let mut s = app.borrow_mut();
        s.tabs.push(Tab::new(start));
        s.active = s.tabs.len() - 1;
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

/// Apply `uid` to every selected item, or clear all tags when `None`.
///
/// Tags are stored per path and saved immediately: a crash or a kill must not
/// lose the colour the user just chose.
fn set_tag_on_selection(ui: &AppWindow, app: &Rc<RefCell<App>>, uid: Option<&str>) {
    let paths = app.borrow().tab().selected_paths();
    if paths.is_empty() {
        return;
    }
    let mut failed_save = None;
    let (name, changed) = {
        let mut s = app.borrow_mut();
        let base = uid
            .and_then(|u| s.tags.tag(u).map(|t| t.name.clone()))
            .unwrap_or_else(|| "Bỏ thẻ".to_string());
        let name = base;
        // Bấm lại một thẻ đã gắn thì gỡ nó ra: đó là cách tự nhiên nhất để
        // tắt màu mà không phải nhớ "Bỏ thẻ".
        let all_tagged = uid.is_some_and(|u| {
            paths
                .iter()
                .all(|p| s.tags.tags_for(p).iter().any(|t| t.uid == u))
        });
        for path in &paths {
            match uid {
                Some(u) if all_tagged => s.tags.detach(path, u),
                Some(u) => s.tags.attach(path, u),
                None => s.tags.clear(path),
            }
        }
        let path = s.tag_path.clone();
        let store = s.tags.clone();
        drop(s);
        if let Err(e) = store.save(&path) {
            failed_save = Some(e);
        }
        (name, paths.len())
    };

    if let Some(e) = failed_save {
        logging::write(&format!("không lưu được thẻ: {e}"));
        let text = format!("Không lưu được thẻ: {e}");
        ui.set_error_text(SharedString::from(text.as_str()));
    } else {
        ui.set_status_tertiary(SharedString::from(
            format!("{name}: {changed} mục").as_str(),
        ));
    }
    // Vẽ lại để icon đổi màu ngay.
    push_view(ui, app);
}

/// A menu row carrying a colour swatch, used for the tag list.
fn menu_tag(tag: &tags::Tag, attached: bool) -> MenuItem {
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
        dot: slint::Color::from_argb_u8(
            255,
            (tags::parse_hex(&tag.color).unwrap_or(0x808080) >> 16) as u8,
            ((tags::parse_hex(&tag.color).unwrap_or(0x808080) >> 8) & 0xFF) as u8,
            (tags::parse_hex(&tag.color).unwrap_or(0x808080) & 0xFF) as u8,
        ),
        has_dot: true,
    }
}

/// Tab the context menu points at, if it is still a valid index.
fn menu_tab(ui: &AppWindow, app: &Rc<RefCell<App>>) -> Option<usize> {
    let idx = ui.get_menu_tab();
    let len = app.borrow().tabs.len();
    (idx >= 0 && (idx as usize) < len).then_some(idx as usize)
}

/// Make the menu's tab the active one, so the "close others" family acts on it.
fn focus_menu_tab(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    if let Some(idx) = menu_tab(ui, app) {
        app.borrow_mut().active = idx;
    }
}

/// Close one tab by index, falling back to the active tab.
///
/// Mirrors what the × button does, so the menu and the button cannot drift
/// apart: same bookkeeping for `closed_tabs`, same clamp of `active`.
fn close_tab_at(ui: &AppWindow, app: &Rc<RefCell<App>>, target: Option<usize>) {
    {
        let mut s = app.borrow_mut();
        let idx = target.unwrap_or(s.active);
        let Some((_, new_active)) = close_index_result(s.tabs.len(), s.active, idx) else {
            return;
        };
        // Ghi nhớ để Ctrl+Shift+T mở lại được.
        let closed = s.tabs[idx].current().to_path_buf();
        s.tabs.remove(idx);
        s.closed_tabs.push(closed);
        s.active = new_active;
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

/// What (tab_count, active_index) becomes after closing tab `idx`.
///
/// `None` means the close must be ignored: the last tab stays, like Files and
/// every browser, and an out-of-range index means a stale menu.
///
/// Kept pure so the index arithmetic — the part that silently mis-selects a tab
/// — can be tested without a window.
fn close_index_result(len: usize, active: usize, idx: usize) -> Option<(usize, usize)> {
    if len <= 1 || idx >= len {
        return None;
    }
    let new_len = len - 1;
    let new_active = if active >= new_len {
        // Đóng tab cuối đang mở: lùi về tab trước.
        new_len - 1
    } else if active > idx {
        // Đóng tab bên trái tab đang mở: mọi thứ dịch sang trái một bậc.
        active - 1
    } else {
        // Đóng tab bên phải (hoặc chính tab đang mở): vị trí giữ nguyên, và
        // tab kế tiếp trượt vào chỗ đó.
        active
    };
    Some((new_len, new_active))
}

/// Move `delta` tabs along, wrapping at both ends.
fn cycle_tab(ui: &AppWindow, app: &Rc<RefCell<App>>, delta: i32) {
    {
        let mut s = app.borrow_mut();
        let n = s.tabs.len();
        if n <= 1 {
            return;
        }
        let cur = s.active as i32;
        s.active = (cur + delta).rem_euclid(n as i32) as usize;
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

fn close_other_tabs(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    {
        let mut s = app.borrow_mut();
        if s.tabs.len() <= 1 {
            return;
        }
        let keep = s.active;
        let kept = s.tabs.remove(keep);
        // Every removed tab becomes reopenable, oldest first.
        let removed: Vec<PathBuf> = s.tabs.drain(..).map(|t| t.current().to_path_buf()).collect();
        s.closed_tabs.extend(removed);
        s.tabs.push(kept);
        s.active = 0;
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

/// Close every tab to the left (`left == true`) or right of the active one.
fn close_tabs_side(ui: &AppWindow, app: &Rc<RefCell<App>>, left: bool) {
    {
        let mut s = app.borrow_mut();
        if s.tabs.len() <= 1 {
            return;
        }
        let active = s.active;
        let (range, new_active) = if left {
            (0..active, 0usize)
        } else {
            ((active + 1)..s.tabs.len(), active)
        };
        let mut removed = Vec::new();
        for i in range.rev() {
            removed.push(s.tabs.remove(i).current().to_path_buf());
        }
        for path in removed {
            s.closed_tabs.push(path);
        }
        s.active = new_active.min(s.tabs.len().saturating_sub(1));
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

/// Close everything and open one fresh tab, like Files' "Close all tabs".
fn close_all_tabs(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let start = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\"));
    {
        let mut s = app.borrow_mut();
        let paths: Vec<PathBuf> = s.tabs.iter().map(|t| t.current().to_path_buf()).collect();
        for p in paths {
            s.closed_tabs.push(p);
        }
        s.tabs = vec![Tab::new(start)];
        s.active = 0;
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

fn move_selection(ui: &AppWindow, app: &Rc<RefCell<App>>, down: bool) {
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

fn set_clipboard(ui: &AppWindow, app: &Rc<RefCell<App>>, cut: bool) {
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

fn do_paste(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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

fn ask_delete(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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

fn ask_rename(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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

fn ask_new(ui: &AppWindow, app: &Rc<RefCell<App>>, kind: &str) {
    let _ = app;
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
fn show_new_menu(ui: &AppWindow, x: f32, y: f32) {
    let items = vec![
        menu("new-folder", "Thư mục mới", "folder", "Ctrl+Shift+N", true, false),
        menu("new-file", "Tệp văn bản mới", "document", "", true, false),
    ];
    ui.set_menu_items(ModelRc::new(VecModel::from(items)));
    ui.set_menu_x(x);
    ui.set_menu_y(y);
    ui.set_menu_open(true);
}

fn confirm_dialog(ui: &AppWindow, app: &Rc<RefCell<App>>, value: &str) {
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
                s.tab().selected_entries().first().map(|e| PathBuf::from(&e.path))
            };
            if let Some(from) = from {
                app.borrow().worker.submit(Job::Rename { from, to: value.to_string() });
                ui.set_busy(true);
                ui.set_busy_text(SharedString::from("Đang đổi tên…"));
            }
        }
        "new-folder" => {
            app.borrow().worker.submit(Job::CreateFolder { parent: current, name: value.to_string() });
            ui.set_busy(true);
            ui.set_busy_text(SharedString::from("Đang tạo thư mục…"));
        }
        "new-file" => {
            app.borrow().worker.submit(Job::CreateFile { parent: current, name: value.to_string() });
            ui.set_busy(true);
            ui.set_busy_text(SharedString::from("Đang tạo tệp…"));
        }
        "confirm-delete" => {
            let paths = app.borrow().tab().selected_paths();
            if !paths.is_empty() {
                app.borrow().worker.submit(Job::Delete { paths });
                ui.set_busy(true);
                ui.set_busy_text(SharedString::from("Đang chuyển vào Thùng rác…"));
            }
        }
        "confirm-default" => register_association(ui),
        "confirm-delete-permanent" => {
            let paths = app.borrow().tab().selected_paths();
            if !paths.is_empty() {
                app.borrow().worker.submit(Job::DeletePermanent { paths });
                ui.set_busy(true);
                ui.set_busy_text(SharedString::from("Đang xoá vĩnh viễn…"));
            }
        }
        _ => {}
    }
}

fn show_properties(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let s = app.borrow();
    let Some(entry) = s.tab().selected_entries().first().cloned() else { return };
    preview::show(ui, entry);
}

/// Compress the selection into `<name>.zip` in the current folder.
fn compress_selection(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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
fn extract_selection(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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

fn hash_selection(ui: &AppWindow, app: &Rc<RefCell<App>>, kind: files_fs::HashKind) {
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
fn view_folder_size(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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

/// Drain finished background work on a short timer.
///
/// `invoke_from_event_loop` would be simpler but cannot be called from a thread
/// that outlives the component; a timer keeps the handoff inside the event loop
/// and makes shutdown deterministic.
fn install_result_pump(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let (w, a) = (ui.as_weak(), app.clone());
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        let Some(ui) = w.upgrade() else { return };
        loop {
            let done = {
                let s = a.borrow();
                s.results.try_recv()
            };
            match done {
                Ok(JobDone::Loaded { token, path, result }) => {
                    // A reply for a directory the user already left must not
                    // overwrite the current view.
                    if token != a.borrow().load_token {
                        continue;
                    }
                    // This reply settles the in-flight read, whatever it says.
                    a.borrow_mut().load_in_flight = false;
                    match result {
                        Ok(listing) => {
                            apply_listing(&ui, &a, listing);
                        }
                        Err(e) => {
                            ui.set_error_text(SharedString::from(e.message.as_str()));
                            ui.set_busy(false);
                            // The history must not keep a path the user never saw.
                            let back = a.borrow().tab().current().parent().map(Path::to_path_buf);
                            if let Some(p) = back {
                                if p != path {
                                    let mut s = a.borrow_mut();
                                    s.tab_mut().history.replace_current(p);
                                }
                            }
                        }
                    }
                }
                Ok(JobDone::OpDone { label, result }) => {
                    ui.set_busy(false);
                    ui.set_busy_text(SharedString::default());
                    match result {
                        Ok(()) => {
                            ui.set_status_tertiary(SharedString::from(label.as_str()));
                            // Sau khi di chuyển, nguồn đã biến mất nên clipboard
                            // phải rỗng; dán lại sẽ báo lỗi không tìm thấy.
                            if a.borrow().clipboard_cut {
                                a.borrow_mut().clipboard.clear();
                                ui.set_clipboard_count(0);
                                a.borrow_mut().clipboard_cut = false;
                                ui.set_clipboard_mode(SharedString::default());
                            }
                            reload(&ui, &a);
                        }
                        Err(e) => {
                            logging::write(&format!("thao tác lỗi [{}]: {}", e.code, e.message));
                            // Câu giải thích thay cho "(os error 5)" trần trụi.
                            let text = format::explain_error(&e.code, &e.message);
                            ui.set_error_text(SharedString::from(text.as_str()));
                        }
                    }
                }
                Ok(JobDone::FolderSize { path, bytes, token }) => {
                    // Token 0 là lệnh thủ công từ menu; token khác phải khớp
                    // lần đọc thư mục hiện tại, nếu không kết quả thuộc về
                    // một thư mục người dùng đã rời đi.
                    if token != 0 && token != a.borrow().load_token {
                        continue;
                    }
                    let updated = {
                        let mut s = a.borrow_mut();
                        let active = s.active;
                        let path_str = path.to_string_lossy().into_owned();
                        match s.tabs[active]
                            .entries
                            .iter_mut()
                            .find(|e| e.path == path_str)
                        {
                            Some(e) => {
                                e.size = Some(bytes);
                                true
                            }
                            None => false,
                        }
                    };
                    if updated {
                        // Điền đúng ô đó, không dựng lại cả bảng.
                        fill_size_cell(&ui, &a, &path, bytes);
                    }
                    if token != 0 {
                        ui.set_status_tertiary(SharedString::from(
                            format!("Kích thước thư mục: {}", format::format_size(Some(bytes))).as_str(),
                        ));
                    }
                }
                Ok(JobDone::ShellIcon { path, icon, token }) => {
                    // Bỏ qua nếu người dùng đã rời thư mục.
                    if token != a.borrow().load_token {
                        continue;
                    }
                    let image = to_slint_image(&icon);
                    apply_shell_icon(&ui, &a, &path, image);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
        }
    });
    // The timer must stay alive for the lifetime of the window.
    std::mem::forget(timer);
}

/// Poll the directory watcher and refresh when the folder changes.
fn install_watch_pump(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let (w, a) = (ui.as_weak(), app.clone());
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(400), move || {
        let Some(ui) = w.upgrade() else { return };
        let fired = {
            let s = a.borrow();
            match &s.watch {
                Some(h) => h.try_recv().is_some(),
                None => false,
            }
        };
        // Một lần đọc đang chạy thì bỏ qua nhịp này: nếu không, mỗi sự kiện
        // của `notify` lại xếp thêm một lần đọc và UI quay vòng vô ích.
        if fired && !a.borrow().load_in_flight {
            reload(&ui, &a);
        }
    });
    std::mem::forget(timer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_palette_query_lists_every_command() {
        // The palette must be browsable on open: an empty box teaches nothing.
        assert_eq!(filter_palette("").len(), PALETTE.len());
    }

    #[test]
    fn a_query_filters_case_insensitively() {
        let hits = filter_palette("ĐỔI TÊN");
        assert!(
            hits.iter().any(|c| c.id == "rename"),
            "Vietnamese is case-folded too"
        );
    }

    #[test]
    fn a_query_with_no_match_returns_nothing() {
        assert!(filter_palette("zzzznotacommand").is_empty());
    }

    #[test]
    fn every_palette_id_is_unique() {
        // Duplicate ids would make two entries run the same command, which is
        // how a palette silently rots.
        let mut ids: Vec<&str> = PALETTE.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate command id in PALETTE");
    }

    /// Scratch directory next to the test binary.
    ///
    /// `CARGO_TARGET_TMPDIR` only exists for integration tests in `tests/`, not
    /// for unit tests inside a binary, and `%TEMP%` is not writable on every
    /// machine. Sitting beside the exe keeps the test self-contained.
    fn scratch(name: &str) -> PathBuf {
        let exe = std::env::current_exe().expect("đường dẫn exe");
        let base = exe.parent().expect("thư mục chứa exe").join("test-scratch");
        let dir = base.join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tạo thư mục tạm");
        dir
    }

    #[test]
    fn creating_and_renaming_through_the_worker_reaches_disk() {
        // Đúng đường đi của hộp thoại: dialog gửi Job, worker chạy, đĩa đổi.
        // Test này bắt được lỗi "bấm Tạo mà không có gì xảy ra".
        let root = scratch("worker-create");
        let (worker, rx) = Worker::spawn();

        worker.submit(Job::CreateFolder {
            parent: root.clone(),
            name: "tài liệu".into(),
        });
        match rx.recv_timeout(Duration::from_secs(10)).expect("worker phải trả lời") {
            JobDone::OpDone { result, .. } => result.expect("tạo thư mục phải thành công"),
            _ => panic!("cần OpDone"),
        }
        let created = root.join("tài liệu");
        assert!(created.is_dir(), "thư mục phải có trên đĩa");

        worker.submit(Job::Rename {
            from: created.clone(),
            to: "đã đổi tên".into(),
        });
        match rx.recv_timeout(Duration::from_secs(10)).expect("worker phải trả lời") {
            JobDone::OpDone { result, .. } => result.expect("đổi tên phải thành công"),
            _ => panic!("cần OpDone"),
        }
        assert!(root.join("đã đổi tên").is_dir(), "tên mới phải tồn tại");
        assert!(!created.exists(), "tên cũ phải biến mất");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_name_still_creates_a_folder() {
        // Hộp thoại cho bấm "Tạo" khi ô trống; Rust tự đặt tên.
        let root = scratch("worker-empty-name");
        let (worker, rx) = Worker::spawn();
        worker.submit(Job::CreateFolder {
            parent: root.clone(),
            name: String::new(),
        });
        match rx.recv_timeout(Duration::from_secs(10)).expect("worker phải trả lời") {
            JobDone::OpDone { result, .. } => result.expect("tạo với tên rỗng phải thành công"),
            _ => panic!("cần OpDone"),
        }
        let count = std::fs::read_dir(&root).unwrap().count();
        assert_eq!(count, 1, "phải có đúng một thư mục được tạo");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn icon_work_never_blocks_file_operations() {
        // Đây là hồi quy cho lỗi thật: icon và thao tác tệp từng chung một
        // luồng, nên một icon chậm làm mọi lệnh tạo/đổi tên sau đó đứng im.
        // Xếp 300 icon trước, rồi gửi lệnh tạo: lệnh tạo vẫn phải xong nhanh.
        let root = scratch("worker-not-blocked");
        let (worker, rx) = Worker::spawn();

        let exe = std::env::current_exe().expect("đường dẫn exe");
        for _ in 0..300 {
            worker.submit_icon(exe.clone(), 20, 1);
        }
        worker.submit(Job::CreateFolder {
            parent: root.clone(),
            name: "sau-icon".into(),
        });

        let start = std::time::Instant::now();
        // Bỏ qua các kết quả icon cho tới khi gặp OpDone.
        loop {
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(JobDone::OpDone { result, .. }) => {
                    result.expect("tạo thư mục phải thành công");
                    break;
                }
                Ok(_) => continue,
                Err(e) => panic!("worker không trả lời: {e}"),
            }
        }
        let elapsed = start.elapsed();
        assert!(
            root.join("sau-icon").is_dir(),
            "thư mục phải được tạo dù có 300 icon xếp trước"
        );
        // Ngưỡng rộng: mục tiêu là bắt được việc bị chặn hẳn, không phải đo hiệu năng.
        assert!(
            elapsed < Duration::from_secs(5),
            "lệnh tạo bị chặn bởi icon: mất {elapsed:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn closing_a_tab_before_the_active_one_keeps_the_same_view() {
        // [A][B][C], đang ở C (2), đóng B (1) -> vẫn là C, nay ở vị trí 1.
        assert_eq!(close_index_result(3, 2, 1), Some((2, 1)));
    }

    #[test]
    fn closing_the_active_last_tab_selects_the_new_last() {
        // [A][B][C], đang ở C (2), đóng C -> về B ở vị trí 1.
        assert_eq!(close_index_result(3, 2, 2), Some((2, 1)));
    }

    #[test]
    fn closing_a_tab_after_the_active_one_keeps_the_index() {
        // [A][B][C], đang ở A (0), đóng C (2) -> vẫn A ở 0.
        assert_eq!(close_index_result(3, 0, 2), Some((2, 0)));
    }

    #[test]
    fn closing_the_active_middle_tab_shows_the_next_one() {
        // [A][B][C], đang ở B (1), đóng B -> tab sau trượt vào, vẫn vị trí 1.
        assert_eq!(close_index_result(3, 1, 1), Some((2, 1)));
    }

    #[test]
    fn the_last_tab_cannot_be_closed() {
        // Đóng tab duy nhất phải bị bỏ qua, nếu không cửa sổ trắng.
        assert_eq!(close_index_result(1, 0, 0), None);
    }

    #[test]
    fn a_stale_menu_index_is_ignored() {
        // Menu mở trên tab đã bị đóng ở nơi khác: không được panic.
        assert_eq!(close_index_result(2, 0, 5), None);
    }

    #[test]
    fn closing_never_leaves_the_active_index_out_of_range() {
        // Quét mọi tổ hợp để chắc không có cửa sổ trắng hay chỉ số âm.
        for len in 2..8usize {
            for active in 0..len {
                for idx in 0..len {
                    let (new_len, new_active) =
                        close_index_result(len, active, idx).expect("phải đóng được");
                    assert_eq!(new_len, len - 1);
                    assert!(
                        new_active < new_len,
                        "len={len} active={active} idx={idx} -> {new_active} ngoài {new_len}"
                    );
                }
            }
        }
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
