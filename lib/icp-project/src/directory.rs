use crate::model::RawProjectManifest;
use crate::structure::ProjectDirectoryStructure;
use camino::{Utf8Path, Utf8PathBuf};
use icp_canister::model::CanisterManifest;
use icp_fs::yaml::{LoadYamlFileError, load_yaml_file};
use icp_network::NetworkDirectory;
use snafu::{ResultExt, Snafu};
use std::io;

pub struct ProjectDirectory {
    structure: ProjectDirectoryStructure,
}

impl ProjectDirectory {
    #[cfg(test)]
    pub fn new(root: &Utf8Path) -> Self {
        let structure = ProjectDirectoryStructure::new(root);
        Self { structure }
    }

    pub fn find() -> Result<Option<Self>, FindProjectError> {
        let current_dir = Utf8PathBuf::try_from(std::env::current_dir().context(AccessSnafu)?)
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
        port_descriptor_dir: impl AsRef<Utf8Path>,
    ) -> NetworkDirectory {
        let network_root = self.structure.network_root(network_name);

        NetworkDirectory::new(network_name, &network_root, port_descriptor_dir.as_ref())
    }

    pub fn load_project_manifest(&self) -> Result<RawProjectManifest, LoadYamlFileError> {
        let path = self.structure.project_yaml_path();
        load_yaml_file(path)
    }

    pub fn load_canister_manifest(
        &self,
        canister_path: &Utf8Path,
    ) -> Result<CanisterManifest, LoadYamlFileError> {
        let path = self.structure().canister_yaml_path(canister_path);
        load_yaml_file(path)
    }
}

#[derive(Debug, Snafu)]
pub enum FindProjectError {
    #[snafu(display("project path is non-UTF-8"))]
    NonUtf8 { source: camino::FromPathBufError },
    #[snafu(display("failed to access current directory"))]
    AccessError { source: io::Error },
}
