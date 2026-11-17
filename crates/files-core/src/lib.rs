//! A Windows file manager's data model, free of UI and OS dependencies.
//!
//! Nothing in this crate touches the filesystem. The storage layer
//! (`files-fs`) produces these values and the shell renders them, which keeps
//! the rules testable without a running app.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod entry;
pub mod error;
pub mod path;
pub mod sort;

pub use entry::{Entry, EntryKind};
pub use error::FsError;
pub use path::{breadcrumbs, Crumb};
pub use sort::{ListOptions, SortBy, SortDirection, SortSpec};

/// Where the user currently is, and how to render that location.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// Canonical absolute path.
    pub path: String,
    /// Clickable segments for the address bar.
    pub crumbs: Vec<Crumb>,
    /// Parent directory, unless this is a root.
    pub parent: Option<String>,
    /// Name shown in a tab, e.g. `Documents`.
    pub display_name: String,
}

impl Location {
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Self {
        let p = path.as_ref();
        let crumbs = breadcrumbs(p);
        let display_name = crumbs
            .last()
            .map(|c| c.label.clone())
            .unwrap_or_else(|| p.to_string_lossy().into_owned());
        Self {
            path: p.to_string_lossy().into_owned(),
            parent: path::parent(p).map(|x| x.to_string_lossy().into_owned()),
            crumbs,
            display_name,
        }
    }
}

/// One complete directory view handed to the UI in a single message.
///
/// The original keeps this split across `ItemManipulationModel`, the layout
/// page, and `CurrentInstanceViewModel`. Collapsing it into one snapshot means
/// the frontend never renders a half-updated list: either the whole view
/// changes or none of it does.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub location: Location,
    pub entries: Vec<Entry>,
    /// Directories excluded by the filter, so the status bar can say `3 hidden`.
    pub hidden_count: usize,
    pub sort: SortSpec,
    /// Set when the directory was readable but some entries were not;
    /// the view still renders, and the UI can offer a retry.
    pub partial_errors: Vec<String>,
}
