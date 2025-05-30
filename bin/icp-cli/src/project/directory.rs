use crate::project::structure::ProjectDirectoryStructure;
use icp_network::NetworkDirectory;

pub struct ProjectDirectory {
    structure: ProjectDirectoryStructure,
}

impl ProjectDirectory {
    pub fn find() -> Option<Self> {
        let current_dir = std::env::current_dir().ok()?;
        let mut path = current_dir.clone();
        loop {
            let structure = ProjectDirectoryStructure::new(&path);

            if structure.project_yaml_path().exists() {
                break Some(Self { structure });
            }
            if !path.pop() {
                break None;
            }
        }
    }

    pub fn structure(&self) -> &ProjectDirectoryStructure {
        &self.structure
    }

    pub fn network(&self, network_name: &str) -> NetworkDirectory {
        let network_root = self.structure.network_root(network_name);
        NetworkDirectory::new(&network_root)
    }
}
