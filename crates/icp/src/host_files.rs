//! Host filesystem implementation of [`icp_deploy_canister::FileAccess`].
//!
//! Backs project loading/consolidation on the real filesystem. Stateless:
//! operates on the (absolute) paths the model passes in.

use async_trait::async_trait;
use icp_deploy_canister::files::{FileAccess, FileAccessError};

use crate::prelude::*;

#[derive(Debug, Default, Clone, Copy)]
pub struct HostFileAccess;

#[async_trait]
impl FileAccess for HostFileAccess {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, FileAccessError> {
        crate::fs::read(path).map_err(|e| FileAccessError::Read {
            path: path.to_owned(),
            message: e.to_string(),
        })
    }

    async fn read_to_string(&self, path: &Path) -> Result<String, FileAccessError> {
        crate::fs::read_to_string(path).map_err(|e| FileAccessError::Read {
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
        let canon = dunce::canonicalize(path.as_std_path()).ok()?;
        PathBuf::try_from(canon).ok()
    }
}
