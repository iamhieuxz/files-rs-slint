slint::include_modules!();

mod format;
mod history;
mod ops_worker;
mod preview;

use files_core::sort::ListOptions;
use files_core::{breadcrumbs, Entry, SortBy, SortDirection, SortSpec};
use history::History;
use ops_worker::{Job, JobDone, Worker};
use slint::{ModelRc, SharedString, Timer, TimerMode, VecModel};
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
    worker: Worker,
    results: Receiver<JobDone>,
    /// Paths whose change should trigger a refresh, from the directory watcher.
    watch: Option<files_fs::WatchHandle>,
    watched_path: Option<PathBuf>,
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
        worker,
        results,
        watch: None,
        watched_path: None,
    }));

    build_sidebar(&ui);
    refresh_theme(&ui, false);
    reload(&ui, &app);
    sync_tabs(&ui, &app.borrow());

    wire_callbacks(&ui, &app);
    install_result_pump(&ui, &app);
    install_watch_pump(&ui, &app);

    ui.run()
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
    ui.set_error_text(SharedString::default());
    ui.set_busy(false);
    push_view(ui, app);
    push_status(ui, app);
}

fn push_view(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let a = app.borrow();
    let tab = a.tab();
    let rows: Vec<FileRow> = tab
        .visible
        .iter()
        .filter_map(|i| tab.entries.get(*i))
        .map(|e| {
            let (icon, glyph) = format::icon_for(e);
            FileRow {
                name: SharedString::from(e.name.as_str()),
                path: SharedString::from(e.path.as_str()),
                is_dir: e.is_dir(),
                is_hidden: e.is_hidden,
                selected: tab.selection.contains(&index_of(tab, e)),
                size: SharedString::from(format::format_size(e.size).as_str()),
                modified: SharedString::from(format::format_time(e.modified_ms).as_str()),
                kind: SharedString::from(format::kind_label(e).as_str()),
                icon: SharedString::from(icon),
                glyph: color_from_u32(glyph),
            }
        })
        .collect();

    ui.set_rows(ModelRc::new(VecModel::from(rows)));
    ui.set_address_text(SharedString::from(tab.current().to_string_lossy().as_ref()));
    ui.set_can_back(tab.history.can_back());
    ui.set_can_forward(tab.history.can_forward());
    ui.set_selection_count(tab.selection.len() as i32);
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

/// Index of `entry` inside `tab.entries`.
fn index_of(tab: &Tab, entry: &Entry) -> usize {
    tab.entries
        .iter()
        .position(|e| e.path == entry.path)
        .unwrap_or(0)
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
        ui.on_select_row(move |i, extend| {
            let Some(ui) = w.upgrade() else { return };
            select_row(&ui, &a, i as usize, extend);
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
            a.borrow_mut().show_hidden = !a.borrow().show_hidden;
            ui.set_show_hidden(a.borrow().show_hidden);
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
            {
                let mut s = a.borrow_mut();
                if s.tabs.len() <= 1 {
                    return;
                }
                let idx = i as usize;
                if idx >= s.tabs.len() {
                    return;
                }
                s.tabs.remove(idx);
                if s.active >= s.tabs.len() {
                    s.active = s.tabs.len() - 1;
                } else if s.active > idx {
                    s.active -= 1;
                }
            }
            sync_tabs(&ui, &a.borrow());
            reload(&ui, &a);
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
        let (w, a) = (ui.as_weak(), app.clone());
        ui.on_row_context_menu(move |i, x, y| {
            let Some(ui) = w.upgrade() else { return };
            let has_sel = !a.borrow().tab().selection.is_empty();
            let items = vec![
                menu("open", "Mở", "open", "Enter", i >= 0, false),
                menu("open-new-tab", "Mở trong tab mới", "window", "", i >= 0, false),
                menu("", "", "", "", false, true),
                menu("copy", "Sao chép", "copy", "Ctrl+C", i >= 0, false),
                menu("cut", "Cắt", "cut", "Ctrl+X", i >= 0, false),
                menu("rename", "Đổi tên", "rename", "F2", i >= 0, false),
                menu("delete", "Xoá", "delete", "Del", i >= 0, false),
                menu("", "", "", "", false, true),
                menu("properties", "Thuộc tính", "info", "", i >= 0, false),
            ];
            let _ = has_sel;
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
        ui.on_sidebar_toggled(move |_id| {
            let Some(ui) = w.upgrade() else { return };
            let _ = &ui;
        });
    }
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

fn select_row(ui: &AppWindow, app: &Rc<RefCell<App>>, visible_index: usize, extend: bool) {
    {
        let mut s = app.borrow_mut();
        let active = s.active;
        let tab = &mut s.tabs[active];
        let Some(entry_index) = tab.visible.get(visible_index).copied() else {
            return;
        };
        if extend {
            // Shift+click selects the range from the anchor to the clicked row.
            let anchor = tab.anchor.unwrap_or(entry_index);
            let lo = anchor.min(entry_index);
            let hi = anchor.max(entry_index);
            tab.selection = tab
                .visible
                .iter()
                .copied()
                .filter(|i| *i >= lo && *i <= hi)
                .collect();
        } else {
            tab.selection = vec![entry_index];
            tab.anchor = Some(entry_index);
        }
    }
    push_view(ui, app);
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

fn run_command(ui: &AppWindow, app: &Rc<RefCell<App>>, id: &str) {
    match id {
        "refresh" => reload(ui, app),
        "copy" => set_clipboard(app, false),
        "cut" => set_clipboard(app, true),
        "paste" => do_paste(ui, app),
        "delete" => ask_delete(ui, app),
        "rename" => ask_rename(ui, app),
        "new-folder" => ask_new(ui, app, "new-folder"),
        "new-file" => ask_new(ui, app, "new-file"),
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
        "nav-up" | "nav-down" => move_selection(ui, app, id == "nav-down"),
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
        select_row(ui, app, i, false);
    }
}

fn set_clipboard(app: &Rc<RefCell<App>>, cut: bool) {
    let paths = app.borrow().tab().selected_paths();
    if paths.is_empty() {
        return;
    }
    let mut s = app.borrow_mut();
    s.clipboard = paths;
    s.clipboard_cut = cut;
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
    ui.set_dialog_value(SharedString::default());
    ui.set_dialog_open(true);
}

fn confirm_dialog(ui: &AppWindow, app: &Rc<RefCell<App>>, value: &str) {
    let kind = ui.get_dialog_kind().to_string();
    ui.set_dialog_open(false);
    let current = app.borrow().tab().current().to_path_buf();

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
        _ => {}
    }
}

fn show_properties(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let s = app.borrow();
    let Some(entry) = s.tab().selected_entries().first().cloned() else { return };
    preview::show(ui, &entry);
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
                            if !a.borrow().clipboard_cut || !label.contains("di chuyển") {
                                // nothing extra
                            }
                            if label.contains("di chuyển") {
                                a.borrow_mut().clipboard.clear();
                                ui.set_clipboard_count(0);
                            }
                            reload(&ui, &a);
                        }
                        Err(e) => {
                            ui.set_error_text(SharedString::from(e.message.as_str()));
                        }
                    }
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
        if fired && !a.borrow().busy_now() {
            reload(&ui, &a);
        }
    });
    std::mem::forget(timer);
}

impl App {
    /// True while a load is in flight; used to avoid stacking refreshes.
    fn busy_now(&self) -> bool {
        false
    }
}
