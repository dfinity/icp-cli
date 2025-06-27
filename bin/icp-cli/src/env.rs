use crate::env::GetProjectError::{ProjectNotFound};
use crate::{store_artifact::ArtifactStore, store_id::IdStore};
use ic_agent::Identity;
use icp_dirs::IcpCliDirs;
use icp_identity::key::LoadIdentityInContextError;
use icp_project::directory::{FindProjectError, ProjectDirectory};
use icp_project::model::{LoadProjectManifestError, ProjectManifest};
use snafu::Snafu;
use std::sync::{Arc, OnceLock};

pub struct Env {
    dirs: IcpCliDirs,
    identity: Option<String>,
    pub id_store: IdStore,
    pub artifact_store: ArtifactStore,
    project: TryOnceLock<ProjectManifest>,
}

impl Env {
    pub fn new(
        dirs: IcpCliDirs,
        identity: Option<String>,
        id_store: IdStore,
        artifact_store: ArtifactStore,
    ) -> Self {
        Self {
            dirs,
            identity,
            id_store,
            artifact_store,
            project: TryOnceLock::new(),
        }
    }

    pub fn load_identity(&self) -> Result<Arc<dyn Identity>, LoadIdentityInContextError> {
        if let Some(identity) = &self.identity {
            Ok(icp_identity::key::load_identity(
                &self.dirs,
                &icp_identity::manifest::load_identity_list(&self.dirs)?,
                identity,
                || todo!(),
            )?)
        } else {
            icp_identity::key::load_identity_in_context(&self.dirs, || todo!())
        }
    }

    pub fn dirs(&self) -> &IcpCliDirs {
        &self.dirs
    }

    // Returns the project manifest if it exists in the current directory or its parents.
    // This memoizes the project if successful, but since the program
    // should propagate any errors immediately, the error
    // isn't memoized. This avoids having to clone the error type.
    pub fn project(&self) -> Result<&ProjectManifest, GetProjectError> {
        let pd = ProjectDirectory::find()?
            .ok_or(ProjectNotFound)?;

        let project = self.project
            .get_or_try_init(|| ProjectManifest::load(pd))?;

        Ok(project)
    }
}

#[derive(Debug, Snafu)]
pub enum GetProjectError {
    #[snafu(transparent)]
    FindProjectDirectory { source: FindProjectError },

    #[snafu(transparent)]
    LoadProjectManifest { source: LoadProjectManifestError },

    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectNotFound,
}

#[derive(Debug)]
pub struct TryOnceLock<T> {
    inner: OnceLock<T>,
}

// todo(ericswanson): when OnceLock::get_or_try_init is stabilized, use that instead
impl<T> TryOnceLock<T> {
    pub fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    pub fn get_or_try_init<E, F>(&self, f: F) -> Result<&T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        if let Some(val) = self.inner.get() {
            Ok(val)
        } else {
            let value = f()?;
            Ok(self.inner.get_or_init(|| value))
        }
    }
}
