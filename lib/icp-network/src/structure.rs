use icp_support::directories;
use std::path::PathBuf;

pub struct NetworkDirectoryStructure {
    pub network_root: PathBuf,
}

impl NetworkDirectoryStructure {
    pub fn new(network_root: PathBuf) -> Self {
        Self { network_root }
    }

    pub fn project_descriptor_path(&self) -> PathBuf {
        self.network_root.join("descriptor.json")
    }

    pub fn port_descriptor_path(port: u16) -> PathBuf {
        directories::cache_dir()
            .join("local-network-descriptors-by-port")
            .join(format!("{}.json", port))
    }

    pub fn state_dir(&self) -> PathBuf {
        self.network_root.join("state")
    }
}
