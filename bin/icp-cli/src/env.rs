use ic_agent::Identity;
use icp_dirs::IcpCliDirs;
use icp_identity::key::LoadIdentityInContextError;
use std::sync::Arc;

use crate::options::Format;

#[derive(Default)]
pub struct EnvBuilder {
    dirs: IcpCliDirs,
    identity: Option<String>,
    output_format: Format,
}

impl EnvBuilder {
   
    pub fn identity(mut self, identity: Option<String>) -> Self {
        self.identity = identity;
        self
    }

    pub fn output_format(mut self, output_format: Format) -> Self {
        self.output_format = output_format;
        self
    }

    pub fn build(self) -> Env {
        Env {
            dirs: self.dirs,
            identity: self.identity,
            output_format: self.output_format,
        }
    }

}

pub struct Env {
    dirs: IcpCliDirs,
    identity: Option<String>,
    pub output_format: Format,
}

impl Env {

    pub fn builder() -> EnvBuilder {
        EnvBuilder::default()
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
