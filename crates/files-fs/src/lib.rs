//! Filesystem access for the shell.
//!
//! This crate is the only place that talks to the OS. It produces the pure
//! values defined in `files-core`, so the rest of the app stays testable.

pub mod drives;
pub mod listing;
pub mod ops;
pub mod watch;

pub use drives::{list_drives, DriveInfo};
pub use listing::read_dir;
pub use ops::{
    copy_into, create_file, create_folder, delete_to_recycle_bin, is_hidden_on_disk, move_into,
    rename, suggested_copy_name,
};
pub use watch::{watch_directory, WatchHandle};
