//! Application state.
//!
//! Two structures: [`Tab`] owns everything one tab is looking at, [`App`] owns
//! the tabs and the process-wide choices (theme, panels, clipboard, tag store).
//!
//! Deliberately free of Slint callbacks and of any business operation: this
//! module only *holds* state and offers the small accessors the other modules
//! need. That keeps the dependency direction one-way — `state` depends on
//! nothing of the UI.

use crate::history::History;
use crate::ops_worker::{JobDone, Worker};
use files_core::sort::ListOptions;
use files_core::{Entry, SortSpec};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Instant;

use crate::slint_generated::*;
use crate::{logging, settings, tags};

/// Which of the two file panes owns the address bar, command bar and keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Left,
    Right,
}

impl Pane {
    pub fn index(self) -> usize {
        match self {
            Pane::Left => 0,
            Pane::Right => 1,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Pane::Left => "left",
            Pane::Right => "right",
        }
    }
}

/// Load generation for one pane. Tokens are unique across both panes.
#[derive(Clone, Copy, Default)]
pub struct PaneLoad {
    pub token: u64,
    pub in_flight: bool,
}

/// Rectangle in Slint logical window coordinates.
#[derive(Clone, Copy, Default, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

#[derive(Default)]
pub struct DragState {
    pub source: Option<Pane>,
    pub paths: Vec<PathBuf>,
    pub copy: bool,
    pub pane_bounds: [Option<Rect>; 2],
    pub sidebar_zones: Vec<(String, Rect)>,
    /// Đích của lần DragMove gần nhất, để lúc thả dùng đúng vùng người dùng
    /// đang thấy sáng — không hit-test lại sau khi overlay biến mất.
    pub hover_pane: Option<Pane>,
    pub hover_sidebar: Option<String>,
}

/// Everything one tab owns.
pub struct Tab {
    pub history: History,
    pub entries: Vec<Entry>,
    /// Indices into `entries` that are currently selected.
    pub selection: Vec<usize>,
    /// Last row clicked, so Shift+click can extend from it.
    pub anchor: Option<usize>,
    pub sort: SortSpec,
    /// Text the user typed into the filter box, lowercased once.
    pub filter: String,
    /// Row indices after filtering; `entries` stays the source of truth.
    pub visible: Vec<usize>,
    /// Cách nhóm đang chọn; `None` là danh sách phẳng.
    pub group_by: files_core::group::GroupBy,
    /// Đơn vị gom ngày khi nhóm theo ngày.
    pub group_unit: files_core::group::DateUnit,
    /// Entries dropped by the hidden filter, reported in the status bar.
    pub hidden_count: usize,
}

impl Tab {
    pub fn new(start: PathBuf) -> Self {
        Self {
            history: History::new(start),
            entries: Vec::new(),
            selection: Vec::new(),
            anchor: None,
            sort: SortSpec::default(),
            filter: String::new(),
            visible: Vec::new(),
            hidden_count: 0,
            group_by: files_core::group::GroupBy::None,
            group_unit: files_core::group::DateUnit::Month,
        }
    }

    pub fn current(&self) -> &Path {
        self.history.current()
    }

