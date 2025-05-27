use clap::Parser;
use parse_display::Display;
use serde::Serialize;
use snafu::{Snafu, ensure};

use crate::env::Env;

#[derive(Parser)]
pub struct DefaultCmd {
    name: String,
}

pub fn exec(env: &Env, cmd: DefaultCmd) -> Result<UseIdentityMessage, UseIdentityError> {
    let list = icp_identity::load_identity_list(env.dirs())?;
    ensure!(
        list.identities.contains_key(&cmd.name),
        NoSuchIdentitySnafu { name: &cmd.name }
    );
    let mut defaults = icp_identity::load_identity_defaults(env.dirs())?;
    defaults.default = cmd.name.clone();
    icp_identity::write_identity_defaults(env.dirs(), &defaults)?;
    Ok(UseIdentityMessage {
        default: cmd.name.clone(),
    })
}

#[derive(Debug, Snafu)]
pub enum UseIdentityError {
    #[snafu(transparent)]
    WriteDefaults {
        source: icp_identity::WriteIdentityError,
    },
    #[snafu(transparent)]
    LoadList {
        source: icp_identity::LoadIdentityError,
    },
    #[snafu(display("no identity found with name `{name}`"))]
    NoSuchIdentity { name: String },
}

#[derive(Display, Serialize)]
#[display("Set default identity to {default}")]
pub struct UseIdentityMessage {
    default: String,
}
