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
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;

mod progress;
pub use progress::Progress;

#[cfg(test)]
mod tests;

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
    /// Đọc trạng thái git của một thư mục.
    GitStatus {
        dir: PathBuf,
        token: u64,
    },
    /// Xoay ảnh 90° và ghi đè.
    Rotate {
        paths: Vec<PathBuf>,
        /// `true` là ngược chiều kim đồng hồ (xoay trái).
        counter_clockwise: bool,
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
    /// Apply the inverse of an action off the UI thread.
    Undo {
        action: crate::undo::UndoAction,
    },
    /// Re-apply an action after an undo.
    Redo {
        action: crate::undo::UndoAction,
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
    /// Trạng thái git của một thư mục.
    GitStatus {
        dir: PathBuf,
        status: files_fs::StatusMap,
        token: u64,
    },
    /// Ảnh thu nhỏ đã giải mã.
    Thumbnail {
        path: PathBuf,
        image: files_fs::RgbaIcon,
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
    /// Dữ liệu nặng của panel xem trước, đọc trên luồng nền.
    Preview {
        path: PathBuf,
        data: files_fs::PreviewData,
        token: u64,
    },
    /// Kết quả hoàn tác/làm lại; UI dời action giữa hai ngăn xếp khi thành công.
    UndoDone {
        action: crate::undo::UndoAction,
        redo: bool,
        result: Result<(), FsError>,
    },
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
    Size {
        path: PathBuf,
        token: u64,
    },
    Icon {
        path: PathBuf,
        size: u32,
        token: u64,
    },
    Thumbnail {
        path: PathBuf,
        size: u32,
        token: u64,
    },
    /// Đọc dữ liệu nặng cho panel xem trước: số mục của thư mục, ảnh, hoặc
    /// đoạn văn bản đầu tệp. `token` chống kết quả cũ ghi đè lựa chọn mới.
    Preview {
        path: PathBuf,
        kind: files_fs::PreviewKind,
        token: u64,
    },
}

/// Run one decoration job. `None` means "nothing worth telling the UI".
///
/// Failures are deliberately silent: a corrupt image or an unreadable icon must
/// not look like a file operation, because the result pump would reload the
/// folder and queue the same failing job again — an endless refresh loop.
fn run_aux(job: AuxJob) -> Option<JobDone> {
    match job {
        AuxJob::Size { path, token } => Some(JobDone::FolderSize {
            bytes: files_fs::directory_size(&path),
            path,
            token,
        }),
        AuxJob::Icon { path, size, token } => files_fs::shell_icon(&path, size)
            .ok()
            .flatten()
            .map(|icon| JobDone::ShellIcon { path, icon, token }),
        AuxJob::Thumbnail { path, size, token } => files_fs::thumbnail_cached(&path, size)
            .ok()
            .flatten()
            .map(|image| JobDone::Thumbnail { path, image, token }),
        AuxJob::Preview { path, kind, token } => Some(JobDone::Preview {
            data: files_fs::preview_data(&path, kind),
            path,
            token,
        }),
    }
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
                    let Some(done) = run_aux(job) else {
                        continue;
                    };
                    if aux_done.send(done).is_err() {
                        break;
                    }
                }
            })?;

        Ok((
            Self {
                tx: job_tx,
                aux_tx,
                progress,
            },
            done_rx,
        ))
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

    /// Queue an image thumbnail. Decoration work stays off the operation queue
    /// so a folder of photos cannot delay a rename or a paste.
    pub fn submit_thumbnail(&self, path: PathBuf, size: u32, token: u64) {
        let _ = self.aux_tx.send(AuxJob::Thumbnail { path, size, token });
    }

    /// Queue the heavy part of the preview panel (image decode, text read,
    /// folder entry count) so selecting a row never touches the disk on the UI
    /// thread.
    pub fn submit_preview(&self, path: PathBuf, kind: files_fs::PreviewKind, token: u64) {
        let _ = self.aux_tx.send(AuxJob::Preview { path, kind, token });
    }
}