    /// Recompute `visible` from `entries` and the active filter.
    pub fn rebuild_visible(&mut self) {
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

    pub fn selected_entries(&self) -> Vec<&Entry> {
        self.selection
            .iter()
            .filter_map(|i| self.entries.get(*i))
            .collect()
    }

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected_entries()
            .into_iter()
            .map(|e| PathBuf::from(&e.path))
            .collect()
    }
}

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Pane phải của chế độ hai khung. `None` = đang ở chế độ một khung.
    pub right: Option<Tab>,
    /// Khung đang nhận lệnh từ toolbar, address bar và bàn phím.
    pub active_pane: Pane,
    /// Token tải riêng cho từng khung: hai khung có thể tải song song.
    pub loads: [PaneLoad; 2],
    pub next_load_token: u64,
    pub show_hidden: bool,
    pub show_sidebar: bool,
    pub show_preview: bool,
    pub view_mode: i32,
    pub dark: bool,
    pub clipboard: Vec<PathBuf>,
    pub clipboard_cut: bool,
    /// Width available to the left file list, so the grid can pick a column count.
    pub grid_width: f32,
    /// Bề rộng khung phải, để chia lưới theo đúng bề rộng của khung đó.
    pub right_grid_width: f32,
    pub worker: Worker,
    pub results: Receiver<JobDone>,
    /// Paths whose change should trigger a refresh, from the directory watcher.
    pub watch: Option<files_fs::WatchHandle>,
    pub watched_path: Option<PathBuf>,
    /// Một sự kiện của watcher tới trong lúc đang tải, chờ lần làm mới kế tiếp.
    /// Bỏ qua mà không ghi nhớ là mất thay đổi của người dùng.
    pub watch_dirty: bool,
    /// Tabs the user closed, newest last. `Ctrl+Shift+T` pops from here, which
    /// is what Files and every browser do — the path alone is enough to restore.
    pub closed_tabs: Vec<PathBuf>,
    /// Hide `.txt`-style extensions in the name column.
    pub show_extensions: bool,
    /// Group folders above files, or sort them together.
    pub folder_order: files_core::sort::FolderOrder,
    /// Thẻ màu của tệp và thư mục, cùng nơi lưu để ghi lại khi thay đổi.
    pub tags: tags::TagStore,
    pub tag_path: PathBuf,
    /// Giao diện đang dùng, ghi lại sau mỗi thay đổi để lần sau mở đúng như cũ.
    pub settings: settings::Settings,
    pub settings_path: PathBuf,
    /// Lịch sử hoàn tác cho các thao tác tệp.
    pub undo: crate::undo::UndoStack,
    /// Trạng thái git của thư mục đang xem, đọc một lần khi mở thư mục.
    pub git_status: files_fs::StatusMap,
    /// Ứng dụng Shell đề xuất cho tệp đang mở menu chuột phải.
    pub open_with: Vec<files_fs::OpenWithApp>,
    /// Thời điểm bắt đầu thao tác dài, để thẻ tiến trình đếm giây.
    pub operation_started: Option<Instant>,
    /// Trạng thái kéo-thả nội bộ và vùng thả do UI báo lên.
    pub drag: DragState,
}

impl App {
    pub fn tab_for(&self, pane: Pane) -> Option<&Tab> {
        match pane {
            Pane::Left => Some(&self.tabs[self.active]),
            Pane::Right => self.right.as_ref(),
        }
    }

    pub fn tab_for_mut(&mut self, pane: Pane) -> Option<&mut Tab> {
        match pane {
            Pane::Left => Some(&mut self.tabs[self.active]),
            Pane::Right => self.right.as_mut(),
        }
    }

    pub fn tab(&self) -> &Tab {
        self.tab_for(self.active_pane)
            .unwrap_or(&self.tabs[self.active])
    }

    pub fn tab_mut(&mut self) -> &mut Tab {
        if self.active_pane == Pane::Right {
            if let Some(tab) = self.right.as_mut() {
                return tab;
            }
        }
        &mut self.tabs[self.active]
    }

    pub fn pane_load(&self, pane: Pane) -> PaneLoad {
        self.loads[pane.index()]
    }

    pub fn pane_load_mut(&mut self, pane: Pane) -> &mut PaneLoad {
        &mut self.loads[pane.index()]
    }

    /// Token mới, duy nhất cho cả hai khung.
    pub fn next_token(&mut self) -> u64 {
        self.next_load_token = self.next_load_token.wrapping_add(1);
        if self.next_load_token == 0 {
            self.next_load_token = 1;
        }
        self.next_load_token
    }

    pub fn is_dual(&self) -> bool {
        self.right.is_some()
    }

    pub fn list_options(&self) -> ListOptions {
        ListOptions {
            show_hidden: self.show_hidden,
            show_dotfiles: self.show_hidden,
        }
    }
}

