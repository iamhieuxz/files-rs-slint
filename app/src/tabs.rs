//! Tab operations: open, close, duplicate, cycle, and the group-close commands.
//!
//! Tab bookkeeping is easy to get subtly wrong — closing a tab to the left of the
//! active one must shift the active index, closing the last one must step back —
//! so the index arithmetic lives in a pure function with its own tests.

use crate::navigation::reload;
use crate::slint_generated::*;
use crate::state::{App, Pane, Tab};
use slint::{ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

pub fn sync_tabs(ui: &AppWindow, app: &App) {
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
    ui.set_active_pane(SharedString::from(app.active_pane.as_str()));
}

// ---------------------------------------------------------------- theme

/// Restore the most recently closed tab.
pub fn reopen_closed_tab(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let path = app.borrow_mut().closed_tabs.pop();
    let Some(path) = path else { return };
    // Không kiểm tra `is_dir()` trên UI thread: thư mục có thể đã bị xoá, và
    // `Job::Load` sẽ báo lỗi đó. Gọi đĩa đồng bộ ở đây làm treo cửa sổ với ổ
    // mạng đang ngủ.
    {
        let mut s = app.borrow_mut();
        s.tabs.push(Tab::new(path));
        s.active = s.tabs.len() - 1;
        s.active_pane = Pane::Left;
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

/// Open a copy of the active tab, keeping its folder and history.
pub fn duplicate_tab(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let start = app.borrow().tab().current().to_path_buf();
    {
        let mut s = app.borrow_mut();
        s.tabs.push(Tab::new(start));
        s.active = s.tabs.len() - 1;
        s.active_pane = Pane::Left;
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

/// Tab the context menu points at, if it is still a valid index.
pub fn menu_tab(ui: &AppWindow, app: &Rc<RefCell<App>>) -> Option<usize> {
    let idx = ui.get_menu_tab();
    let len = app.borrow().tabs.len();
    (idx >= 0 && (idx as usize) < len).then_some(idx as usize)
}

/// Make the menu's tab the active one, so the "close others" family acts on it.
pub fn focus_menu_tab(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    if let Some(idx) = menu_tab(ui, app) {
        let mut s = app.borrow_mut();
        s.active = idx;
        s.active_pane = Pane::Left;
    }
}

/// Close one tab by index, falling back to the active tab.
///
/// Mirrors what the × button does, so the menu and the button cannot drift
/// apart: same bookkeeping for `closed_tabs`, same clamp of `active`.
pub fn close_tab_at(ui: &AppWindow, app: &Rc<RefCell<App>>, target: Option<usize>) {
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
        s.active_pane = Pane::Left;
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
pub fn close_index_result(len: usize, active: usize, idx: usize) -> Option<(usize, usize)> {
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
pub fn cycle_tab(ui: &AppWindow, app: &Rc<RefCell<App>>, delta: i32) {
    {
        let mut s = app.borrow_mut();
        let n = s.tabs.len();
        if n <= 1 {
            return;
        }
        let cur = s.active as i32;
        s.active = (cur + delta).rem_euclid(n as i32) as usize;
        s.active_pane = Pane::Left;
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

pub fn close_other_tabs(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    {
        let mut s = app.borrow_mut();
        if s.tabs.len() <= 1 {
            return;
        }
        let keep = s.active;
        let kept = s.tabs.remove(keep);
        // Every removed tab becomes reopenable, oldest first.
        let removed: Vec<PathBuf> = s
            .tabs
            .drain(..)
            .map(|t| t.current().to_path_buf())
            .collect();
        s.closed_tabs.extend(removed);
        s.tabs.push(kept);
        s.active = 0;
        s.active_pane = Pane::Left;
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

/// Close every tab to the left (`left == true`) or right of the active one.
pub fn close_tabs_side(ui: &AppWindow, app: &Rc<RefCell<App>>, left: bool) {
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
        s.active_pane = Pane::Left;
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

/// Close everything and open one fresh tab, like Files' "Close all tabs".
pub fn close_all_tabs(ui: &AppWindow, app: &Rc<RefCell<App>>) {
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
        s.active_pane = Pane::Left;
    }
    sync_tabs(ui, &app.borrow());
    reload(ui, app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_a_tab_before_the_active_one_keeps_the_same_view() {
        // [A][B][C], đang ở C (2), đóng B (1) -> vẫn là C, nay ở vị trí 1.
        assert_eq!(close_index_result(3, 2, 1), Some((2, 1)));
    }

    #[test]
    fn closing_the_active_last_tab_selects_the_new_last() {
        assert_eq!(close_index_result(3, 2, 2), Some((2, 1)));
    }

    #[test]
    fn closing_a_tab_after_the_active_one_keeps_the_index() {
        assert_eq!(close_index_result(3, 0, 2), Some((2, 0)));
    }

    #[test]
    fn closing_the_active_middle_tab_shows_the_next_one() {
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
}
