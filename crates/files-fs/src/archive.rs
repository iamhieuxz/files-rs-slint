//! Archive and hashing operations.
//!
//! Files exposes both as shell commands: "Create zip archive" / "Extract here"
//! and a hash list in Properties. The Shell does the archive work through
//! `IShellItem`; here the `zip` crate does it directly, which keeps the code
//! portable and testable. Hashing is plain `sha2`, because the OS has no stable
//! API for it that does not go through PowerShell.

use files_core::FsError;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

/// Read the file in 64 KB blocks so a large file does not have to fit in RAM.
const HASH_BUF: usize = 64 * 1024;

/// Which digest the Properties pane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashKind {
    Md5,
    Sha1,
    Sha256,
}

impl HashKind {
    pub fn label(self) -> &'static str {
        match self {
            HashKind::Md5 => "MD5",
            HashKind::Sha1 => "SHA-1",
            HashKind::Sha256 => "SHA-256",
        }
    }
}

/// Hash a file. Directories are rejected: hashing a tree is a different feature
/// (and a much slower one) than what the properties pane promises.
pub fn hash_file(path: &Path, kind: HashKind) -> Result<String, FsError> {
    let meta = std::fs::metadata(path).map_err(FsError::from)?;
    if meta.is_dir() {
        return Err(FsError::new("not_a_file", "Chỉ tính được mã băm cho tệp"));
    }

    let file = File::open(path).map_err(FsError::from)?;
    let mut reader = BufReader::with_capacity(HASH_BUF, file);
    let mut buf = vec![0u8; HASH_BUF];

    match kind {
        HashKind::Md5 => {
            // `md5` 0.7 exposes a context API, not the `Digest` trait.
            let mut ctx = md5::Context::new();
            loop {
                let n = reader.read(&mut buf).map_err(FsError::from)?;
                if n == 0 {
                    break;
                }
                ctx.consume(&buf[..n]);
            }
            Ok(hex(&ctx.compute().0))
        }
        HashKind::Sha1 => {
            use sha1::Digest;
            let mut h = sha1::Sha1::new();
            loop {
                let n = reader.read(&mut buf).map_err(FsError::from)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(hex(&h.finalize()))
        }
        HashKind::Sha256 => {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            loop {
                let n = reader.read(&mut buf).map_err(FsError::from)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(hex(&h.finalize()))
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Archive failures are a distinct class: the file exists and is readable, but
/// its contents are not a valid archive.
fn zip_err(err: zip::result::ZipError) -> FsError {
    match err {
        zip::result::ZipError::Io(e) => FsError::from(e),
        other => FsError::new("invalid_archive", other.to_string()),
    }
}

/// Name for the archive produced from `sources`, without the extension.
///
/// One item keeps its own name (`photos` → `photos.zip`); several items are
/// named after the folder they live in, which is what Explorer does.
pub fn archive_name(sources: &[PathBuf]) -> String {
    if sources.len() == 1 {
        return sources[0]
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "archive".into());
    }
    sources
        .first()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "archive".into())
}

/// Compress `sources` into `dest` (a `.zip` file).
///
/// Directory trees are stored with their relative paths so extraction recreates
/// the structure instead of dumping every file into one folder.
pub fn create_zip(sources: &[PathBuf], dest: &Path) -> Result<usize, FsError> {
    // `File::create` truncates, so overwriting here destroys data before any
    // error can be reported. This also catches zipping a file onto itself:
    // the destination is then one of the sources and already exists.
    if dest.exists() {
        return Err(FsError::new(
            "exists",
            format!("Đã tồn tại: {}", dest.display()),
        ));
    }
    let file = File::create(dest).map_err(FsError::from)?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut count = 0usize;
    for src in sources {
        let base = src
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("item"));
        count += add_tree(&mut zip, src, &base, options)?;
    }
    zip.finish().map_err(zip_err)?;
    Ok(count)
}

fn add_tree(
    zip: &mut zip::ZipWriter<File>,
    path: &Path,
    rel: &Path,
    options: zip::write::SimpleFileOptions,
) -> Result<usize, FsError> {
    let meta = std::fs::metadata(path).map_err(FsError::from)?;
    let mut count = 0usize;
    if meta.is_dir() {
        // An explicit entry keeps empty folders from vanishing on extract.
        zip.add_directory(zip_name(rel), options).map_err(zip_err)?;
        for child in std::fs::read_dir(path).map_err(FsError::from)? {
            let child = child.map_err(FsError::from)?;
            let child_rel = rel.join(child.file_name());
            count += add_tree(zip, &child.path(), &child_rel, options)?;
        }
    } else {
        zip.start_file(zip_name(rel), options).map_err(zip_err)?;
        let mut f = File::open(path).map_err(FsError::from)?;
        let mut buf = vec![0u8; HASH_BUF];
        loop {
            let n = f.read(&mut buf).map_err(FsError::from)?;
            if n == 0 {
                break;
            }
            zip.write_all(&buf[..n]).map_err(FsError::from)?;
        }
        count += 1;
    }
    Ok(count)
}

/// Zip paths always use forward slashes, on every platform.
fn zip_name(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Extract `archive` into `target_dir`.
///
/// Returns the number of entries written. Paths are validated so a crafted
/// archive cannot write outside the target folder.
pub fn extract_zip(archive: &Path, target_dir: &Path) -> Result<usize, FsError> {
    let file = File::open(archive).map_err(FsError::from)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| FsError::new("invalid_archive", format!("Không đọc được tệp nén: {e}")))?;

    std::fs::create_dir_all(target_dir).map_err(FsError::from)?;
    let mut count = 0usize;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| FsError::new("invalid_archive", format!("Mục {i} hỏng: {e}")))?;

        // `enclosed_name` chặn `..`; kiểm tra thêm dấu tuyệt đối vì một số
        // archive ghi `/file` hoặc `C:ile` và thư viện có thể chuẩn hoá nó
        // thành đường dẫn tương đối.
        let raw_name = entry.name();
        if raw_name.starts_with('/')
            || raw_name.starts_with('\\')
            || raw_name.as_bytes().get(1) == Some(&b':')
        {
            continue;
        }
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let out = target_dir.join(rel);

        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(FsError::from)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(FsError::from)?;
        }
        let mut f = File::create(&out).map_err(FsError::from)?;
        std::io::copy(&mut entry, &mut f).map_err(FsError::from)?;
        count += 1;
    }
    Ok(count)
}
