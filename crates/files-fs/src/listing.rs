//! Directory listing.
//!
//! The original's equivalent is `FilesystemOperations.GetDirectoryContentAsync`
//! plus `ItemManipulationModel`, which streams entries into an observable
//! collection as enumeration proceeds. Here the directory is read to completion
//! and returned as one sorted snapshot, which is what the frontend renders.

use files_core::entry::{system_time_ms, Entry, EntryKind};
use files_core::sort::{filter, ListOptions, SortSpec};
use files_core::{FsError, Listing, Location};
use std::path::Path;

/// Read one directory and return a rendered-ready snapshot.
///
/// Per-entry metadata failures do not abort the listing: a directory whose
/// children are partly unreadable still shows what it can, and the failures are
/// reported in `partial_errors`. Only a failure to read the directory itself is
/// an error.
pub fn read_dir(
    path: &Path,
    sort: SortSpec,
    opts: ListOptions,
) -> Result<Listing, FsError> {
    let meta = std::fs::metadata(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => FsError::not_found(&path.to_string_lossy()),
        std::io::ErrorKind::PermissionDenied => FsError::access_denied(&path.to_string_lossy()),
        _ => FsError::from(e),
    })?;

    if !meta.is_dir() {
        return Err(FsError::not_a_directory(&path.to_string_lossy()));
    }

    let mut entries = Vec::new();
    let mut partial_errors = Vec::new();

    let iter = match std::fs::read_dir(path) {
        Ok(it) => it,
        Err(e) => {
            let err = FsError::from(e);
            return Err(match err.code.as_str() {
                "not_found" => FsError::not_found(&path.to_string_lossy()),
                "access_denied" => FsError::access_denied(&path.to_string_lossy()),
                _ => err,
            });
        }
    };

    for item in iter {
        let item = match item {
            Ok(i) => i,
            Err(e) => {
                partial_errors.push(e.to_string());
                continue;
            }
        };

        // `DirEntry::file_type` avoids a full stat when the platform caches the
        // type in the directory record, which is the common case on NTFS.
        let file_type = match item.file_type() {
            Ok(t) => t,
            Err(e) => {
                partial_errors.push(format!("{}: {e}", item.path().display()));
                // Still list the name so the user sees the object exists.
                entries.push(Entry {
                    name: item.file_name().to_string_lossy().into_owned(),
                    path: item.path().to_string_lossy().into_owned(),
                    kind: EntryKind::Other,
                    size: None,
                    modified_ms: None,
                    created_ms: None,
                    extension: None,
                    is_hidden: false,
                    is_readonly: false,
                });
                continue;
            }
        };

        let md = item.metadata();
        let (size, modified_ms, created_ms) = match &md {
            Ok(m) => (
                if file_type.is_dir() { None } else { Some(m.len()) },
                m.modified().ok().and_then(system_time_ms),
                m.created().ok().and_then(system_time_ms),
            ),
            Err(e) => {
                partial_errors.push(format!("{}: {e}", item.path().display()));
                (None, None, None)
            }
        };

        let name = item.file_name().to_string_lossy().into_owned();
        let kind = if file_type.is_symlink() {
            EntryKind::Symlink
        } else if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };

        let readonly = md.as_ref().map(|m| m.permissions().readonly()).unwrap_or(false);

        let extension = if kind.is_dir() {
            None
        } else {
            Path::new(&name)
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
        };

        entries.push(Entry {
            path: item.path().to_string_lossy().into_owned(),
            // Windows hides on the FILE_ATTRIBUTE_HIDDEN/SYSTEM bits, not on a
            // leading dot. Dotfiles are still treated as hidden so the rule
            // holds on the portable test path as well.
            is_hidden: name.starts_with('.') || crate::ops::is_hidden_on_disk(&item.path()),
            is_readonly: readonly,
            extension,
            size,
            modified_ms,
            created_ms,
            kind,
            name,
        });
    }

    let total = entries.len();
    let mut entries = filter(entries, &opts);
    let hidden_count = total - entries.len();
    sort.sort(&mut entries);

    Ok(Listing {
        location: Location::from_path(path),
        entries,
        hidden_count,
        sort,
        partial_errors,
    })
}
