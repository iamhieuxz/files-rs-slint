// Không mở cửa sổ console: app gốc không có console. Thuộc tính phải đứng
// trước mọi item khác, nên đặt ngay đầu tệp.
#![cfg_attr(windows, windows_subsystem = "windows")]
// Code production không được `unwrap`/`expect`: lỗi phải đi qua `Result`.
// Test được phép dùng (cfg(test) không bị ảnh hưởng).
// Mã do Slint sinh ra nằm ngoài lint này: xem `allow` trên `include_modules`.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
// Code production không được `unwrap`/`expect`: lỗi phải đi qua `Result` và
// hiển thị cho người dùng. Test được phép dùng (cfg(test) không bị ảnh hưởng).
// Mã do Slint sinh ra dùng `unwrap` nội bộ; nằm ngoài lint của mình.
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod slint_generated {
    slint::include_modules!();
}
use slint_generated::*;

mod commands;
mod dialogs;
mod events;
mod file_ops;
mod format;
mod history;
mod logging;
mod ops_worker;
mod preview;
mod settings;
mod selection;
mod state;
mod tabs;
mod navigation;
mod view;
mod tags;
mod window;
mod wiring;

use wiring::wire_callbacks;
use navigation::{build_sidebar, reload};
use state::{apply_saved_ui, settings_path, tag_store_path, App, Tab};
use tabs::sync_tabs;
use events::{install_result_pump, install_watch_pump};
use ops_worker::Worker;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

fn main() -> Result<(), slint::PlatformError> {
    // Không có console nên stderr mất hút; ghi log ra tệp để còn chẩn đoán.
    logging::install_panic_hook();
    // `files-fs` không tự biết ghi log ở đâu; cài sink để lỗi Win32 không mất.
    files_fs::set_log_sink(logging::write);
    logging::write(&format!(
        "khởi động, log tại {:?}",
        logging::log_path()
    ));

    let ui = AppWindow::new()?;
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\"));

    // Nạp giao diện lần trước trước khi dựng App, để mọi thứ nhất quán ngay
    // từ khung hình đầu tiên thay vì chớp sang nền sáng rồi mới đổi.
    let settings_path = settings_path();
    let saved = settings::Settings::load(&settings_path);

    // Không tạo được luồng nền thì app không thể hoạt động: báo lỗi và thoát
    // bằng `?`, không panic.
    let (worker, results) = Worker::spawn().map_err(|e| {
        slint::PlatformError::Other(format!("Không tạo được luồng nền: {e}"))
    })?;
    let app = Rc::new(RefCell::new(App {
        tabs: vec![Tab::new(home.clone())],
        active: 0,
        show_hidden: saved.show_hidden,
        show_sidebar: saved.show_sidebar,
        show_preview: saved.show_preview,
        view_mode: saved.view_mode,
        dark: saved.dark,
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
        show_extensions: saved.show_extensions,
        folders_first: saved.folders_first,
        tags: {
            let path = tag_store_path();
            tags::TagStore::load(&path)
        },
        tag_path: tag_store_path(),
        settings: saved,
        settings_path,
    }));

    build_sidebar(&ui);
    // Áp giao diện đã lưu TRƯỚC khung hình đầu tiên, để không bị chớp sáng
    // rồi mới tối.
    apply_saved_ui(&ui, &app.borrow());
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

// ---------------------------------------------------------------- sidebar

#[cfg(test)]
mod tests {
    use super::*;
    use commands::{filter_palette, PALETTE};
    use ops_worker::{Job, JobDone};
    use std::time::Duration;

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
        let (worker, rx) = Worker::spawn().expect("tạo luồng worker cho test");

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
        let (worker, rx) = Worker::spawn().expect("tạo luồng worker cho test");
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
        let (worker, rx) = Worker::spawn().expect("tạo luồng worker cho test");

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








}
