//! Filesystem access for the shell.
//!
//! This crate is the only place that talks to the OS. It produces the pure
//! values defined in `files-core`, so the rest of the app stays testable.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod archive;
pub mod assoc;
pub mod drives;
pub mod listing;
pub mod log;
pub mod ops;
pub mod search;
pub mod shellicon;
pub mod watch;

pub use assoc::{open_settings_page, register as register_file_assoc, unregister as unregister_file_assoc};
pub use archive::{archive_name, create_zip, extract_zip, hash_file, HashKind};
pub use drives::{list_drives, DriveInfo};
pub use log::set_log_sink;
pub use listing::read_dir;
pub use search::{search, SearchHit, SearchOptions};
pub use shellicon::{shell_icon, thumbnail, wants_shell_icon, RgbaIcon};
pub use ops::{
    copy_into, copy_into_with, copy_text_to_clipboard, create_file, create_folder, ConflictPolicy,
    delete_permanently,
    delete_to_recycle_bin, directory_size, is_hidden_on_disk, move_into, move_into_with,
    open_terminal, rename, rotate_image_clockwise, shortcut_target,
    suggested_copy_name,
};
pub use watch::{watch_directory, WatchHandle};
