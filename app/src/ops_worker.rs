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
    },
    Move {
        sources: Vec<PathBuf>,
        target: PathBuf,
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
    /// Delete without the Recycle Bin (Shift+Delete).
    DeletePermanent {
        paths: Vec<PathBuf>,
    },
    /// Read the Windows shell icon for one file.
    ShellIcon {
        path: PathBuf,
        size: u32,
        token: u64,
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
    },
    /// A folder's total size, so the UI can fill the size column in place.
    /// `token` identifies the directory listing that asked for it.
    FolderSize {
        path: PathBuf,
        bytes: u64,
        token: u64,
    },
    /// A decoded Windows shell icon, ready for Slint.
    ShellIcon {
        path: PathBuf,
        icon: files_fs::RgbaIcon,
        token: u64,
    },
}

pub struct Worker {
    tx: Sender<Job>,
    /// Folder sizes run on their own thread so a slow tree cannot delay a copy,
    /// a delete, or the next navigation. Each job carries the directory token it
    /// belongs to; the UI drops replies whose token is stale.
    size_tx: Sender<SizeJob>,
}

/// A folder-size request, separate from `Job` because it is cancellable by
/// staleness rather than by finishing.
pub struct SizeJob {
    pub path: PathBuf,
    pub token: u64,
}

impl Worker {
    /// Spawn the worker thread and return the handle plus the result channel.
    pub fn spawn() -> (Self, Receiver<JobDone>) {
        let (job_tx, job_rx) = channel::<Job>();
        let (done_tx, done_rx) = channel::<JobDone>();
        let main_done = done_tx.clone();

        thread::Builder::new()
            .name("files-worker".into())
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    let done = run(job);
                    // A closed channel means the UI is shutting down; stop.
                    if main_done.send(done).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn worker thread");

        // Second thread, same result channel: sizes and operations are
        // independent, and one slow folder must not stall the other.
        let (size_tx, size_rx) = channel::<SizeJob>();
        let size_done = done_tx.clone();
        thread::Builder::new()
            .name("files-size".into())
            .spawn(move || {
                while let Ok(job) = size_rx.recv() {
                    let bytes = files_fs::directory_size(&job.path);
                    let done = JobDone::FolderSize {
                        path: job.path,
                        bytes,
                        token: job.token,
                    };
                    if size_done.send(done).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn size thread");

        (Self { tx: job_tx, size_tx }, done_rx)
    }

    pub fn submit(&self, job: Job) {
        // A closed channel means the worker exited; the UI stays alive and the
        // user can retry, so dropping the job is the least-bad option.
        let _ = self.tx.send(job);
    }

    /// Queue a folder-size walk. Replies are tagged with `token`.
    pub fn submit_size(&self, path: PathBuf, token: u64) {
        let _ = self.size_tx.send(SizeJob { path, token });
    }
}

fn run(job: Job) -> JobDone {
    match job {
        Job::Load { path, sort, opts, token } => {
            let result = files_fs::read_dir(&path, sort, opts);
            JobDone::Loaded { token, path, result }
        }
        Job::Copy { sources, target } => {
            let (done, err) = run_batch(&sources, |src| files_fs::copy_into(src, &target));
            let label = match &err {
                Some(e) => format!("Sao chép lỗi: {}", e.message),
                None => format!("Đã sao chép {done}/{} mục", sources.len()),
            };
            JobDone::OpDone { label, result: err.map_or(Ok(()), Err) }
        }
        Job::Move { sources, target } => {
            let (done, err) = run_batch(&sources, |src| files_fs::move_into(src, &target));
            let label = match &err {
                Some(e) => format!("Di chuyển lỗi: {}", e.message),
                None => format!("Đã di chuyển {done}/{} mục", sources.len()),
            };
            JobDone::OpDone { label, result: err.map_or(Ok(()), Err) }
        }
        Job::Delete { paths } => {
            let n = paths.len();
            match files_fs::delete_to_recycle_bin(&paths) {
                Ok(count) => JobDone::OpDone {
                    label: format!("Đã chuyển {count}/{n} mục vào Thùng rác"),
                    result: Ok(()),
                },
                Err(e) => JobDone::OpDone {
                    label: format!("Xoá lỗi: {}", e.message),
                    result: Err(e),
                },
            }
        }
        Job::CreateFolder { parent, name } => {
            let (label, result) = match files_fs::create_folder(&parent, &name) {
                Ok(p) => (format!("Đã tạo thư mục {}", file_label(&p)), Ok(())),
                Err(e) => (format!("Tạo thư mục lỗi: {}", e.message), Err(e)),
            };
            JobDone::OpDone { label, result }
        }
        Job::CreateFile { parent, name } => {
            let (label, result) = match files_fs::create_file(&parent, &name) {
                Ok(p) => (format!("Đã tạo tệp {}", file_label(&p)), Ok(())),
                Err(e) => (format!("Tạo tệp lỗi: {}", e.message), Err(e)),
            };
            JobDone::OpDone { label, result }
        }
        Job::Rename { from, to } => {
            let (label, result) = match files_fs::rename(&from, &to) {
                Ok(p) => (format!("Đã đổi tên thành {}", file_label(&p)), Ok(())),
                Err(e) => (format!("Đổi tên lỗi: {}", e.message), Err(e)),
            };
            JobDone::OpDone { label, result }
        }
        Job::Zip { sources, dest } => {
            let n = sources.len();
            let (label, result) = match files_fs::create_zip(&sources, &dest) {
                Ok(count) => (
                    format!("Đã nén {count} tệp vào {}", file_label(&dest)),
                    Ok(()),
                ),
                Err(e) => (format!("Nén lỗi: {}", e.message), Err(e)),
            };
            let _ = n;
            JobDone::OpDone { label, result }
        }
        Job::Unzip { archive, target } => {
            let (label, result) = match files_fs::extract_zip(&archive, &target) {
                Ok(count) => (
                    format!("Đã giải nén {count} tệp vào {}", file_label(&target)),
                    Ok(()),
                ),
                Err(e) => (format!("Giải nén lỗi: {}", e.message), Err(e)),
            };
            JobDone::OpDone { label, result }
        }
        Job::Hash { path, kind } => {
            let (label, result) = match files_fs::hash_file(&path, kind) {
                Ok(digest) => (format!("{}: {digest}", kind.label()), Ok(())),
                Err(e) => (format!("Tính mã băm lỗi: {}", e.message), Err(e)),
            };
            JobDone::OpDone { label, result }
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
                Ok(count) => JobDone::OpDone {
                    label: format!("Đã xoá vĩnh viễn {count}/{n} mục"),
                    result: Ok(()),
                },
                Err(e) => JobDone::OpDone {
                    label: format!("Xoá vĩnh viễn lỗi: {}", e.message),
                    result: Err(e),
                },
            }
        }
        Job::ShellIcon { path, size, token } => {
            match files_fs::shell_icon(&path, size) {
                Ok(Some(icon)) => JobDone::ShellIcon { path, icon, token },
                // No icon is not an error: the row keeps its Fluent glyph.
                Ok(None) | Err(_) => JobDone::OpDone {
                    label: String::new(),
                    result: Ok(()),
                },
            }
        }
    }
}

/// Apply `op` to each source, counting successes and keeping the first error.
fn run_batch<F>(sources: &[PathBuf], mut op: F) -> (usize, Option<FsError>)
where
    F: FnMut(&PathBuf) -> Result<PathBuf, FsError>,
{
    let mut done = 0usize;
    let mut first_err = None;
    for src in sources {
        match op(src) {
            Ok(_) => done += 1,
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
    }
    (done, first_err)
}

fn file_label(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}
