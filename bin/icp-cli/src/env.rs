use std::{fmt::Display, sync::Arc};

use ic_agent::Identity;
use icp_dirs::IcpCliDirs;
use icp_identity::LoadIdentityError;
use serde::Serialize;

use crate::OutputFormat;

pub struct Env {
    output_format: OutputFormat,
    dirs: IcpCliDirs,
    identity: Option<String>,
}

impl Env {
    pub fn new(output_format: OutputFormat, identity: Option<String>) -> Self {
        Self {
            output_format,
            dirs: IcpCliDirs::new(),
            identity,
        }
    }
    pub fn _output_format(&self) -> OutputFormat {
        self.output_format
    }
    pub fn print_result<E>(&self, res: Result<impl Display + Serialize, E>) -> Result<(), E> {
        match res {
            Err(e) => Err(e),
            Ok(o) => match self.output_format {
                OutputFormat::Human => {
                    println!("{o}");
                    Ok(())
                }
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string(&o).unwrap());
                    Ok(())
                }
            },
        }
    }
    pub fn load_identity(&self) -> Result<Arc<dyn Identity>, LoadIdentityError> {
        if let Some(identity) = &self.identity {
            icp_identity::load_identity(
                &self.dirs,
                &icp_identity::load_identity_list(&self.dirs)?,
                identity,
                || todo!(),
            )
        } else {
            icp_identity::load_identity_in_context(&self.dirs, || todo!())
        }
    }
    pub fn dirs(&self) -> &IcpCliDirs {
        &self.dirs
    }
}
