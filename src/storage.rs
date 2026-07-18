//! Shared location for persistent bot state and user feedback.

use std::path::PathBuf;

/// Returns the application's writable data directory.
///
/// `CODASAURUS_DATA_DIR` is intended for self-hosted/container deployments;
/// otherwise the platform data directory is used.
pub fn data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("CODASAURUS_DATA_DIR").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }

    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("codasaurus")
}
