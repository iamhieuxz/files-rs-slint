//! Path normalisation and breadcrumb construction.
//!
//! The original builds breadcrumbs in `NavigationHelpers` and renders them with
//! the WinUI `BreadcrumbBar`. The parsing rules live here instead, so a
//! breadcrumb can be asserted in a unit test rather than inspected by eye.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// One clickable segment of the address bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    /// Text shown in the bar, e.g. `Users` or `C:`.
    pub label: String,
    /// Path this crumb navigates to when clicked.
    pub path: String,
}

/// Normalise a user- or shell-supplied path into a canonical display form.
///
/// - Expands a leading `~` to the user profile.
/// - Resolves `.` and `..` lexically. Symlinks are *not* followed, because
///   following them would move the user somewhere they did not navigate to.
/// - Strips a trailing separator, except for a bare drive root (`C:\`).
pub fn normalize(input: &str) -> PathBuf {
    let trimmed = input.trim();
    let expanded: PathBuf = if trimmed == "~" || trimmed.starts_with("~/") || trimmed.starts_with("~\\") {
        let rest = trimmed.trim_start_matches('~').trim_start_matches(['/', '\\']);
        let mut p = home_dir();
        if !rest.is_empty() {
            p.push(rest);
        }
        p
    } else {
        PathBuf::from(trimmed)
    };

    let mut out = PathBuf::new();
    for comp in expanded.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                // Never pop past a root.
                if !out.pop() {
                    out.push(comp.as_os_str());
                }
            }
            other => out.push(other.as_os_str()),
        }
    }

    if out.as_os_str().is_empty() {
        out.push(std::path::MAIN_SEPARATOR.to_string());
    }
    out
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\"))
}

/// Split an absolute path into clickable crumbs.
///
/// `C:\Users\me\Documents` yields `C:` → `Users` → `me` → `Documents`.
/// A UNC path `\\server\share\dir` yields `\\server\share` → `dir`, because the
/// server and share are a single navigable unit, not two.
pub fn breadcrumbs(path: &Path) -> Vec<Crumb> {
    let mut crumbs = Vec::new();
    let mut acc = PathBuf::new();
    let mut unc_root_done = false;

    for comp in path.components() {
        match comp {
            Component::Prefix(prefix) => {
                // `Prefix` carries `C:` for drive paths and the server/share for
                // UNC. Keep it verbatim so `C:` renders as `C:`.
                acc.push(prefix.as_os_str());
                if matches!(prefix.kind(), std::path::Prefix::UNC(..) | std::path::Prefix::VerbatimUNC(..)) {
                    unc_root_done = true;
                }
                crumbs.push(Crumb {
                    label: prefix.as_os_str().to_string_lossy().into_owned(),
                    path: format_root(&acc),
                });
            }
            Component::RootDir => {
                if acc.as_os_str().is_empty() {
                    acc.push(std::path::MAIN_SEPARATOR.to_string());
                } else if unc_root_done {
                    // UNC: `\\server\share\` is the same crumb as the prefix.
                } else {
                    acc.push(std::path::MAIN_SEPARATOR.to_string());
                }
            }
            Component::Normal(part) => {
                acc.push(part);
                crumbs.push(Crumb {
                    label: part.to_string_lossy().into_owned(),
                    path: acc.to_string_lossy().into_owned(),
                });
            }
            Component::CurDir | Component::ParentDir => {}
        }
    }

    crumbs
}

fn format_root(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    if s.len() == 2 && s.ends_with(':') {
        // `C:` alone is not navigable; `C:\` is.
        format!("{s}\\")
    } else {
        s
    }
}

/// Parent of `path`, or `None` at a filesystem root.
pub fn parent(path: &Path) -> Option<PathBuf> {
    path.parent().map(|p| p.to_path_buf())
}
