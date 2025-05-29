use crate::structure::NetworkDirectoryStructure;
use fd_lock::RwLock;
use icp_fs::fs::{CreateDirAllError, create_dir_all};
use snafu::prelude::*;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

pub struct NetworkDirectory {
    structure: NetworkDirectoryStructure,
}

impl NetworkDirectory {
    pub fn new(network_root: &Path) -> Self {
        let structure = NetworkDirectoryStructure::new(network_root);
        Self { structure }
    }

    pub fn structure(&self) -> &NetworkDirectoryStructure {
        &self.structure
    }

    pub fn ensure_exists(&self) -> Result<(), CreateDirAllError> {
        create_dir_all(self.structure.network_root())
    }

    pub fn open_lock_file(&self) -> Result<RwLock<File>, OpenLockFileError> {
        let path = self.structure.lock_path();
        let rwlock = RwLock::new(
            OpenOptions::new()
                .create(true)
                .write(true)
                .read(true)
                .truncate(true)
                .open(&path)
                .context(OpenLockFileSnafu { path })?,
        );
        Ok(rwlock)
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to open lock file at {}", path.display(),))]
pub struct OpenLockFileError {
    source: std::io::Error,
    path: PathBuf,
}
