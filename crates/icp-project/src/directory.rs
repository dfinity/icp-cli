use std::io;

use icp::{fs::yaml, prelude::*};
use icp_network::{NetworkConfig, NetworkDirectory};
use snafu::{ResultExt, Snafu};

use crate::{manifest::ProjectManifest, structure::ProjectDirectoryStructure};

pub struct ProjectDirectory {
    structure: ProjectDirectoryStructure,
}

impl ProjectDirectory {
    #[cfg(test)]
    pub fn new(root: &Path) -> Self {
        let structure = ProjectDirectoryStructure::new(root);
        Self { structure }
    }

    pub fn find() -> Result<Option<Self>, FindProjectError> {
        let current_dir = PathBuf::try_from(std::env::current_dir().context(AccessSnafu)?)
            .context(NonUtf8Snafu)?;
        let mut path = current_dir.clone();
        loop {
            let structure = ProjectDirectoryStructure::new(&path);

            if structure.project_yaml_path().exists() {
                break Ok(Some(Self { structure }));
            }
            if !path.pop() {
                break Ok(None);
            }
        }
    }

    pub fn structure(&self) -> &ProjectDirectoryStructure {
        &self.structure
    }

    pub fn network(
        &self,
        network_name: &str,
        port_descriptor_dir: impl AsRef<Path>,
    ) -> NetworkDirectory {
        let network_root = self.structure.network_root(network_name);

        NetworkDirectory::new(network_name, &network_root, port_descriptor_dir.as_ref())
    }

    pub fn load_project_manifest(&self) -> Result<ProjectManifest, yaml::Error> {
        yaml::load(&self.structure.project_yaml_path())
    }

    pub fn load_canister_manifest(
        &self,
        canister_path: &Path,
    ) -> Result<CanisterManifest, yaml::Error> {
        yaml::load(&self.structure().canister_yaml_path(canister_path))
    }

    pub fn load_network_config(&self, network_path: &Path) -> Result<NetworkConfig, yaml::Error> {
        yaml::load(&self.structure.network_config_path(network_path))
    }
}

#[derive(Debug, Snafu)]
pub enum FindProjectError {
    #[snafu(display("project path is non-UTF-8"))]
    NonUtf8 { source: FromPathBufError },

    #[snafu(display("failed to access current directory"))]
    AccessError { source: io::Error },
}
