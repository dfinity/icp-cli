use std::path::{Path, PathBuf};

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

    #[allow(dead_code)]
    pub fn network_config_path(&self, name: &str) -> PathBuf {
        self.root.join("networks").join(format!("{name}.yaml"))
    }

    fn work_dir(&self) -> PathBuf {
        self.root.join(".icp")
    }

    pub fn network_root(&self, network_name: &str) -> PathBuf {
        self.work_dir().join("networks").join(network_name)
    }
}
