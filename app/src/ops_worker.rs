//! Background work: directory listing and file operations.
//!
//! A filesystem call on the UI thread freezes the window for as long as the
//! slowest volume takes to answer — a mapped network drive can take seconds.
//! Every blocking call therefore runs on a worker thread and reports back over a
//! channel that the UI drains on a timer.
//!
//! The queue is deliberately serial: file operations mutate shared state, and
//! running two deletes at once makes the "which one failed" report meaningless.

use files_core::sort::ListOptions;
use files_core::{FsError, Listing, SortSpec};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;

/// One unit of background work.
pub enum Job {
    /// Re-read a directory. `token` lets a stale reply be discarded when the
    /// user has already navigated elsewhere.
    Load {
        path: PathBuf,
        sort: SortSpec,
        opts: ListOptions,
        token: u64,
    },
    Copy {
        sources: Vec<PathBuf>,
        target: PathBuf,
        /// Cách xử lý khi đích đã tồn tại.
        policy: files_fs::ConflictPolicy,
    },
    Move {
        sources: Vec<PathBuf>,
        target: PathBuf,
        policy: files_fs::ConflictPolicy,
    },
    Delete {
        paths: Vec<PathBuf>,
    },
    CreateFolder {
        parent: PathBuf,
        name: String,
    },
    CreateFile {
        parent: PathBuf,
        name: String,
    },
    Rename {
        from: PathBuf,
        to: String,
    },
    /// Compress `sources` into `dest`.
    Zip {
        sources: Vec<PathBuf>,
        dest: PathBuf,
    },
    /// Extract `archive` into a folder next to it.
    Unzip {
        archive: PathBuf,
        target: PathBuf,
    },
    /// Hash one file; the result is a hex digest.
    Hash {
        path: PathBuf,
        kind: files_fs::HashKind,
    },
    /// Walk a folder to learn its total size.
    FolderSize {
        path: PathBuf,
    },
    /// Xoay ảnh 90° và ghi đè.
    Rotate {
        paths: Vec<PathBuf>,
    },
    /// Tìm đệ quy dưới một thư mục.
    Search {
        root: PathBuf,
        query: String,
        max_depth: usize,
        limit: usize,
        token: u64,
    },
    /// Delete without the Recycle Bin (Shift+Delete).
    DeletePermanent {
        paths: Vec<PathBuf>,
    },
}

/// Result of a job.
pub enum JobDone {
    Loaded {
        token: u64,
        path: PathBuf,
        result: Result<Listing, FsError>,
    },
    /// `label` is what the status bar shows; `result` carries an error when the
    /// whole batch failed.
    OpDone {
        label: String,
        result: Result<(), FsError>,
        /// Cách hoàn tác, khi thao tác này hoàn tác được.
        undo: Option<crate::undo::UndoAction>,
    },
    /// A folder's total size, so the UI can fill the size column in place.
    /// `token` identifies the directory listing that asked for it.
    FolderSize {
        path: PathBuf,
        bytes: u64,
        token: u64,
    },
    /// Kết quả tìm kiếm đệ quy.
    SearchDone {
        token: u64,
        root: PathBuf,
        hits: Vec<files_fs::SearchHit>,
        cancelled: bool,
    },
    /// A decoded Windows shell icon, ready for Slint.
    ShellIcon {
        path: PathBuf,
        icon: files_fs::RgbaIcon,
        token: u64,
    },
}

/// Tiến trình của thao tác đang chạy, chia sẻ giữa UI và luồng worker.
///
/// UI đọc bằng cách hỏi theo nhịp (`snapshot`) chứ không nhận thông điệp cho
/// từng tệp: một thư mục 10.000 tệp sẽ làm ngập kênh kết quả, và UI chỉ cần
/// vẽ lại vài chục lần mỗi giây là đủ.
///
/// `total == 0` nghĩa là không có thao tác nào đang chạy.
pub struct Progress {
    total: AtomicUsize,
    done: AtomicUsize,
    cancel: AtomicBool,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            total: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            cancel: AtomicBool::new(false),
        }
    }
}

