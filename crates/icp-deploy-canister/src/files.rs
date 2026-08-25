//! Abstracted filesystem access.
//!
//! Project loading, manifest consolidation, and reading built wasm artifacts all
//! go through [`FileAccess`] rather than touching the real filesystem, so the
//! same logic can run inside a canister (backed by, e.g., stable-memory blobs).
//! Paths are `camino` UTF-8 paths.

use async_trait::async_trait;
use snafu::Snafu;

use crate::prelude::*;

#[derive(Debug, Snafu)]
pub enum FileAccessError {
    #[snafu(display("failed to read file at '{path}': {message}"))]
    Read { path: PathBuf, message: String },

    #[snafu(display("failed to list directory at '{path}': {message}"))]
    ReadDir { path: PathBuf, message: String },
}

/// Read-oriented filesystem access, rooted at the project directory.
///
/// Predicate methods (`exists`/`is_file`/`is_dir`) return `false` on any error,
/// matching the `std::path` inherent methods they replace. `canonicalize`
/// returns `None` when the path cannot be resolved or is not valid UTF-8.
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
pub trait FileAccess: Send + Sync {
    /// Read the raw bytes of a file.
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, FileAccessError>;

    /// Read a file as a UTF-8 string.
    async fn read_to_string(&self, path: &Path) -> Result<String, FileAccessError>;

    async fn exists(&self, path: &Path) -> bool;

    async fn is_file(&self, path: &Path) -> bool;

    async fn is_dir(&self, path: &Path) -> bool;

    /// Non-recursive directory listing. Entries are returned as absolute paths
    /// (the directory joined with each entry name).
    async fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, FileAccessError>;

    /// Canonicalize a path (resolve `..` and symlinks). Returns `None` if the
    /// path does not exist or does not resolve to valid UTF-8; callers treat
    /// that as "cannot establish identity", which is safe for de-duplication.
    async fn canonicalize(&self, path: &Path) -> Option<PathBuf>;
}
