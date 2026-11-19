//! File logging.
//!
//! The app ships without a console window, so `stderr` goes nowhere. Panics and
//! failed operations would be invisible exactly when the user needs them, so
//! everything diagnostic goes to an append-only text file.
//!
//! The location is chosen by trying several candidates in order, because a
//! launched process may legitimately be unable to write to its own profile:
//! a restricted token, a redirected profile, or a parent harness that confines
//! writes to a project directory. A logger that silently writes nowhere is worse
//! than no logger, so the first writable candidate wins and the choice is
//! remembered.

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Candidates in preference order: proper user data first, then beside the exe.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        out.push(PathBuf::from(base).join("FilesRS").join("log.txt"));
    }
    if let Some(exe) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        out.push(exe.join("files-rs-log.txt"));
    }
    out
}

/// The log file actually in use, decided once per process.
pub fn log_path() -> PathBuf {
    static CHOSEN: OnceLock<PathBuf> = OnceLock::new();
    CHOSEN
        .get_or_init(|| {
            for candidate in candidates() {
                if can_append(&candidate) {
                    return candidate;
                }
            }
            // Nothing is writable; keep the preferred name so the message is
            // still meaningful if the situation changes later.
            candidates()
                .into_iter()
                .next()
                .unwrap_or_else(|| PathBuf::from("files-rs-log.txt"))
        })
        .clone()
}

/// True when `path` can be opened for append, creating it and its folder.
fn can_append(path: &std::path::Path) -> bool {
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return false;
        }
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .is_ok()
}

/// Append one line to `path`, creating parent folders as needed.
///
/// Kept separate from [`write`] so it can be tested against a path this process
/// is definitely allowed to touch.
pub fn write_to(path: &std::path::Path, msg: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Keep the file bounded: a runaway loop must not fill the disk.
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > 1_000_000 {
            let _ = std::fs::remove_file(path);
        }
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    writeln!(f, "[{stamp}] {msg}")
}

/// Append one line to the active log file. Never panics.
///
/// A logger that crashes the app is worse than no logger, so every failure is
/// dropped.
pub fn write(msg: &str) {
    let _ = write_to(&log_path(), msg);
}

/// Install a panic hook that records the panic before the default handler runs.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown".to_string());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        write(&format!("PANIC at {location}: {payload}"));
        default(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn lines_are_appended_in_order() {
        let dir = scratch("log-append");
        let path = dir.join("log.txt");
        write_to(&path, "dòng một").expect("ghi được");
        write_to(&path, "dòng hai").expect("ghi được");
        let text = std::fs::read_to_string(&path).expect("đọc lại được");
        let first = text.find("dòng một").expect("có dòng một");
        let second = text.find("dòng hai").expect("có dòng hai");
        assert!(first < second, "phải giữ đúng thứ tự");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_folders_are_created() {
        // `%LOCALAPPDATA%\FilesRS` may not exist on a fresh profile.
        let dir = scratch("log-mkdir");
        let path = dir.join("a").join("b").join("log.txt");
        write_to(&path, "tạo thư mục").expect("phải tạo được thư mục cha");
        assert!(path.exists(), "tệp log phải tồn tại");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_candidate_is_a_plausible_log_file() {
        let list = candidates();
        assert!(!list.is_empty(), "phải có ít nhất một chỗ ghi");
        for p in &list {
            assert_eq!(p.extension().and_then(|e| e.to_str()), Some("txt"));
        }
        // The preferred one lives in the user's own profile when that is available.
        if std::env::var_os("LOCALAPPDATA").is_some() {
            let first = &list[0];
            assert!(
                first.to_string_lossy().contains("FilesRS"),
                "ưu tiên thư mục riêng của app: {first:?}"
            );
        }
    }

    #[test]
    fn the_chosen_path_is_stable_within_a_process() {
        // Re-deciding on every call would let a transient failure move the log
        // mid-session and split the evidence.
        assert_eq!(log_path(), log_path());
    }
}