impl Progress {
    /// Bắt đầu một thao tác gồm `total` mục.
    pub fn begin(&self, total: usize) {
        self.done.store(0, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
        // Đặt `total` SAU cùng: UI thấy total khác 0 là biết thao tác đã bắt đầu,
        // và lúc đó `done`/`cancel` đã ở trạng thái đúng.
        self.total.store(total, Ordering::Release);
    }

    /// Đánh dấu xong một mục.
    pub fn tick(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    /// Kết thúc thao tác (thành công, lỗi, hoặc bị huỷ).
    pub fn finish(&self) {
        self.total.store(0, Ordering::Release);
        self.done.store(0, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
    }

    /// Người dùng bấm Huỷ. Thao tác dừng ở ranh giới tệp kế tiếp.
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Đã được yêu cầu huỷ chưa. Worker hỏi trước mỗi tệp.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// `Some((đã xong, tổng))` khi có thao tác đang chạy.
    pub fn snapshot(&self) -> Option<(usize, usize)> {
        let total = self.total.load(Ordering::Acquire);
        if total == 0 {
            return None;
        }
        Some((self.done.load(Ordering::Relaxed), total))
    }
}

pub struct Worker {
    tx: Sender<Job>,
    /// Cosmetic background work: folder sizes and shell icons.
    ///
    /// These run on their own thread, never on the operation queue. They are
    /// best-effort decorations, and a slow one — a folder tree on a sleeping
    /// drive, a shortcut pointing at an unreachable network path — must never
    /// delay a create, a rename, or a delete. Putting them here was a real bug:
    /// one stuck icon made every later file operation wait forever, which reads
    /// to the user as "adding and renaming do not work".
    aux_tx: Sender<AuxJob>,
    /// Tiến trình của thao tác đang chạy, dùng chung với luồng worker.
    progress: Arc<Progress>,
}

/// Best-effort work that only decorates the list.
pub enum AuxJob {
    Size { path: PathBuf, token: u64 },
    Icon { path: PathBuf, size: u32, token: u64 },
}

impl Worker {
    /// Spawn the worker threads and return the handle plus the result channel.
    ///
    /// Fails only if the OS refuses to create a thread. The caller decides what
    /// to do with that; this function never panics.
    pub fn spawn() -> std::io::Result<(Self, Receiver<JobDone>)> {
        let (job_tx, job_rx) = channel::<Job>();
        let (done_tx, done_rx) = channel::<JobDone>();
        let main_done = done_tx.clone();
        let progress = Arc::new(Progress::default());
        let worker_progress = Arc::clone(&progress);

        thread::Builder::new()
            .name("files-worker".into())
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    let done = run(job, &worker_progress);
                    // A closed channel means the UI is shutting down; stop.
                    if main_done.send(done).is_err() {
                        break;
                    }
                }
            })?;

        // Second thread, same result channel: decorations are independent of
        // operations, and one slow folder must not stall the other.
        let (aux_tx, aux_rx) = channel::<AuxJob>();
        let aux_done = done_tx.clone();
        thread::Builder::new()
            .name("files-bg".into())
            .spawn(move || {
                while let Ok(job) = aux_rx.recv() {
                    let done = match job {
                        AuxJob::Size { path, token } => JobDone::FolderSize {
                            bytes: files_fs::directory_size(&path),
                            path,
                            token,
                        },
                        AuxJob::Icon { path, size, token } => {
                            match files_fs::shell_icon(&path, size) {
                                Ok(Some(icon)) => JobDone::ShellIcon { path, icon, token },
                                // No icon is not an error: the row keeps its glyph.
                                Ok(None) | Err(_) => continue,
                            }
                        }
                    };
                    if aux_done.send(done).is_err() {
                        break;
                    }
                }
            })?;

        Ok((Self { tx: job_tx, aux_tx, progress }, done_rx))
    }

    pub fn submit(&self, job: Job) {
        // A closed channel means the worker exited; the UI stays alive and the
        // user can retry, so dropping the job is the least-bad option.
        let _ = self.tx.send(job);
    }

    /// Tiến trình của thao tác đang chạy, nếu có.
    pub fn progress(&self) -> &Arc<Progress> {
        &self.progress
    }

    /// Queue a folder-size walk. Replies are tagged with `token`.
    pub fn submit_size(&self, path: PathBuf, token: u64) {
        let _ = self.aux_tx.send(AuxJob::Size { path, token });
    }

    /// Queue a Windows shell icon lookup. Replies are tagged with `token`.
    pub fn submit_icon(&self, path: PathBuf, size: u32, token: u64) {
        let _ = self.aux_tx.send(AuxJob::Icon { path, size, token });
    }
}

