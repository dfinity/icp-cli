use std::path::PathBuf;

pub struct ProjectStructure {
    root: PathBuf,
}

impl ProjectStructure {
    pub fn find() -> Option<Self> {
        let current_dir = std::env::current_dir().ok()?;
        let mut path = current_dir.clone();
        loop {
            if path.join("icp-project.yaml").exists() {
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

    pub fn network_config_path(&self, name: &str) -> PathBuf {
        self.root.join("networks").join(format!("{name}.yaml"))
    }

    pub fn network_root(&self, name: &str) -> PathBuf {
        self.root.join(".networks").join(name)
    }
}
