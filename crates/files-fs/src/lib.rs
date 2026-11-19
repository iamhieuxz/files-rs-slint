//! Filesystem access for the shell.
//!
//! This crate is the only place that talks to the OS. It produces the pure
//! values defined in `files-core`, so the rest of the app stays testable.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod archive;
pub mod assoc;
pub mod drives;
pub mod gitstatus;
pub mod listing;
pub mod log;
pub mod openwith;
pub mod ops;
pub mod previewdata;
pub mod recycle;
pub mod search;
pub mod shellicon;
pub mod thumbcache;
pub mod watch;

pub use archive::{archive_name, create_zip, extract_zip, hash_file, HashKind};
pub use assoc::{
    open_settings_page, register as register_file_assoc, unregister as unregister_file_assoc,
};
pub use drives::{list_drives, DriveInfo};
pub use gitstatus::{repo_root, status_for, GitState, StatusMap};
pub use listing::read_dir;
pub use log::set_log_sink;
pub use openwith::{open_with_apps, open_with_dialog, OpenWithApp};
pub use ops::{
    copy_into, copy_into_with, copy_text_to_clipboard, create_file, create_folder,
    delete_permanently, delete_to_recycle_bin, directory_size, free_path, is_hidden_on_disk,
    move_into, move_into_with, open_terminal, rename, rotate_image_clockwise,
    rotate_image_counter_clockwise, shortcut_target, suggested_copy_name, validate_name,
    ConflictPolicy,
};
pub use previewdata::{preview_data, PreviewData, PreviewKind, FOLDER_COUNT_LIMIT, TEXT_LIMIT};
pub use recycle::{
    bin_path as recycle_bin_path, can_browse as can_browse_recycle_bin, can_restore,
    empty as empty_recycle_bin, list as list_recycle_bin, RecycleItem,
};
pub use search::{search, SearchHit, SearchOptions};
pub use shellicon::{shell_icon, thumbnail, wants_shell_icon, RgbaIcon};
pub use thumbcache::{thumbnail_cached, thumbnail_cached_in};
pub use watch::{watch_directory, WatchHandle};