fn run(job: Job, progress: &Progress) -> JobDone {
    match job {
        Job::Load { path, sort, opts, token } => {
            let result = files_fs::read_dir(&path, sort, opts);
            JobDone::Loaded { token, path, result }
        }
        Job::Copy { sources, target, policy } => {
            let (created, err, cancelled) =
                run_batch(&sources, progress, |src| {
                    files_fs::copy_into_with(src, &target, policy)
                });
            let label = if cancelled {
                format!("Đã huỷ sau {}/{} mục", created.len(), sources.len())
            } else {
                match &err {
                    Some(e) => format!("Sao chép lỗi: {}", e.message),
                    None => format!("Đã sao chép {}/{} mục", created.len(), sources.len()),
                }
            };
            let undo = created.is_empty().then_some(()).map_or(
                Some(crate::undo::UndoAction::Copied {
                    sources: sources.clone(),
                    created,
                    target,
                }),
                |_| None,
            );
            JobDone::OpDone { label, result: err.map_or(Ok(()), Err), undo }
        }
        Job::Move { sources, target, policy } => {
            let (created, err, cancelled) =
                run_batch(&sources, progress, |src| {
                    files_fs::move_into_with(src, &target, policy)
                });
            let label = if cancelled {
                format!("Đã huỷ sau {}/{} mục", created.len(), sources.len())
            } else {
                match &err {
                    Some(e) => format!("Di chuyển lỗi: {}", e.message),
                    None => format!("Đã di chuyển {}/{} mục", created.len(), sources.len()),
                }
            };
            // Ghép nguồn với đích theo đúng thứ tự đã di chuyển thành công.
            let pairs: Vec<(PathBuf, PathBuf)> =
                sources.iter().cloned().zip(created.iter().cloned()).collect();
            let undo = if pairs.is_empty() {
                None
            } else {
                Some(crate::undo::UndoAction::Moved { pairs })
            };
            JobDone::OpDone { label, result: err.map_or(Ok(()), Err), undo }
        }
        Job::Delete { paths } => {
            let n = paths.len();
            match files_fs::delete_to_recycle_bin(&paths) {
                // Thùng rác đã là một dạng hoàn tác; không chồng thêm bản ghi.
                Ok(count) => JobDone::OpDone {
                    label: format!("Đã chuyển {count}/{n} mục vào Thùng rác"),
                    result: Ok(()),
                    undo: None,
                },
                Err(e) => JobDone::OpDone {
                    label: format!("Xoá lỗi: {}", e.message),
                    result: Err(e),
                    undo: None,
                },
            }
        }
        Job::CreateFolder { parent, name } => {
            let (label, result, undo) = match files_fs::create_folder(&parent, &name) {
                Ok(p) => (
                    format!("Đã tạo thư mục {}", file_label(&p)),
                    Ok(()),
                    Some(crate::undo::UndoAction::Created { path: p, is_dir: true }),
                ),
                Err(e) => (format!("Tạo thư mục lỗi: {}", e.message), Err(e), None),
            };
            JobDone::OpDone { label, result, undo }
        }
        Job::CreateFile { parent, name } => {
            let (label, result, undo) = match files_fs::create_file(&parent, &name) {
                Ok(p) => (
                    format!("Đã tạo tệp {}", file_label(&p)),
                    Ok(()),
                    Some(crate::undo::UndoAction::Created { path: p, is_dir: false }),
                ),
                Err(e) => (format!("Tạo tệp lỗi: {}", e.message), Err(e), None),
            };
            JobDone::OpDone { label, result, undo }
        }
        Job::Rename { from, to } => {
            let (label, result, undo) = match files_fs::rename(&from, &to) {
                Ok(p) => (
                    format!("Đã đổi tên thành {}", file_label(&p)),
                    Ok(()),
                    Some(crate::undo::UndoAction::Rename { from, to: p }),
                ),
                Err(e) => (format!("Đổi tên lỗi: {}", e.message), Err(e), None),
            };
            JobDone::OpDone { label, result, undo }
        }
        Job::Zip { sources, dest } => {
            let (label, result) = match files_fs::create_zip(&sources, &dest) {
                Ok(count) => (
                    format!("Đã nén {count} tệp vào {}", file_label(&dest)),
                    Ok(()),
                ),
                Err(e) => (format!("Nén lỗi: {}", e.message), Err(e)),
            };
            JobDone::OpDone { label, result, undo: None }
        }
        Job::Unzip { archive, target } => {
            let (label, result) = match files_fs::extract_zip(&archive, &target) {
                Ok(count) => (
                    format!("Đã giải nén {count} tệp vào {}", file_label(&target)),
                    Ok(()),
                ),
                Err(e) => (format!("Giải nén lỗi: {}", e.message), Err(e)),
            };
            JobDone::OpDone { label, result, undo: None }
        }
        Job::Hash { path, kind } => {
            let (label, result) = match files_fs::hash_file(&path, kind) {
                Ok(digest) => (format!("{}: {digest}", kind.label()), Ok(())),
                Err(e) => (format!("Tính mã băm lỗi: {}", e.message), Err(e)),
            };
            JobDone::OpDone { label, result, undo: None }
        }
        Job::Rotate { paths } => {
            progress.begin(paths.len());
            let mut done = 0usize;
            let mut first_err = None;
            for p in &paths {
                if progress.cancelled() {
                    break;
                }
                match files_fs::rotate_image_clockwise(p) {
                    Ok(()) => done += 1,
                    Err(e) => {
                        if first_err.is_none() {
                            first_err = Some(e);
                        }
                    }
                }
                progress.tick();
            }
            progress.finish();
            let label = match &first_err {
                Some(e) => format!("Xoay ảnh lỗi: {}", e.message),
                None => format!("Đã xoay {done}/{} ảnh", paths.len()),
            };
            JobDone::OpDone { label, result: first_err.map_or(Ok(()), Err), undo: None }
        }
        Job::Search { root, query, max_depth, limit, token } => {
            let opts = files_fs::SearchOptions { query, max_depth, limit };
            // Tìm kiếm không huỷ được ở đây: nó chạy trên luồng thao tác nên
            // không được chiếm chỗ lâu. Giới hạn độ sâu và số kết quả là chốt chặn.
            let (hits, cancelled) = files_fs::search(&root, &opts, || progress.cancelled());
            progress.finish();
            JobDone::SearchDone { token, root, hits, cancelled }
        }
        Job::FolderSize { path } => {
            // Explicit "tính kích thước" from the menu; token 0 never matches a
            // live listing, so the reply is applied by the manual path only.
            let bytes = files_fs::directory_size(&path);
            JobDone::FolderSize { path, bytes, token: 0 }
        }
        Job::DeletePermanent { paths } => {
            let n = paths.len();
            match files_fs::delete_permanently(&paths) {
                // Xoá vĩnh viễn KHÔNG hoàn tác được; hộp thoại đã cảnh báo.
                Ok(count) => JobDone::OpDone {
                    label: format!("Đã xoá vĩnh viễn {count}/{n} mục"),
                    result: Ok(()),
                    undo: None,
                },
                Err(e) => JobDone::OpDone {
                    label: format!("Xoá vĩnh viễn lỗi: {}", e.message),
                    result: Err(e),
                    undo: None,
                },
            }
        }
    }
}

