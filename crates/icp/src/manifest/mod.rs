use crate::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;
use snafu::prelude::*;

pub(crate) mod adapter;
pub(crate) mod canister;
pub(crate) mod environment;
pub(crate) mod network;
pub mod project;
pub(crate) mod recipe;
pub(crate) mod serde_helpers;

pub use {canister::CanisterManifest, environment::EnvironmentManifest, network::NetworkManifest};

pub const PROJECT_MANIFEST: &str = "icp.yaml";
pub const CANISTER_MANIFEST: &str = "canister.yaml";

#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Item<T> {
    /// Path to a manifest
    Path(String),

    /// The manifest
    Manifest(T),
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
                }.fail();
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
                }.fail();
            }

            return Ok(dir);
        }
    }
}