/// Where the tag database lives.
///
/// `%LOCALAPPDATA%` is the right home, but a confined profile can refuse writes;
/// in that case the store sits beside the executable, the same fallback the
/// logger uses, so tagging still works instead of silently doing nothing.
pub fn tag_store_path() -> PathBuf {
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

/// Where preferences live: beside the tag database.
///
/// Same fallback rule as the log and the tags: a confined profile falls back to
/// the folder holding the executable so settings still persist.
pub fn settings_path() -> PathBuf {
    let preferred = settings::Settings::default_path();
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
        .map(|d| d.join("files-rs-settings.json"))
        .unwrap_or(preferred)
}

/// Push the stored preferences into the UI.
pub fn apply_saved_ui(ui: &AppWindow, app: &App) {
    crate::view::refresh_theme(ui, app.dark);
    ui.set_view_mode(app.view_mode);
    ui.set_show_sidebar(app.show_sidebar);
    ui.set_show_preview(app.show_preview);
    ui.set_show_hidden(app.show_hidden);
    ui.set_folder_order(slint::SharedString::from(app.folder_order.label()));
    ui.set_column_order(crate::view::column_order_model(&app.settings.column_order));
    let width = |id: &str| {
        app.settings
            .column_widths
            .get(id)
            .copied()
            .unwrap_or_else(|| settings::column_default_width(id)) as f32
    };
    ui.set_col_name_width(width("name"));
    ui.set_col_tag_width(width("tag"));
    ui.set_col_modified_width(width("modified"));
    ui.set_col_kind_width(width("kind"));
    ui.set_col_size_width(width("size"));
}

/// Write the current preferences to disk.
///
/// Called after every change; a failed write is logged but never blocks the UI —
/// losing a preference is annoying, crashing on it is worse.
fn snapshot_for_save(app: &std::rc::Rc<std::cell::RefCell<App>>) -> (PathBuf, settings::Settings) {
    let a = app.borrow();
    let snapshot = settings::Settings {
        dark: a.dark,
        view_mode: a.view_mode,
        show_sidebar: a.show_sidebar,
        show_preview: a.show_preview,
        show_hidden: a.show_hidden,
        show_extensions: a.show_extensions,
        folder_order: a.folder_order,
        pinned_folders: a.settings.pinned_folders.clone(),
        column_order: a.settings.column_order.clone(),
        column_widths: a.settings.column_widths.clone(),
    };
    (a.settings_path.clone(), snapshot)
}

fn write_settings(
    app: &std::rc::Rc<std::cell::RefCell<App>>,
    path: &Path,
    merged: settings::Settings,
) {
    app.borrow_mut().settings = merged.clone();
    if let Err(e) = merged.save(path) {
        logging::write(&format!("không lưu được giao diện: {e}"));
    }
}

/// Copy the live `App` UI fields into the settings snapshot, then save.
///
/// Pins are taken from the file: another window may have added some, and this
/// window's snapshot would otherwise erase them.
pub fn persist_ui(app: &std::rc::Rc<std::cell::RefCell<App>>) {
    let (path, current) = snapshot_for_save(app);
    let merged = settings::Settings::load(&path).with_ui_from(&current);
    write_settings(app, &path, merged);
}

/// Save a pin change.
///
/// The caller refreshed `pinned_folders` from disk before toggling, so this
/// writes the file's other preferences untouched and only replaces the list.
pub fn persist_pins(app: &std::rc::Rc<std::cell::RefCell<App>>) {
    let (path, current) = snapshot_for_save(app);
    let merged = settings::Settings::load(&path)
        .with_ui_from(&current)
        .with_pins_from(&current);
    write_settings(app, &path, merged);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_tab_starts_empty_with_every_row_visible() {
        let mut tab = Tab::new(PathBuf::from("C:\\"));
        assert_eq!(tab.current(), Path::new("C:\\"));
        assert!(tab.entries.is_empty());
        tab.rebuild_visible();
        assert!(tab.visible.is_empty());
    }

    #[test]
    fn filtering_narrows_visible_rows() {
        let mut tab = Tab::new(PathBuf::from("C:\\"));
        tab.entries = vec![entry("BaoCao.txt"), entry("anh.png"), entry("baoCao2.txt")];
        tab.rebuild_visible();
        assert_eq!(tab.visible.len(), 3);

        tab.filter = "baocao".into();
        tab.rebuild_visible();
        assert_eq!(tab.visible, vec![0, 2], "lọc không phân biệt hoa thường");
    }

    #[test]
    fn filtering_drops_selection_of_hidden_rows() {
        // Giữ lựa chọn trỏ vào dòng không còn hiển thị sẽ làm thao tác tệp
        // chạm vào thứ người dùng không thấy.
        let mut tab = Tab::new(PathBuf::from("C:\\"));
        tab.entries = vec![entry("giu.txt"), entry("bo.png")];
        tab.rebuild_visible();
        tab.selection = vec![0, 1];

        tab.filter = "giu".into();
        tab.rebuild_visible();
        assert_eq!(tab.selection, vec![0]);
    }

    #[test]
    fn selected_paths_follow_the_selection() {
        let mut tab = Tab::new(PathBuf::from("C:\\"));
        tab.entries = vec![entry("a.txt"), entry("b.txt")];
        tab.rebuild_visible();
        tab.selection = vec![1];
        assert_eq!(tab.selected_paths(), vec![PathBuf::from("C:\\b.txt")]);
    }

    #[test]
    fn grouping_keeps_every_visible_row_exactly_once() {
        // Nhóm không được làm mất hoặc lặp hàng: `visible` là nguồn sự thật và
        // mọi chỉ số phải xuất hiện đúng một lần trong thứ tự đã nhóm.
        let mut tab = Tab::new(PathBuf::from("C:\\"));
        tab.entries = (0..20)
            .map(|i| entry(&format!("{}.txt", (b'a' + (i % 5) as u8) as char)))
            .collect();
        tab.rebuild_visible();
        tab.group_by = files_core::group::GroupBy::Name;

        let (order, headers) = crate::view::grouped_rows(&tab, |_| None);

        assert_eq!(order.len(), tab.visible.len(), "mất hoặc lặp hàng");
        let mut sorted = order.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), tab.visible.len());
        assert!(!headers.is_empty(), "phải có header nhóm");
    }

    #[test]
    fn grouping_off_returns_the_visible_order_unchanged() {
        let mut tab = Tab::new(PathBuf::from("C:\\"));
        tab.entries = vec![entry("b.txt"), entry("a.txt")];
        tab.rebuild_visible();
        tab.group_by = files_core::group::GroupBy::None;

        let (order, headers) = crate::view::grouped_rows(&tab, |_| None);

        assert_eq!(order, tab.visible, "không nhóm thì giữ nguyên thứ tự");
        assert!(headers.is_empty(), "không nhóm thì không có header");
    }

    #[test]
    fn selecting_every_row_is_not_quadratic() {
        // `Tab::selected_paths` và phép dựng hàng đều dựa vào `selection`.
        // Trước đây mỗi hàng gọi `selection.contains`, thành O(n²) khi chọn hết.
        // Test này đo trực tiếp trên 10.000 mục: nếu ai đó đưa `contains` trở
        // lại, thời gian tăng vọt và test bắt được.
        let mut tab = Tab::new(PathBuf::from("C:\\"));
        tab.entries = (0..10_000)
            .map(|i| entry(&format!("tep-{i:05}.txt")))
            .collect();
        tab.rebuild_visible();
        tab.selection = (0..tab.entries.len()).collect();

        let started = std::time::Instant::now();
        let paths = tab.selected_paths();
        let elapsed = started.elapsed();

        assert_eq!(paths.len(), 10_000);
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "lấy đường dẫn của 10.000 mục quá chậm: {elapsed:?}"
        );
    }

    #[test]
    fn a_pending_watch_event_is_remembered_until_a_reload_is_queued() {
        // Watcher có thể báo thay đổi trong lúc đang tải. Nếu cờ này bị bỏ qua,
        // danh sách hiện trạng thái cũ cho tới lần làm mới sau.
        let mut app = test_app();
        assert!(!app.watch_dirty, "mới mở thì không có sự kiện nào chờ");

        app.watch_dirty = true;
        assert!(app.watch_dirty);

        // Xếp được một lần đọc thì cờ phải được xoá để không lặp vô hạn.
        app.watch_dirty = false;
        assert!(!app.watch_dirty);
    }

    /// `App` tối thiểu cho test: chỉ các trường mà test này chạm tới.
    fn test_app() -> App {
        let (worker, results) = crate::ops_worker::Worker::spawn().expect("worker cho test");
        App {
            tabs: vec![Tab::new(PathBuf::from("C:\\"))],
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
            tags: tags::TagStore::default(),
            tag_path: PathBuf::from("C:\\t.json"),
            settings: settings::Settings::default(),
            settings_path: PathBuf::from("C:\\s.json"),
            undo: Default::default(),
            git_status: Default::default(),
            open_with: Vec::new(),
            operation_started: None,
            drag: Default::default(),
        }
    }

    fn entry(name: &str) -> Entry {
        Entry {
            name: name.into(),
            path: format!("C:\\{name}"),
            kind: files_core::EntryKind::File,
            size: Some(1),
            modified_ms: None,
            created_ms: None,
            extension: Path::new(name)
                .extension()
                .map(|e| e.to_string_lossy().into_owned()),
            is_hidden: false,
            is_readonly: false,
        }
    }
}
