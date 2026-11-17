slint::include_modules!();

mod preview;

use files_core::{Entry, EntryKind, ListOptions, SortBy, SortDirection, SortSpec};
use files_fs::{list_drives, read_dir};
use slint::{ModelRc, SharedString, VecModel};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::cell::RefCell;

// Trạng thái của một tab: lịch sử điều hướng, thư mục hiện tại, danh sách mục.
struct TabState {
    history: Vec<PathBuf>,
    cursor: usize,
    current: PathBuf,
    sort: SortSpec,
    entries: Vec<Entry>,
}

impl TabState {
    fn new(start: PathBuf) -> Self {
        Self {
            history: vec![start.clone()],
            cursor: 0,
            current: start,
            sort: SortSpec::default(),
            entries: Vec::new(),
        }
    }

    fn push(&mut self, p: PathBuf) {
        self.history.truncate(self.cursor + 1);
        self.history.push(p.clone());
        self.cursor += 1;
        self.current = p;
    }

    fn back(&mut self) -> Option<PathBuf> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        self.current = self.history[self.cursor].clone();
        Some(self.current.clone())
    }

    fn forward(&mut self) -> Option<PathBuf> {
        if self.cursor + 1 >= self.history.len() {
            return None;
        }
        self.cursor += 1;
        self.current = self.history[self.cursor].clone();
        Some(self.current.clone())
    }
}

// Toàn bộ trạng thái ứng dụng: nhiều tab, mỗi tab có TabState riêng.
struct State {
    tabs: Vec<TabState>,
    active: usize,
}

impl State {
    fn new(start: PathBuf) -> Self {
        Self { tabs: vec![TabState::new(start)], active: 0 }
    }

    fn tab(&self) -> &TabState {
        &self.tabs[self.active]
    }

    fn tab_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active]
    }
}

fn format_size(bytes: Option<u64>) -> String {
    match bytes {
        None => String::new(),
        Some(b) if b < 1024 => format!("{b} B"),
        Some(b) if b < 1024 * 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        Some(b) if b < 1024 * 1024 * 1024 => format!("{:.1} MB", b as f64 / 1024.0 / 1024.0),
        Some(b) => format!("{:.2} GB", b as f64 / 1024.0 / 1024.0 / 1024.0),
    }
}

fn format_time(ms: Option<i64>) -> String {
    match ms.and_then(chrono::DateTime::from_timestamp_millis) {
        Some(dt) => dt
            .with_timezone(&chrono::Local)
            .format("%d/%m/%Y %H:%M")
            .to_string(),
        None => String::new(),
    }
}

fn kind_label(e: &Entry) -> String {
    match e.kind {
        EntryKind::Directory => "Thư mục".into(),
        EntryKind::Symlink => "Liên kết".into(),
        EntryKind::Other => "Khác".into(),
        EntryKind::File => e
            .extension
            .as_ref()
            .map(|x| format!("Tệp {}", x.to_uppercase()))
            .unwrap_or_else(|| "Tệp".into()),
    }
}

fn sort_from_column(column: &str, current: SortSpec) -> SortSpec {
    let by = match column {
        "size" => SortBy::Size,
        "modified" => SortBy::DateModified,
        _ => SortBy::Name,
    };
    let direction = if current.by == by && current.direction == SortDirection::Ascending {
        SortDirection::Descending
    } else {
        SortDirection::Ascending
    };
    SortSpec { by, direction, folders_first: true }
}

fn apply_view(ui: &AppWindow, state: &State) {
    let tab = state.tab();
    let rows: Vec<FileRow> = tab
        .entries
        .iter()
        .map(|e| FileRow {
            name: SharedString::from(e.name.as_str()),
            path: SharedString::from(e.path.as_str()),
            is_dir: e.is_dir(),
            size: SharedString::from(format_size(e.size).as_str()),
            modified: SharedString::from(format_time(e.modified_ms).as_str()),
            kind: SharedString::from(kind_label(e).as_str()),
        })
        .collect();

    let folders = tab.entries.iter().filter(|e| e.is_dir()).count();
    let files = tab.entries.len() - folders;
    ui.set_rows(ModelRc::new(VecModel::from(rows)));
    ui.set_current_path(SharedString::from(tab.current.to_string_lossy().as_ref()));
    ui.set_address_text(SharedString::from(tab.current.to_string_lossy().as_ref()));
    ui.set_can_back(tab.cursor > 0);
    ui.set_can_forward(tab.cursor + 1 < tab.history.len());
    ui.set_sort_column(SharedString::from(match tab.sort.by {
        SortBy::Size => "size",
        SortBy::DateModified => "modified",
        _ => "name",
    }));
    ui.set_sort_desc(tab.sort.direction == SortDirection::Descending);
    ui.set_status_text(SharedString::from(format!(
        "{folders} thư mục, {files} tệp"
    ).as_str()));
}

