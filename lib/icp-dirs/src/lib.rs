use camino::Utf8PathBuf;
use directories::ProjectDirs;
use snafu::{OptionExt, ResultExt, Snafu};

#[derive(Debug, Clone)]
pub struct IcpCliDirs {
    pub cache_dir: Utf8PathBuf,
    pub config_dir: Utf8PathBuf,
    pub data_dir: Utf8PathBuf,
}

impl IcpCliDirs {
    pub fn new() -> Result<Self, DiscoverDirsError> {
        if let Ok(override_var) = std::env::var("ICP_HOME") {
            let root = Utf8PathBuf::from(override_var.clone());
            Ok(Self::from_override(root))
        } else {
            let project_dirs =
                ProjectDirs::from("org.dfinity", "", "icp-cli").context(CannotFindHomeSnafu)?;
            Ok(Self::from_dirs(project_dirs).context(NonUtf8Snafu)?)
        }
    }

    pub fn identity_dir(&self) -> Utf8PathBuf {
        self.data_dir.join("identity")
    }

    fn from_dirs(dirs: ProjectDirs) -> Result<Self, camino::FromPathBufError> {
        Ok(Self {
            cache_dir: dirs.cache_dir().to_owned().try_into()?,
            config_dir: dirs.config_dir().to_owned().try_into()?,
            data_dir: dirs.data_dir().to_owned().try_into()?,
        })
    }

    fn from_override(root: Utf8PathBuf) -> Self {
        Self {
            cache_dir: root.join("cache"),
            config_dir: root.join("config"),
            data_dir: root.join("data"),
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