/// Apply `op` to each source, counting successes and keeping the first error.
/// Áp `op` lên từng nguồn, báo tiến trình và dừng khi bị huỷ.
///
/// Trả về cả đường dẫn đích: hoàn tác cần biết chính xác cái gì đã được tạo.
/// Mục đã xong trước khi huỷ vẫn nằm trong danh sách trả về — người dùng phải
/// hoàn tác được đúng những gì đã thực sự xảy ra.
fn run_batch<F>(
    sources: &[PathBuf],
    progress: &Progress,
    mut op: F,
) -> (Vec<PathBuf>, Option<FsError>, bool)
where
    F: FnMut(&PathBuf) -> Result<PathBuf, FsError>,
{
    progress.begin(sources.len());
    let mut created = Vec::new();
    let mut first_err = None;
    let mut cancelled = false;
    for src in sources {
        // Dừng ở ranh giới tệp: không bao giờ bỏ dở một tệp đang chép.
        if progress.cancelled() {
            cancelled = true;
            break;
        }
        match op(src) {
            Ok(dest) => created.push(dest),
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
        progress.tick();
    }
    progress.finish();
    (created, first_err, cancelled)
}

fn file_label(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

#[cfg(test)]
mod tests {
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
        let (created, err, cancelled) = run_batch(&sources, p2, |src| {
            let r = files_fs::copy_into(src, &target);
            p2.request_cancel();
            r
        });

        assert!(cancelled, "phải báo là đã huỷ");
        assert!(err.is_none(), "huỷ không phải lỗi");
        assert_eq!(created.len(), 1, "dừng ngay sau tệp đang xử lý");
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
        let (created, err, cancelled) =
            run_batch(&sources, &p, |src| files_fs::copy_into(src, &target));

        assert!(!cancelled);
        assert!(err.is_none());
        assert_eq!(created.len(), 3);
        assert_eq!(p.snapshot(), None, "kết thúc thì không còn tiến trình");
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
}
