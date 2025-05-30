use crate::project::structure::ProjectDirectoryStructure;
use camino::Utf8PathBuf;
use icp_network::NetworkDirectory;
use snafu::{ResultExt, Snafu};
use std::io;

pub struct ProjectDirectory {
    structure: ProjectDirectoryStructure,
}

impl ProjectDirectory {
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

    pub fn network(&self, network_name: &str) -> NetworkDirectory {
        let network_root = self.structure.network_root(network_name);
        NetworkDirectory::new(&network_root)
    }
}

#[derive(Debug, Snafu)]
pub enum FindProjectError {
    #[snafu(display("project path is non-UTF-8"))]
    NonUtf8 { source: camino::FromPathBufError },
    #[snafu(display("failed to access current directory"))]
    AccessError { source: io::Error },
}
