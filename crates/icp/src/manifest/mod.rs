use std::marker::PhantomData;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use snafu::prelude::*;

use crate::fs;
use crate::prelude::*;

pub(crate) mod adapter;
pub(crate) mod canister;
pub(crate) mod environment;
pub(crate) mod network;
pub(crate) mod project;
pub(crate) mod recipe;
pub(crate) mod serde_helpers;

pub use {
    adapter::assets,
    adapter::prebuilt,
    canister::{
        ArgsFormat, BuildStep, BuildSteps, CanisterManifest, Instructions, ManifestInitArgs,
        SyncStep, SyncSteps,
    },
    environment::EnvironmentManifest,
    network::NetworkManifest,
    project::ProjectManifest,
};

pub const PROJECT_MANIFEST: &str = "icp.yaml";
pub const CANISTER_MANIFEST: &str = "canister.yaml";

// A manifest item that can either be a path to another manifest file or the manifest itself.
//
// The valid path specifications are:
// - CanisterManifest: path or glob pattern to the directory containing "canister.yaml"
// - NetworkManifest: path to network manifest
// - EnvironmentManifest: path to environment manifest
#[derive(Clone, Debug, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum Item<T> {
    /// Path to a manifest
    Path(String),

    /// The manifest
    Manifest(T),
}

impl<T: Serialize> Serialize for Item<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Item::Path(p) => p.serialize(serializer),
            Item::Manifest(m) => m.serialize(serializer),
        }
    }
}

impl<'de, T> Deserialize<'de> for Item<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{MapAccess, Visitor, value::MapAccessDeserializer};
        use std::fmt;

        struct ItemVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ItemVisitor<T> {
            type Value = Item<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string path or a manifest object")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Item::Path(v.to_owned()))
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Item::Path(v))
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                T::deserialize(MapAccessDeserializer::new(map)).map(Item::Manifest)
            }
        }

        deserializer.deserialize_any(ItemVisitor(PhantomData))
    }
}

#[derive(Debug, Snafu)]
pub enum ProjectRootLocateError {
    #[snafu(display("project manifest not found in {path}"))]
    NotFound { path: PathBuf },
}

/// Trait for locating the project root directory containing the project manifest file (`icp.yaml`).
pub trait ProjectRootLocate: Sync + Send {
    /// Locate the project root directory.
    fn locate(&self) -> Result<PathBuf, ProjectRootLocateError>;
}

/// Implementation of [`ProjectRootLocate`].
pub struct ProjectRootLocateImpl {
    /// Current directory to begin search from in case dir is unspecified.
    cwd: PathBuf,

    /// Specific directory to be used as project root directly.
    dir: Option<PathBuf>,
}

impl ProjectRootLocateImpl {
    /// Creates a new instance of `ProjectRootLocateImpl`.
    ///
    /// - If `dir` is specified, it will be used as Project Root directly.
    /// - Otherwise, it will search upwards from `cwd` for the project manifest file (`icp.yaml`).
    pub fn new(cwd: PathBuf, dir: Option<PathBuf>) -> Self {
        Self { cwd, dir }
    }
}

impl ProjectRootLocate for ProjectRootLocateImpl {
    fn locate(&self) -> Result<PathBuf, ProjectRootLocateError> {
        // Specified path
        if let Some(dir) = &self.dir {
            if !dir.join(PROJECT_MANIFEST).exists() {
                return NotFoundSnafu {
                    path: dir.to_owned(),
                }
                .fail();
            }

            return Ok(dir.to_owned());
        }

        // Unspecified path
        let mut dir = self.cwd.to_owned();

        loop {
            if !dir.join(PROJECT_MANIFEST).exists() {
                if let Some(p) = dir.parent() {
                    dir = p.to_path_buf();
                    continue;
                }

                return NotFoundSnafu {
                    path: self.cwd.to_owned(),
                }
                .fail();
            }

            return Ok(dir);
        }
    }
}

#[derive(Debug, Snafu)]
pub enum LoadManifestFromPathError {
    #[snafu(display("failed to read manifest from path"))]
    Read { source: fs::IoError },

    #[snafu(display("failed to parse manifest at '{path}'"))]
    Parse {
        source: serde_yaml::Error,
        path: PathBuf,
    },
}

/// Loads a manifest of type `T` from the specified file path.
pub async fn load_manifest_from_path<T>(path: &Path) -> Result<T, LoadManifestFromPathError>
where
    T: for<'de> Deserialize<'de>,
{
    let content = fs::read(path).context(ReadSnafu)?;
    let m = serde_yaml::from_slice::<T>(&content).context(ParseSnafu {
        path: path.to_path_buf(),
    })?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino_tempfile::Utf8TempDir;

    fn write_manifest(dir: &Path) {
        std::fs::write(dir.join(PROJECT_MANIFEST), "").unwrap();
    }

    #[test]
    fn locate_returns_cwd_when_manifest_present() {
        let tmp = Utf8TempDir::new().unwrap();
        write_manifest(tmp.path());

        let locator = ProjectRootLocateImpl::new(tmp.path().to_path_buf(), None);
        assert_eq!(locator.locate().unwrap(), tmp.path());
    }

    #[test]
    fn locate_walks_up_to_manifest() {
        let tmp = Utf8TempDir::new().unwrap();
        write_manifest(tmp.path());

        let nested = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();

        let locator = ProjectRootLocateImpl::new(nested, None);
        assert_eq!(locator.locate().unwrap(), tmp.path());
    }

    #[test]
    fn locate_returns_not_found_when_no_manifest_anywhere() {
        let tmp = Utf8TempDir::new().unwrap();
        let nested = tmp.path().join("a/b");
        std::fs::create_dir_all(&nested).unwrap();

        // Host filesystem contains no icp.yaml above the tempdir (assumed in CI).
        let locator = ProjectRootLocateImpl::new(nested, None);
        assert!(matches!(
            locator.locate(),
            Err(ProjectRootLocateError::NotFound { .. })
        ));
    }

    // When cwd is a symlinked directory, locate() walks up via the symlink's
    // lexical parents
    #[cfg(unix)]
    #[test]
    fn locate_walks_up_through_symlink() {
        // target/ has no manifest anywhere above it within the test's scope.
        let target = Utf8TempDir::new().unwrap();

        // project/ contains the manifest; `project/link` is a symlink to target/.
        let project = Utf8TempDir::new().unwrap();
        write_manifest(project.path());
        let link = project.path().join("link");
        std::os::unix::fs::symlink(target.path().as_std_path(), link.as_std_path()).unwrap();

        // cwd is the symlink path; its lexical parent is `project`,
        // which contains the manifest.
        let locator = ProjectRootLocateImpl::new(link, None);
        assert_eq!(locator.locate().unwrap(), project.path());
    }
}
