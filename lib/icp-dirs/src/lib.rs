use camino::Utf8PathBuf;
use directories::ProjectDirs;
use snafu::{OptionExt, ResultExt, Snafu};

#[derive(Debug, Clone)]
pub enum IcpCliDirs {
    Standard(Utf8ProjectDirs),
    Overridden(Utf8PathBuf),
}

impl IcpCliDirs {
    pub fn new() -> Result<Self, DiscoverDirsError> {
        if let Ok(override_var) = std::env::var("ICP_HOME") {
            Ok(Self::Overridden(override_var.into()))
        } else {
            Ok(Self::Standard(
                Utf8ProjectDirs::from_dirs(
                    ProjectDirs::from("org.dfinity", "", "icp-cli").context(CannotFindHomeSnafu)?,
                )
                .context(NonUtf8Snafu)?,
            ))
        }
    }

    pub fn identity_dir(&self) -> Utf8PathBuf {
        match self {
            Self::Standard(dirs) => dirs.data_dir.join("identity"),
            Self::Overridden(path) => path.join("identity"),
        }
    }
}

#[derive(Debug, Snafu)]
pub enum DiscoverDirsError {
    #[snafu(display("user directories are non-UTF-8"))]
    NonUtf8 { source: camino::FromPathBufError },
    #[snafu(display("home directory could not be located"))]
    CannotFindHome,
}

#[derive(Debug, Clone)]
pub struct Utf8ProjectDirs {
    pub data_dir: Utf8PathBuf,
    pub config_dir: Utf8PathBuf,
    pub cache_dir: Utf8PathBuf,
}

impl Utf8ProjectDirs {
    pub fn from_dirs(dirs: ProjectDirs) -> Result<Self, camino::FromPathBufError> {
        Ok(Self {
            data_dir: dirs.data_dir().to_owned().try_into()?,
            config_dir: dirs.config_dir().to_owned().try_into()?,
            cache_dir: dirs.cache_dir().to_owned().try_into()?,
        })
    }
}
