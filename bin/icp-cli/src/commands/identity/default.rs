use crate::env::Env;
use clap::Parser;
use parse_display::Display;
use serde::Serialize;
use snafu::{Snafu, ensure};

#[derive(Debug, Parser)]
pub struct DefaultCmd {
    name: String,
}

pub fn exec(env: &Env, cmd: DefaultCmd) -> Result<DefaultIdentityMessage, DefaultIdentityError> {
    let list = icp_identity::load_identity_list(env.dirs())?;
    ensure!(
        list.identities.contains_key(&cmd.name),
        NoSuchIdentitySnafu { name: &cmd.name }
    );
    let mut defaults = icp_identity::load_identity_defaults(env.dirs())?;
    defaults.default = cmd.name.clone();
    icp_identity::write_identity_defaults(env.dirs(), &defaults)?;
    Ok(DefaultIdentityMessage {
        default: cmd.name.clone(),
    })
}

#[derive(Debug, Snafu)]
pub enum DefaultIdentityError {
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
#[serde(rename_all = "kebab-case")]
#[display("Set default identity to {default}")]
pub struct DefaultIdentityMessage {
    default: String,
}
