//! Error surface shared by the storage layer and the shell.
//!
//! Errors are deliberately flat: the UI shows `message`, while `code` lets the
//! frontend branch without parsing prose.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsError {
    /// Stable machine-readable tag, e.g. `not_found`, `access_denied`.
    pub code: String,
    /// Human-readable detail, safe to display.
    pub message: String,
}

impl FsError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn not_found(path: &str) -> Self {
        Self::new("not_found", format!("Path does not exist: {path}"))
    }

    pub fn not_a_directory(path: &str) -> Self {
        Self::new("not_a_directory", format!("Not a directory: {path}"))
    }

    pub fn access_denied(path: &str) -> Self {
        Self::new("access_denied", format!("Access denied: {path}"))
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for FsError {}

impl From<std::io::Error> for FsError {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        let code = match err.kind() {
            ErrorKind::NotFound => "not_found",
            ErrorKind::PermissionDenied => "access_denied",
            ErrorKind::InvalidInput => "invalid_path",
            _ => "io_error",
        };
        Self::new(code, err.to_string())
    }
}
