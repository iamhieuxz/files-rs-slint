//! Test của `ops_worker`. Tách từ `ops_worker.rs`.

use super::*;
use std::time::Duration;

fn scratch(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("đường dẫn exe");
    let dir = exe
        .parent()
        .expect("thư mục chứa exe")
        .join("test-scratch")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tạo thư mục tạm");
    dir
}

#[test]
fn a_fresh_progress_reports_nothing_running() {
    let p = Progress::default();
    assert_eq!(p.snapshot(), None);
}

#[test]
fn begin_makes_a_snapshot_available() {
    let p = Progress::default();
    p.begin(5);
    assert_eq!(p.snapshot(), Some((0, 5)));
}

#[test]
fn ticks_move_the_counter_and_stop_at_the_total() {
    let p = Progress::default();
    p.begin(3);
    p.tick();
    assert_eq!(p.snapshot(), Some((1, 3)));
    p.tick();
    p.tick();
    assert_eq!(p.snapshot(), Some((3, 3)), "không vượt quá tổng");
}

#[test]
fn finish_clears_the_snapshot() {
    let p = Progress::default();
    p.begin(2);
    p.tick();
    p.finish();
    assert_eq!(p.snapshot(), None);
}

#[test]
fn finish_also_resets_a_pending_cancel() {
    // Nếu không xoá cờ, thao tác SAU sẽ bị huỷ ngay từ tệp đầu tiên.
    let p = Progress::default();
    p.begin(2);
    p.request_cancel();
    assert!(p.cancelled());
    p.finish();
    assert!(!p.cancelled(), "cờ huỷ phải được xoá khi kết thúc");
}

#[test]
fn begin_clears_a_stale_cancel_from_the_previous_run() {
    let p = Progress::default();
    p.begin(2);
    p.request_cancel();
    p.begin(2);
    assert!(!p.cancelled(), "lần chạy mới không được thừa hưởng cờ cũ");
}

