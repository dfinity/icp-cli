use std::path::{Path, PathBuf};

pub struct NetworkDirectoryStructure {
    pub network_root: PathBuf,
}

impl NetworkDirectoryStructure {}

impl NetworkDirectoryStructure {
    pub fn new(network_root: &Path) -> Self {
        let network_root = network_root.to_path_buf();
        Self { network_root }
    }

    pub fn network_root(&self) -> &PathBuf {
        &self.network_root
    }

    pub fn project_descriptor_path(&self) -> PathBuf {
        self.network_root.join("descriptor.json")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.network_root.join("lock")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.network_root.join("state")
    }

    pub fn pocketic_dir(&self) -> PathBuf {
        self.network_root.join("pocket-ic")
    }

    // pocketic expects this file not to exist when launching it.
    // pocketic populates it with the port number, and deletes the file when it exits.
    // if the file exists, pocketic assumes this means another pocketic instance
    // is running, and exits with exit code(0).
    pub fn pocketic_port_file(&self) -> PathBuf {
        self.pocketic_dir().join("port")
    }
}
