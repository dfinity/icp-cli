use std::path::PathBuf;

use directories::ProjectDirs;

#[derive(Debug, Clone)]
pub enum IcpCliDirs {
    Standard(ProjectDirs),
    Overridden(PathBuf),
}

impl IcpCliDirs {
    pub fn new() -> Self {
        if let Some(override_var) = std::env::var_os("ICP_CLI_HOME") {
            Self::Overridden(override_var.into())
        } else {
            Self::Standard(ProjectDirs::from("org.dfinity", "DFINITY Stiftung", "icp-cli").unwrap())
        }
    }

    pub fn identity_dir(&self) -> PathBuf {
        match self {
            Self::Standard(dirs) => dirs.data_dir().join("identity"),
            Self::Overridden(path) => path.join("identity"),
        }
    }
}

impl Default for IcpCliDirs {
    fn default() -> Self {
        Self::new()
    }
}