#[test]
fn cancelling_a_batch_stops_after_the_current_file() {
    let root = scratch("cancel-batch");
    let target = root.join("dich");
    std::fs::create_dir_all(&target).unwrap();
    let sources: Vec<PathBuf> = (0..5)
        .map(|i| {
            let p = root.join(format!("tep{i}.txt"));
            std::fs::write(&p, b"x").unwrap();
            p
        })
        .collect();

    let p = Progress::default();
    // Huỷ ngay sau tệp đầu tiên.
    let p2 = &p;
    let (done, err, cancelled) = run_batch(&sources, p2, |src| {
        let r = files_fs::copy_into(src, &target);
        p2.request_cancel();
        r
    });

    assert!(cancelled, "phải báo là đã huỷ");
    assert!(err.is_none(), "huỷ không phải lỗi");
    assert_eq!(done.len(), 1, "dừng ngay sau tệp đang xử lý");
    assert_eq!(
        std::fs::read_dir(&target).unwrap().count(),
        1,
        "chỉ đúng một tệp được chép"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_completed_batch_reports_every_file() {
    let root = scratch("full-batch");
    let target = root.join("dich");
    std::fs::create_dir_all(&target).unwrap();
    let sources: Vec<PathBuf> = (0..3)
        .map(|i| {
            let p = root.join(format!("t{i}.txt"));
            std::fs::write(&p, b"x").unwrap();
            p
        })
        .collect();

    let p = Progress::default();
    let (done, err, cancelled) = run_batch(&sources, &p, |src| files_fs::copy_into(src, &target));

    assert!(!cancelled);
    assert!(err.is_none());
    assert_eq!(done.len(), 3);
    assert_eq!(p.snapshot(), None, "kết thúc thì không còn tiến trình");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_partial_batch_keeps_each_source_with_its_own_destination() {
    // Nếu mục thứ hai lỗi, cặp của mục thứ ba không được ghép lệch sang
    // tên của mục thứ hai: làm lại/hoàn tác sẽ chạm nhầm tệp.
    let root = scratch("partial-batch");
    let sources: Vec<PathBuf> = (0..3)
        .map(|i| {
            let p = root.join(format!("t{i}.txt"));
            std::fs::write(&p, b"x").unwrap();
            p
        })
        .collect();

    let p = Progress::default();
    let (done, err, cancelled) = run_batch(&sources, &p, |src| {
        if src.ends_with("t1.txt") {
            return Err(FsError::new("io_error", "mục hai lỗi"));
        }
        Ok(root.join(format!(
            "dich-{}",
            src.file_name().unwrap().to_string_lossy()
        )))
    });

    assert!(!cancelled);
    assert!(err.is_some());
    assert_eq!(done.len(), 2, "chỉ hai mục thành công");
    assert_eq!(done[0].0, sources[0]);
    assert_eq!(done[0].1, root.join("dich-t0.txt"));
    assert_eq!(done[1].0, sources[2], "mục thứ ba phải giữ đúng nguồn");
    assert_eq!(done[1].1, root.join("dich-t2.txt"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_aux_result_carries_its_listing_token() {
    // Wrapper phải giữ token để UI bỏ được kết quả của thư mục cũ.
    // Dùng folder-size thay vì shell icon: kết quả tất định, không phụ
    // thuộc COM của Shell đang bận ở test khác.
    let root = scratch("aux-token");
    std::fs::write(root.join("a.txt"), b"x").unwrap();
    match run_aux(AuxJob::Size {
        path: root.clone(),
        token: 77,
    }) {
        Some(JobDone::FolderSize { token, bytes, .. }) => {
            assert_eq!(token, 77);
            assert_eq!(bytes, 1);
        }
        _ => panic!("phải trả kết quả kèm token"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_failed_thumbnail_is_silent() {
    // Ảnh hỏng không được sinh thông điệp: một OpDone ở đây làm pump
    // reload thư mục và lại xếp đúng job lỗi đó — vòng lặp vô hạn.
    let root = scratch("aux-thumb-silent");
    let fake = root.join("hong.png");
    std::fs::write(&fake, b"khong phai anh").unwrap();
    assert!(
        run_aux(AuxJob::Thumbnail {
            path: fake,
            size: 48,
            token: 1
        })
        .is_none(),
        "thumbnail lỗi phải im lặng"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_indeterminate_progress_is_visible_and_cancellable() {
    // Search không biết tổng số mục; nếu total = 0 thì UI coi như không có
    // thao tác và nút Huỷ không bao giờ hiện.
    let p = Progress::default();
    p.begin_indeterminate();
    let (done, total) = p.snapshot().expect("phải có tiến trình");
    assert_eq!(done, 0);
    assert!(total > done, "phải còn việc để huỷ");
    p.request_cancel();
    assert!(p.cancelled(), "worker phải thấy cờ huỷ");
    p.finish();
    assert_eq!(p.snapshot(), None, "kết thúc thì UI thôi hiện nút");
}

#[test]
fn undo_runs_on_the_worker_and_restores_the_old_name() {
    let root = scratch("worker-undo");
    let old = root.join("cu.txt");
    let new = root.join("moi.txt");
    std::fs::write(&old, b"x").unwrap();
    std::fs::rename(&old, &new).unwrap();
    let action = crate::undo::UndoAction::Rename {
        from: old.clone(),
        to: new.clone(),
    };

    let (worker, rx) = Worker::spawn().expect("worker");
    worker.submit(Job::Undo { action });
    match rx
        .recv_timeout(Duration::from_secs(10))
        .expect("worker phải trả lời")
    {
        JobDone::UndoDone { result, redo, .. } => {
            assert!(!redo);
            result.expect("hoàn tác phải thành công");
        }
        _ => panic!("cần UndoDone"),
    }
    assert!(old.exists(), "tên cũ phải trở lại");
    assert!(!new.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn redo_runs_on_the_worker_and_reapplies_the_rename() {
    let root = scratch("worker-redo");
    let old = root.join("cu.txt");
    let new = root.join("moi.txt");
    std::fs::write(&old, b"x").unwrap();
    let action = crate::undo::UndoAction::Rename {
        from: old.clone(),
        to: new.clone(),
    };

    let (worker, rx) = Worker::spawn().expect("worker");
    worker.submit(Job::Redo { action });
    match rx
        .recv_timeout(Duration::from_secs(10))
        .expect("worker phải trả lời")
    {
        JobDone::UndoDone { result, redo, .. } => {
            assert!(redo);
            result.expect("làm lại phải thành công");
        }
        _ => panic!("cần UndoDone"),
    }
    assert!(!old.exists());
    assert!(new.exists(), "tên mới phải được áp lại");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_worker_exposes_its_progress() {
    // UI đọc tiến trình qua worker, nên accessor phải trả cùng một đối tượng.
    let (worker, _rx) = Worker::spawn().expect("tạo worker");
    assert_eq!(worker.progress().snapshot(), None);
    worker.progress().begin(4);
    assert_eq!(worker.progress().snapshot(), Some((0, 4)));
    worker.progress().finish();
    let _ = Duration::from_millis(1);
}
