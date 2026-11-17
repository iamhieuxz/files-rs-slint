//! Directory entry model.
//!
//! Mirrors `Files.App.Data.Items.ListedItem` / `IListedItem` in the original:
//! one immutable snapshot of a filesystem object, produced by the storage layer
//! and rendered verbatim by the UI.

use serde::{Deserialize, Serialize};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

impl EntryKind {
    pub fn is_dir(self) -> bool {
        matches!(self, EntryKind::Directory)
    }
}

/// A single row in a listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// File name only, never a full path.
    pub name: String,
    /// Absolute path, as a display string.
    pub path: String,
    pub kind: EntryKind,
    /// Size in bytes; `None` for directories and for entries whose metadata
    /// could not be read (permission denied, broken symlink).
    pub size: Option<u64>,
    /// Last-write time, milliseconds since the Unix epoch.
    pub modified_ms: Option<i64>,
    pub created_ms: Option<i64>,
    /// Lowercase extension without the dot. `None` for directories.
    pub extension: Option<String>,
    /// True when the entry starts with `.` (hidden on Unix) or carries the
    /// Windows hidden attribute. The storage layer resolves the platform rule;
    /// the UI only filters on this flag.
    pub is_hidden: bool,
    /// Read-only attribute, surfaced in the info pane.
    pub is_readonly: bool,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind.is_dir()
    }

    /// Extension for comparison purposes, lowercased, dot-free.
    pub fn ext_lower(&self) -> Option<String> {
        self.extension.as_ref().map(|e| e.to_ascii_lowercase())
    }
}

/// Convert a system time into epoch milliseconds, saturating instead of failing.
///
/// Public because the storage crate is a separate compilation unit: it reads
/// `SystemTime` from the OS and must produce the same integer encoding.
pub fn system_time_ms(t: std::time::SystemTime) -> Option<i64> {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).ok(),
        // Timestamps before 1970 are representable as negative milliseconds.
        Err(e) => {
            let d = e.duration();
            let ms = i64::try_from(d.as_millis()).ok()?;
            Some(-ms)
        }
    }
}