fn load(ui: &AppWindow, state: &Rc<RefCell<State>>) {
    let (path, sort) = {
        let s = state.borrow();
        (s.tab().current.clone(), s.tab().sort)
    };
    ui.set_selected_index(-1);
    preview::clear(ui);
    match read_dir(&path, sort, ListOptions::default()) {
        Ok(listing) => {
            let mut s = state.borrow_mut();
            s.tab_mut().entries = listing.entries;
            apply_view(ui, &s);
            ui.set_error_text(SharedString::default());
            if !listing.partial_errors.is_empty() {
                ui.set_status_text(SharedString::from(
                    format!("Một số mục không đọc được: {}", listing.partial_errors.len()).as_str(),
                ));
            }
        }
        Err(e) => {
            let mut s = state.borrow_mut();
            s.tab_mut().entries.clear();
            apply_view(ui, &s);
            ui.set_error_text(SharedString::from(e.message.as_str()));
        }
    }
}

fn navigate_to(ui: &AppWindow, state: &Rc<RefCell<State>>, target: PathBuf) {
    state.borrow_mut().tab_mut().push(target);
    load(ui, state);
}

// Đồng bộ danh sách tab sang UI. Tiêu đề lấy từ tên thư mục hiện tại.
fn sync_tabs(ui: &AppWindow, state: &State) {
    let tabs: Vec<TabInfo> = state
        .tabs
        .iter()
        .map(|t| {
            let title = t
                .current
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| t.current.to_string_lossy().into_owned());
            TabInfo {
                title: SharedString::from(title.as_str()),
                path: SharedString::from(t.current.to_string_lossy().as_ref()),
            }
        })
        .collect();
    ui.set_tabs(ModelRc::new(VecModel::from(tabs)));
    ui.set_active_tab(state.active as i32);
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\"));
    let state = Rc::new(RefCell::new(State::new(home.clone())));

    // Sidebar contents
    let quick: Vec<NavEntry> = files_fs::drives::quick_access()
        .into_iter()
        .map(|(label, path)| NavEntry {
            label: SharedString::from(label.as_str()),
            path: SharedString::from(path.as_str()),
        })
        .collect();
    ui.set_quick_access(ModelRc::new(VecModel::from(quick)));

    let drives: Vec<NavEntry> = list_drives()
        .into_iter()
        .map(|d| NavEntry {
            label: SharedString::from(format!("{} ({})", d.label, d.name.unwrap_or_default()).trim_end_matches(" ()")),
            path: SharedString::from(d.path.as_str()),
        })
        .collect();
    ui.set_drives(ModelRc::new(VecModel::from(drives)));

    load(&ui, &state);

    {
        let (ui_w, st) = (ui.as_weak(), state.clone());
        ui.on_navigate(move |p| {
            if let Some(ui) = ui_w.upgrade() {
                navigate_to(&ui, &st, PathBuf::from(p.as_str()));
            }
        });
    }
    {
        let (ui_w, st) = (ui.as_weak(), state.clone());
        ui.on_open_row(move |i| {
            let Some(ui) = ui_w.upgrade() else { return };
            let target = {
                let s = st.borrow();
                s.tab().entries.get(i as usize).map(|e| (e.is_dir(), PathBuf::from(&e.path)))
            };
            if let Some((is_dir, path)) = target {
                if is_dir {
                    navigate_to(&ui, &st, path);
                } else if let Err(e) = open::that_detached(&path) {
                    ui.set_error_text(SharedString::from(format!("Không mở được tệp: {e}").as_str()));
                }
            }
        });
    }
    {
        let (ui_w, st) = (ui.as_weak(), state.clone());
        ui.on_go_back(move || {
            if let Some(ui) = ui_w.upgrade() {
                let next = st.borrow_mut().tab_mut().back();
                if next.is_some() {
                    load(&ui, &st);
                }
            }
        });
    }
    {
        let (ui_w, st) = (ui.as_weak(), state.clone());
        ui.on_go_forward(move || {
            if let Some(ui) = ui_w.upgrade() {
                let next = st.borrow_mut().tab_mut().forward();
                if next.is_some() {
                    load(&ui, &st);
                }
            }
        });
    }
    {
        let (ui_w, st) = (ui.as_weak(), state.clone());
        ui.on_go_up(move || {
            if let Some(ui) = ui_w.upgrade() {
                let parent = {
                    let s = st.borrow();
                    s.tab().current.parent().map(Path::to_path_buf)
                };
                if let Some(p) = parent {
                    navigate_to(&ui, &st, p);
                }
            }
        });
    }
    {
        let (ui_w, st) = (ui.as_weak(), state.clone());
        ui.on_refresh(move || {
            if let Some(ui) = ui_w.upgrade() {
                load(&ui, &st);
            }
        });
    }
    {
        let (ui_w, st) = (ui.as_weak(), state.clone());
        ui.on_address_submitted(move |text| {
            if let Some(ui) = ui_w.upgrade() {
                let p = files_core::path::normalize(text.as_str());
                if p.is_dir() {
                    navigate_to(&ui, &st, p);
                } else {
                    ui.set_error_text(SharedString::from(format!("Không tìm thấy thư mục: {}", p.display()).as_str()));
                }
            }
        });
    }
    {
        let (ui_w, st) = (ui.as_weak(), state.clone());
        ui.on_sort_by(move |column| {
            if let Some(ui) = ui_w.upgrade() {
                {
                    let mut s = st.borrow_mut();
                    let t = s.tab_mut();
                    t.sort = sort_from_column(column.as_str(), t.sort);
                    let sort = t.sort;
                    sort.sort(&mut t.entries);
                }
                let s = st.borrow();
                apply_view(&ui, &s);
            }
        });
    }

    {
        let (ui_w, st) = (ui.as_weak(), state.clone());
        ui.on_select_row(move |i| {
            let Some(ui) = ui_w.upgrade() else { return };
            let s = st.borrow();
            match s.tab().entries.get(i as usize) {
                Some(entry) => {
                    ui.set_selected_index(i);
                    preview::show(&ui, entry);
                }
                None => {
                    ui.set_selected_index(-1);
                    preview::clear(&ui);
                }
            }
        });
    }

    // Tab thật: mỗi tab có TabState riêng (thư mục, lịch sử, sắp xếp, danh sách).
    sync_tabs(&ui, &state.borrow());
    {
        let (ui_w, st) = (ui.as_weak(), state.clone());
        ui.on_new_tab(move || {
            if let Some(ui) = ui_w.upgrade() {
                let home_dir = {
                    let s = st.borrow();
                    s.tab().current.clone()
                };
                {
                    let mut s = st.borrow_mut();
                    s.tabs.push(TabState::new(home_dir));
                    s.active = s.tabs.len() - 1;
                }
                sync_tabs(&ui, &st.borrow());
                load(&ui, &st);
            }
        });
    }
    {
        let (ui_w, st) = (ui.as_weak(), state.clone());
        ui.on_close_tab(move |i| {
            let Some(ui) = ui_w.upgrade() else { return };
            {
                let mut s = st.borrow_mut();
                if s.tabs.len() <= 1 {
                    return;
                }
                let i = i as usize;
                if i >= s.tabs.len() {
                    return;
                }
                s.tabs.remove(i);
                if s.active >= s.tabs.len() {
                    s.active = s.tabs.len() - 1;
                } else if s.active > i {
                    s.active -= 1;
                }
            }
            sync_tabs(&ui, &st.borrow());
            load(&ui, &st);
        });
    }
    {
        let (ui_w, st) = (ui.as_weak(), state.clone());
        ui.on_select_tab(move |i| {
            let Some(ui) = ui_w.upgrade() else { return };
            {
                let mut s = st.borrow_mut();
                if (i as usize) < s.tabs.len() {
                    s.active = i as usize;
                }
            }
            sync_tabs(&ui, &st.borrow());
            load(&ui, &st);
        });
    }

    ui.run()
}
