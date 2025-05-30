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

    #[allow(dead_code)]
    pub fn network_config_path(&self, name: &str) -> Utf8PathBuf {
        self.root.join("networks").join(format!("{name}.yaml"))
    }

    fn work_dir(&self) -> Utf8PathBuf {
        self.root.join(".icp")
    }

    pub fn network_root(&self, network_name: &str) -> Utf8PathBuf {
        self.work_dir().join("networks").join(network_name)
    }
}
