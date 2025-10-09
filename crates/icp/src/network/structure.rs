use crate::prelude::*;

pub struct NetworkDirectoryStructure {
    pub network_root: PathBuf,
    pub port_descriptor_dir: PathBuf,
}

impl NetworkDirectoryStructure {
    pub fn new(network_root: &Path, port_descriptor_dir: &Path) -> Self {
        Self {
            network_root: network_root.to_path_buf(),
            port_descriptor_dir: port_descriptor_dir.to_path_buf(),
        }
    }
}

impl NetworkDirectoryStructure {
    pub fn network_lock_path(&self) -> PathBuf {
        self.network_root.join("lock")
    }

    pub fn network_descriptor_path(&self) -> PathBuf {
        self.network_root.join("descriptor.json")
    }

    pub fn port_lock_path(&self, port: u16) -> PathBuf {
        self.port_descriptor_dir.join(format!("{port}.lock"))
    }

    pub fn port_descriptor_path(&self, port: u16) -> PathBuf {
        self.port_descriptor_dir.join(format!("{port}.json"))
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
