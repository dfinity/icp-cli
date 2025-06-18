use crate::{store_artifact::ArtifactStore, store_id::IdStore};
use ic_agent::Identity;
use icp_dirs::IcpCliDirs;
use icp_identity::key::LoadIdentityInContextError;
use std::sync::Arc;

pub struct Env {
    dirs: IcpCliDirs,
    identity: Option<String>,
    pub id_store: IdStore,
    pub artifact_store: ArtifactStore,
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
}
