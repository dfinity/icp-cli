use crate::prelude::*;
use directories::ProjectDirs;

#[derive(Debug, Clone)]
pub struct DirectoriesInner {
    data: PathBuf,
    cache: PathBuf,
}

impl DirectoriesInner {
    pub fn from_dirs(dirs: ProjectDirs) -> Result<Self, FromPathBufError> {
        Ok(Self {
            data: dirs.data_dir().to_owned().try_into()?,
            cache: dirs.cache_dir().to_owned().try_into()?,
        })
    }
}

#[derive(Debug, Clone)]
pub enum Directories {
    Standard(DirectoriesInner),
    Overridden(PathBuf),
}

#[derive(Debug, thiserror::Error)]
pub enum DirectoriesError {
    #[error("home directory could not be located")]
    LocateHome,

    #[error("user directories are non-UTF-8")]
    Utf8(#[from] FromPathBufError),
}

impl Directories {
    pub fn new() -> Result<Self, DirectoriesError> {
        // Allow overriding home directory
        if let Ok(v) = std::env::var("ICP_HOME") {
            return Ok(Self::Overridden(v.into()));
        }

        let dirs = ProjectDirs::from(
            "org.dfinity", // qualifier
            "",            // organization
            "icp-cli",     // application
        )
        .ok_or(DirectoriesError::LocateHome)?;

        // Convert to utf8 paths
        let dirs = DirectoriesInner::from_dirs(dirs)?;

        Ok(Self::Standard(dirs))
    }
}

impl Directories {
    fn data(&self) -> PathBuf {
        match self {
            Self::Standard(dirs) => dirs.data.clone(),
            Self::Overridden(path) => path.clone(),
        }
    }

    fn cache(&self) -> PathBuf {
        match self {
            Self::Standard(dirs) => dirs.cache.clone(),
            Self::Overridden(path) => path.clone(),
        }
    }

    pub fn identity(&self) -> PathBuf {
        self.data().join("identity")
    }

    pub fn port_descriptor(&self) -> PathBuf {
        self.cache().join("port-descriptors")
    }
}
