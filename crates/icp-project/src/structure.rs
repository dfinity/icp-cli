use camino::{Utf8Path as Path, Utf8PathBuf as PathBuf};

pub struct ProjectDirectoryStructure {
    root: PathBuf,
}

impl ProjectDirectoryStructure {
    pub fn new(root: &Path) -> Self {
        let root = root.to_path_buf();
        Self { root }
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    pub fn project_yaml_path(&self) -> PathBuf {
        self.root.join("icp.yaml")
    }

    pub fn canister_yaml_path(&self, canister_dir: &Path) -> PathBuf {
        self.root.join(canister_dir).join("canister.yaml")
    }

    pub fn network_config_path(&self, network_path: &Path) -> PathBuf {
        self.root.join(format!("{network_path}.yaml"))
    }

    fn work_dir(&self) -> PathBuf {
        self.root.join(".icp")
    }

    pub fn network_root(&self, network_name: &str) -> PathBuf {
        self.work_dir().join("networks").join(network_name)
    }
}