fn run(job: Job, progress: &Progress) -> JobDone {
    match job {
        Job::Load {
            path,
            sort,
            opts,
            token,
        } => {
            let result = files_fs::read_dir(&path, sort, opts);
            JobDone::Loaded {
                token,
                path,
                result,
            }
        }
        Job::Copy {
            sources,
            target,
            policy,
        } => {
            let (done, err, cancelled) = run_batch(&sources, progress, |src| {
                files_fs::copy_into_with(src, &target, policy)
            });
            let count = done.len();
            let label = if cancelled {
                format!("Đã huỷ sau {count}/{} mục", sources.len())
            } else {
                match &err {
                    Some(e) => format!("Sao chép lỗi: {}", e.message),
                    None => format!("Đã sao chép {count}/{} mục", sources.len()),
                }
            };
            // Chỉ ghi những nguồn thật sự đã chép được, theo đúng cặp của nó:
            // làm lại sau một batch lỗi một phần không được chép lại mục đã lỗi.
            let (sources_done, created): (Vec<_>, Vec<_>) = done.into_iter().unzip();
            let undo = if created.is_empty() {
                None
            } else {
                Some(crate::undo::UndoAction::Copied {
                    sources: sources_done,
                    created,
                    target,
                })
            };
            JobDone::OpDone {
                label,
                result: err.map_or(Ok(()), Err),
                undo,
            }
        }
        Job::Move {
            sources,
            target,
            policy,
        } => {
            let (pairs, err, cancelled) = run_batch(&sources, progress, |src| {
                files_fs::move_into_with(src, &target, policy)
            });
            let count = pairs.len();
            let label = if cancelled {
                format!("Đã huỷ sau {count}/{} mục", sources.len())
            } else {
                match &err {
                    Some(e) => format!("Di chuyển lỗi: {}", e.message),
                    None => format!("Đã di chuyển {count}/{} mục", sources.len()),
                }
            };
            let undo = if pairs.is_empty() {
                None
            } else {
                Some(crate::undo::UndoAction::Moved { pairs })
            };
            JobDone::OpDone {
                label,
                result: err.map_or(Ok(()), Err),
                undo,
            }
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
                    Some(crate::undo::UndoAction::Created {
                        path: p,
                        is_dir: true,
                    }),
                ),
                Err(e) => (format!("Tạo thư mục lỗi: {}", e.message), Err(e), None),
            };
            JobDone::OpDone {
                label,
                result,
                undo,
            }
        }
        Job::CreateFile { parent, name } => {
            let (label, result, undo) = match files_fs::create_file(&parent, &name) {
                Ok(p) => (
                    format!("Đã tạo tệp {}", file_label(&p)),
                    Ok(()),
                    Some(crate::undo::UndoAction::Created {
                        path: p,
                        is_dir: false,
                    }),
                ),
                Err(e) => (format!("Tạo tệp lỗi: {}", e.message), Err(e), None),
            };
            JobDone::OpDone {
                label,
                result,
                undo,
            }
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
            JobDone::OpDone {
                label,
                result,
                undo,
            }
        }
        Job::Zip { sources, dest } => {
            let (label, result) = match files_fs::create_zip(&sources, &dest) {
                Ok(count) => (
                    format!("Đã nén {count} tệp vào {}", file_label(&dest)),
                    Ok(()),
                ),
                Err(e) => (format!("Nén lỗi: {}", e.message), Err(e)),
            };
            JobDone::OpDone {
                label,
                result,
                undo: None,
            }
        }
        Job::Unzip { archive, target } => {
            let (label, result) = match files_fs::extract_zip(&archive, &target) {
                Ok(count) => (
                    format!("Đã giải nén {count} tệp vào {}", file_label(&target)),
                    Ok(()),
                ),
                Err(e) => (format!("Giải nén lỗi: {}", e.message), Err(e)),
            };
            JobDone::OpDone {
                label,
                result,
                undo: None,
            }
        }
        Job::Hash { path, kind } => {
            let (label, result) = match files_fs::hash_file(&path, kind) {
                Ok(digest) => (format!("{}: {digest}", kind.label()), Ok(())),
                Err(e) => (format!("Tính mã băm lỗi: {}", e.message), Err(e)),
            };
            JobDone::OpDone {
                label,
                result,
                undo: None,
            }
        }
        Job::GitStatus { dir, token } => {
            // `git status` trên một repo lớn có thể mất vài trăm ms; chạy ở
            // luồng nền nên UI không bị treo.
            let status = files_fs::status_for(&dir);
            JobDone::GitStatus { dir, status, token }
        }
        Job::Rotate {
            paths,
            counter_clockwise,
        } => {
            progress.begin(paths.len());
            let mut done = 0usize;
            let mut first_err = None;
            for p in &paths {
                if progress.cancelled() {
                    break;
                }
                let result = if counter_clockwise {
                    files_fs::rotate_image_counter_clockwise(p)
                } else {
                    files_fs::rotate_image_clockwise(p)
                };
                match result {
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
            JobDone::OpDone {
                label,
                result: first_err.map_or(Ok(()), Err),
                undo: None,
            }
        }
        Job::Search {
            root,
            query,
            max_depth,
            limit,
            token,
        } => {
            let opts = files_fs::SearchOptions {
                query,
                max_depth,
                limit,
            };
            // Không biết trước số mục sẽ quét, nhưng UI vẫn phải biết là có
            // việc đang chạy để hiện nút Huỷ — người dùng huỷ được giữa chừng.
            progress.begin_indeterminate();
            let (hits, cancelled) = files_fs::search(&root, &opts, || progress.cancelled());
            progress.finish();
            JobDone::SearchDone {
                token,
                root,
                hits,
                cancelled,
            }
        }
        Job::FolderSize { path } => {
            // Explicit "tính kích thước" from the menu; token 0 never matches a
            // live listing, so the reply is applied by the manual path only.
            let bytes = files_fs::directory_size(&path);
            JobDone::FolderSize {
                path,
                bytes,
                token: 0,
            }
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
        Job::Undo { action } => JobDone::UndoDone {
            result: crate::undo::undo(&action),
            action,
            redo: false,
        },
        Job::Redo { action } => JobDone::UndoDone {
            result: crate::undo::redo(&action),
            action,
            redo: true,
        },
    }
}

/// Apply `op` to each source, counting successes and keeping the first error.
/// Áp `op` lên từng nguồn, báo tiến trình và dừng khi bị huỷ.
///
/// Trả về cả đường dẫn đích: hoàn tác cần biết chính xác cái gì đã được tạo.
/// Mục đã xong trước khi huỷ vẫn nằm trong danh sách trả về — người dùng phải
/// hoàn tác được đúng những gì đã thực sự xảy ra.
/// Apply `op` to each source, counting successes and keeping the first error.
/// Áp `op` lên từng nguồn, báo tiến trình và dừng khi bị huỷ.
///
/// Trả về cặp (nguồn, đích) của những mục đã xong: hoàn tác/làm lại cần biết
/// chính xác cái gì đã xảy ra, kể cả khi batch có mục lỗi bị bỏ qua.
fn run_batch<F>(
    sources: &[PathBuf],
    progress: &Progress,
    mut op: F,
) -> (Vec<(PathBuf, PathBuf)>, Option<FsError>, bool)
where
    F: FnMut(&PathBuf) -> Result<PathBuf, FsError>,
{
    progress.begin(sources.len());
    let mut done = Vec::new();
    let mut first_err = None;
    let mut cancelled = false;
    for src in sources {
        // Dừng ở ranh giới tệp: không bao giờ bỏ dở một tệp đang chép.
        if progress.cancelled() {
            cancelled = true;
            break;
        }
        match op(src) {
            Ok(dest) => done.push((src.clone(), dest)),
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
        progress.tick();
    }
    progress.finish();
    (done, first_err, cancelled)
}

fn file_label(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}
