use camino::{Utf8Path, Utf8PathBuf};

pub struct ProjectDirectoryStructure {
    root: Utf8PathBuf,
}

impl ProjectDirectoryStructure {
    pub fn new(root: &Utf8Path) -> Self {
        let root = root.to_path_buf();
        Self { root }
    }

    pub fn root(&self) -> &Utf8PathBuf {
        &self.root
    }

    pub fn project_yaml_path(&self) -> Utf8PathBuf {
        self.root.join("icp.yaml")
    }

    pub fn canister_yaml_path(&self, canister_dir: &Utf8Path) -> Utf8PathBuf {
        self.root.join(canister_dir).join("canister.yaml")
    }

    pub fn network_config_path(&self, network_path: &Utf8Path) -> Utf8PathBuf {
        self.root.join(format!("{network_path}.yaml"))
    }

    fn work_dir(&self) -> Utf8PathBuf {
        self.root.join(".icp")
    }

    pub fn network_root(&self, network_name: &str) -> Utf8PathBuf {
        self.work_dir().join("networks").join(network_name)
    }
}
