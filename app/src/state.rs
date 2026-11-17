//! Application state.
//!
//! Two structures: [`Tab`] owns everything one tab is looking at, [`App`] owns
//! the tabs and the process-wide choices (theme, panels, clipboard, tag store).
//!
//! Deliberately free of Slint callbacks and of any business operation: this
//! module only *holds* state and offers the small accessors the other modules
//! need. That keeps the dependency direction one-way — `state` depends on
//! nothing of the UI.

use files_core::sort::ListOptions;
use files_core::{Entry, SortSpec};
use crate::history::History;
use crate::ops_worker::{JobDone, Worker};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use crate::slint_generated::*;
use crate::{logging, settings, tags};

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
    pub show_hidden: bool,
    pub show_sidebar: bool,
    pub show_preview: bool,
    pub view_mode: i32,
    pub dark: bool,
    pub clipboard: Vec<PathBuf>,
    pub clipboard_cut: bool,
    /// Bumped on every navigation so a stale listing reply is ignored.
    pub load_token: u64,
    /// True while a directory read is in flight, so the watcher cannot stack
    /// another reload on top of it.
    pub load_in_flight: bool,
    /// Width available to the file list, so the grid can pick a column count.
    pub grid_width: f32,
    pub worker: Worker,
    pub results: Receiver<JobDone>,
    /// Paths whose change should trigger a refresh, from the directory watcher.
    pub watch: Option<files_fs::WatchHandle>,
    pub watched_path: Option<PathBuf>,
    /// Tabs the user closed, newest last. `Ctrl+Shift+T` pops from here, which
    /// is what Files and every browser do — the path alone is enough to restore.
    pub closed_tabs: Vec<PathBuf>,
    /// Hide `.txt`-style extensions in the name column.
    pub show_extensions: bool,
    /// Group folders above files, or sort them together.
    pub folders_first: bool,
    /// Thẻ màu của tệp và thư mục, cùng nơi lưu để ghi lại khi thay đổi.
    pub tags: tags::TagStore,
    pub tag_path: PathBuf,
    /// Giao diện đang dùng, ghi lại sau mỗi thay đổi để lần sau mở đúng như cũ.
    pub settings: settings::Settings,
    pub settings_path: PathBuf,
    /// Lịch sử hoàn tác cho các thao tác tệp.
    pub undo: crate::undo::UndoStack,
}

impl App {
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    pub fn list_options(&self) -> ListOptions {
        ListOptions { show_hidden: self.show_hidden, show_dotfiles: self.show_hidden }
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
    ui.set_folders_first(app.folders_first);
}

/// Write the current preferences to disk.
///
/// Called after every change; a failed write is logged but never blocks the UI —
/// losing a preference is annoying, crashing on it is worse.
pub fn save_settings(app: &std::rc::Rc<std::cell::RefCell<App>>) {
    let (path, snapshot) = {
        let a = app.borrow();
        (a.settings_path.clone(), a.settings.clone())
    };
    if let Err(e) = snapshot.save(&path) {
        logging::write(&format!("không lưu được giao diện: {e}"));
    }
}

/// Copy the live `App` fields into the settings snapshot, then save.
pub fn persist_ui(app: &std::rc::Rc<std::cell::RefCell<App>>) {
    {
        let mut a = app.borrow_mut();
        let s = settings::Settings {
            dark: a.dark,
            view_mode: a.view_mode,
            show_sidebar: a.show_sidebar,
            show_preview: a.show_preview,
            show_hidden: a.show_hidden,
            show_extensions: a.show_extensions,
            folders_first: a.folders_first,
            // Giữ danh sách ghim đã có: hàm này chỉ đồng bộ phần giao diện.
            pinned_folders: a.settings.pinned_folders.clone(),
        };
        a.settings = s;
    }
    save_settings(app);
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
        tab.entries = vec![
            entry("BaoCao.txt"),
            entry("anh.png"),
            entry("baoCao2.txt"),
        ];
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
