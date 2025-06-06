use ic_agent::Identity;
use icp_dirs::IcpCliDirs;
use icp_identity::key::LoadIdentityInContextError;
use std::sync::Arc;

pub struct Env {
    dirs: IcpCliDirs,
    identity: Option<String>,
}

impl Env {
    pub fn new(dirs: IcpCliDirs, identity: Option<String>) -> Self {
        Self { dirs, identity }
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
