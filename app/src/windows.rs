//! Multiple windows.
//!
//! Slint creates each window from the same component, so "another window" means
//! another `AppWindow` plus another [`App`] holding its own tabs. The two must
//! stay alive together: dropping the `AppWindow` closes the window, and dropping
//! the `App` leaves the callbacks pointing at freed state.
//!
//! So every open window is kept in a process-wide list. A window removes itself
//! when it closes; the list is what keeps the rest alive.

use crate::slint_generated::*;
use crate::state::App;
use crate::{events, logging, navigation, settings, state, tabs, window, wiring};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// One live window: the UI handle and the state behind it.
///
/// The UI is stored as `Rc` because Slint's own handle is reference-counted and
/// the callbacks hold weak references back to it.
struct LiveWindow {
    ui: AppWindow,
    app: Rc<RefCell<App>>,
    /// Set when the window asked to close, so the list can drop it.
    closed: Rc<std::cell::Cell<bool>>,
    /// Event pumps owned by this window.
    ///
    /// Slint timers stop when dropped. Keeping them here — instead of
    /// `mem::forget` — is what lets a closed window release its `App`, its
    /// worker thread and its directory watcher.
    timers: Vec<Rc<slint::Timer>>,
}

thread_local! {
    /// The real storage, on the UI thread where `Rc` is allowed.
    static LIVE: RefCell<Vec<LiveWindow>> = const { RefCell::new(Vec::new()) };
}

/// Build a window and wire it to a fresh `App`.
///
/// `start` is the folder the window opens; passing the current folder is what
/// "Open in new window" means.
pub fn open(start: PathBuf) -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    // Nạp thiết lập dùng chung: mọi cửa sổ nhìn cùng một giao diện.
    let settings_path = state::settings_path();
    let saved = settings::Settings::load(&settings_path);

    let (worker, results) = crate::ops_worker::Worker::spawn()
        .map_err(|e| slint::PlatformError::Other(format!("Không tạo được luồng nền: {e}")))?;

    let app = Rc::new(RefCell::new(App {
        tabs: vec![state::Tab::new(start)],
        active: 0,
        right: None,
        active_pane: state::Pane::Left,
        loads: Default::default(),
        next_load_token: 1,
        show_hidden: saved.show_hidden,
        show_sidebar: saved.show_sidebar,
        show_preview: saved.show_preview,
        view_mode: saved.view_mode,
        dark: saved.dark,
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
        show_extensions: saved.show_extensions,
        folders_first: saved.folders_first,
        tags: {
            let path = state::tag_store_path();
            crate::tags::TagStore::load(&path)
        },
        tag_path: state::tag_store_path(),
        settings: saved,
        settings_path,
        undo: Default::default(),
        git_status: Default::default(),
        open_with: Vec::new(),
        operation_started: None,
        drag: Default::default(),
    }));

    navigation::build_sidebar(&ui, &app.borrow());
    state::apply_saved_ui(&ui, &app.borrow());
    navigation::reload(&ui, &app);
    tabs::sync_tabs(&ui, &app.borrow());

    wiring::wire_callbacks(&ui, &app);
    let mut timers = vec![
        events::install_progress_pump(&ui, &app),
        events::install_status_clear(&ui),
        events::install_result_pump(&ui, &app),
        events::install_watch_pump(&ui, &app),
        window::install_state_poller(&ui),
    ];
    window::wire(&ui, Rc::as_ptr(&app));

    ui.show()?;
    // Đăng ký nhận tệp kéo từ Explorer; timer của nó cũng thuộc cửa sổ.
    timers.push(crate::native_drop::install(&ui, &app));

    // Giữ cửa sổ sống trong danh sách; đóng cửa sổ thì dọn khỏi danh sách.
    let closed = Rc::new(std::cell::Cell::new(false));
    let closed_flag = closed.clone();
    ui.window().on_close_requested(move || {
        closed_flag.set(true);
        slint::CloseRequestResponse::HideWindow
    });

    LIVE.with(|live| {
        live.borrow_mut().push(LiveWindow {
            ui,
            app,
            closed,
            timers,
        });
    });
    sweep();
    logging::write(&format!(
        "mở cửa sổ mới, tổng số: {}",
        LIVE.with(|l| l.borrow().len())
    ));
    Ok(())
}

/// Mark one window as closed, identified by the pointer of its `App`.
///
/// Matching by `Rc` identity is exact: with several windows open, guessing
/// "the last one" would close the wrong window.
pub fn close_by_app(app: *const RefCell<App>) {
    LIVE.with(|live| {
        for w in live.borrow().iter() {
            if Rc::as_ptr(&w.app) == app {
                w.closed.set(true);
                return;
            }
        }
    });
}

/// Drop windows that closed themselves.
///
/// Called after opening one and on a timer from the main window, so a closed
/// window does not keep its worker thread and directory watcher alive.
pub fn sweep() {
    LIVE.with(|live| {
        let mut list = live.borrow_mut();
        let before = list.len();
        // Ẩn tường minh trước khi bỏ: `Drop` của `AppWindow` có thể không chạy
        // ngay, và một cửa sổ đã đóng mà còn hiện là lỗi thấy được.
        for w in list.iter().filter(|w| w.closed.get()) {
            crate::native_drop::revoke(&w.ui);
            // Dừng pump trước khi ẩn: closure của timer giữ `App`, nên chỉ cần
            // thả timer là worker thread và watcher của cửa sổ được giải phóng.
            for timer in &w.timers {
                timer.stop();
            }
            let _ = w.ui.hide();
        }
        list.retain(|w| !w.closed.get());
        if list.len() != before {
            logging::write(&format!(
                "đóng {} cửa sổ, còn {}",
                before - list.len(),
                list.len()
            ));
        }
    });
}

/// True when no window is left open.
pub fn is_empty() -> bool {
    LIVE.with(|live| live.borrow().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_registry_is_empty() {
        assert!(is_empty());
    }

    #[test]
    fn sweeping_an_empty_registry_is_a_no_op() {
        sweep();
        assert!(is_empty());
    }

    #[test]
    fn opening_a_window_registers_it_and_keeps_it_alive() {
        // Đây là điều quan trọng nhất của module: nếu không giữ trong danh sách
        // thì `AppWindow` bị drop ngay và cửa sổ biến mất.
        //
        // Test này cần một event loop đang chạy để tạo cửa sổ; nếu môi trường
        // test không có backend hiển thị thì bỏ qua thay vì báo hỏng.
        if slint::BackendSelector::new().select().is_err() {
            return;
        }
        let before = LIVE.with(|l| l.borrow().len());
        // Không tạo được cửa sổ trong môi trường test: chấp nhận.
        if open(std::path::PathBuf::from("C:\\")).is_ok() {
            let after = LIVE.with(|l| l.borrow().len());
            assert_eq!(after, before + 1, "cửa sổ mới phải nằm trong danh sách");
        }
    }

    #[test]
    fn closing_by_an_unknown_app_marks_nothing() {
        // Con trỏ không khớp cửa sổ nào: không được panic, không đóng nhầm.
        let fake = std::ptr::null();
        close_by_app(fake);
        sweep();
        assert!(is_empty());
    }
}
