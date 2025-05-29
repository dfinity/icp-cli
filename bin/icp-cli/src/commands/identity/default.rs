use crate::env::Env;
use clap::Parser;
use parse_display::Display;
use serde::Serialize;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct DefaultCmd {
    name: String,
}

pub fn exec(env: &Env, cmd: DefaultCmd) -> Result<DefaultIdentityMessage, DefaultIdentityError> {
    let list = icp_identity::manifest::load_identity_list(env.dirs())?;
    icp_identity::manifest::change_default_identity(env.dirs(), &list, &cmd.name)?;
    Ok(DefaultIdentityMessage {
        default: cmd.name.clone(),
    })
}

#[derive(Debug, Snafu)]
pub enum DefaultIdentityError {
    #[snafu(transparent)]
    ChangeDefault {
        source: icp_identity::manifest::ChangeDefaultsError,
    },

    #[snafu(transparent)]
    LoadList {
        source: icp_identity::LoadIdentityError,
    },
}

#[derive(Display, Serialize)]
#[serde(rename_all = "kebab-case")]
#[display("Set default identity to {default}")]
pub struct DefaultIdentityMessage {
    default: String,
}
