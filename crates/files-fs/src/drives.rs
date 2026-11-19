//! Drive and volume enumeration.
//!
//! The original uses `WindowsDriveManager` with WMI plus `IShellItem` for
//! friendly labels, and `DrivesSizeProvider` for capacity. This module reads the
//! same facts through `sysinfo`, which avoids a WMI dependency and keeps the
//! crate buildable on any host.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    /// Mount point as shown to the user, e.g. `C:\`.
    pub path: String,
    /// Single-letter label for a local drive, or the mount name.
    pub label: String,
    /// Volume name when the filesystem supplies one.
    pub name: Option<String>,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub removable: bool,
    pub kind: DriveKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Other,
}

impl DriveInfo {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }

    /// Fraction used, clamped to `0.0..=1.0`. A zero-capacity volume reports 0.
    pub fn used_ratio(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.used_bytes() as f64 / self.total_bytes as f64).clamp(0.0, 1.0)
    }
}

/// Every mounted volume the user can browse, fixed disks first.
pub fn list_drives() -> Vec<DriveInfo> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut out: Vec<DriveInfo> = disks
        .list()
        .iter()
        .map(|d| {
            let mount = d.mount_point().to_string_lossy().into_owned();
            let kind = if d.is_removable() {
                DriveKind::Removable
            } else {
                match d.kind() {
                    sysinfo::DiskKind::SSD | sysinfo::DiskKind::HDD => DriveKind::Fixed,
                    _ => DriveKind::Other,
                }
            };
            DriveInfo {
                label: mount.trim_end_matches(['\\', '/']).to_string(),
                name: Some(d.name().to_string_lossy().into_owned()).filter(|s| !s.is_empty()),
                path: mount,
                total_bytes: d.total_space(),
                free_bytes: d.available_space(),
                removable: d.is_removable(),
                kind,
            }
        })
        .collect();

    // Fixed volumes first, then removable, then the rest; alphabetical within
    // each group so the sidebar order is stable between launches.
    out.sort_by(|a, b| {
        let rank = |k: DriveKind| match k {
            DriveKind::Fixed => 0u8,
            DriveKind::Removable => 1,
            DriveKind::Network => 2,
            DriveKind::Other => 3,
        };
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
    });
    out
}

/// Common user folders, resolved from the environment.
///
/// The original gets these from `KnownFolders` (a Shell API). Environment
/// variables are the portable equivalent and agree with the shell on a standard
/// Windows profile; a redirected or virtualised profile is the known gap.
pub fn quick_access() -> Vec<(String, String)> {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let candidates = [
        ("Home", String::new()),
        ("Desktop", "Desktop".to_string()),
        ("Documents", "Documents".to_string()),
        ("Downloads", "Downloads".to_string()),
        ("Pictures", "Pictures".to_string()),
        ("Music", "Music".to_string()),
        ("Videos", "Videos".to_string()),
    ];

    candidates
        .into_iter()
        .filter_map(|(label, sub)| {
            if home.is_empty() {
                return None;
            }
            let p = if sub.is_empty() {
                std::path::PathBuf::from(&home)
            } else {
                Path::new(&home).join(&sub)
            };
            p.is_dir()
                .then(|| (label.to_string(), p.to_string_lossy().into_owned()))
        })
        .collect()
}
