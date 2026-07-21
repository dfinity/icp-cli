//! Test-only helpers.

use async_trait::async_trait;

use crate::files::{FileAccess, FileAccessError};
use crate::prelude::*;

/// A [`FileAccess`] backed by the real host filesystem, for unit tests that
/// write manifests to a temp dir and consolidate them.
pub struct HostFiles;

#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
impl FileAccess for HostFiles {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, FileAccessError> {
        std::fs::read(path).map_err(|e| FileAccessError::Read {
            path: path.to_owned(),
            message: e.to_string(),
        })
    }

    async fn read_to_string(&self, path: &Path) -> Result<String, FileAccessError> {
        std::fs::read_to_string(path).map_err(|e| FileAccessError::Read {
            path: path.to_owned(),
            message: e.to_string(),
        })
    }

    async fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    async fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    async fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, FileAccessError> {
        let rd = std::fs::read_dir(path).map_err(|e| FileAccessError::ReadDir {
            path: path.to_owned(),
            message: e.to_string(),
        })?;
        let mut out = Vec::new();
        for entry in rd {
            let entry = entry.map_err(|e| FileAccessError::ReadDir {
                path: path.to_owned(),
                message: e.to_string(),
            })?;
            if let Ok(p) = PathBuf::from_path_buf(entry.path()) {
                out.push(p);
            }
        }
        Ok(out)
    }

    async fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        let c = std::fs::canonicalize(path).ok()?;
        PathBuf::from_path_buf(c).ok()
    }
}
