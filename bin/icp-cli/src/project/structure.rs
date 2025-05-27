use icp_network::structure::NetworkDirectoryStructure;
use std::path::PathBuf;

pub struct ProjectDirectoryStructure {
    root: PathBuf,
}

impl ProjectDirectoryStructure {
    pub fn find() -> Option<Self> {
        let current_dir = std::env::current_dir().ok()?;
        let mut path = current_dir.clone();
        loop {
            if path.join("icp.yaml").exists() {
                break Some(Self { root: path });
            }
            if !path.pop() {
                break None;
            }
        }
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    #[allow(dead_code)]
    pub fn network_config_path(&self, name: &str) -> PathBuf {
        self.root.join("networks").join(format!("{name}.yaml"))
    }

    pub fn network(&self, network_name: &str) -> NetworkDirectoryStructure {
        let network_root = self.root.join(".networks").join(network_name);

        NetworkDirectoryStructure::new(&network_root)
    }
}
