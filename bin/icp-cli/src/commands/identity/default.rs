use crate::env::Env;
use clap::Parser;
use parse_display::Display;
use serde::Serialize;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct DefaultCmd {
    name: Option<String>,
}

pub fn exec(env: &Env, cmd: DefaultCmd) -> Result<DefaultCmdMessage, DefaultIdentityError> {
    if let Some(name) = cmd.name {
        let list = icp_identity::manifest::load_identity_list(env.dirs())?;
        icp_identity::manifest::change_default_identity(env.dirs(), &list, &name)?;
        Ok(DefaultCmdMessage::Change(ChangeDefaultIdentityMessage {
            default: name,
        }))
    } else {
        let defaults = icp_identity::manifest::load_identity_defaults(env.dirs())?;
        Ok(DefaultCmdMessage::Display(DisplayDefaultIdentityMessage {
            default: defaults.default,
        }))
    }
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
pub struct ChangeDefaultIdentityMessage {
    default: String,
}

#[derive(Display, Serialize)]
#[serde(rename_all = "kebab-case")]
#[display("{default}")]
pub struct DisplayDefaultIdentityMessage {
    default: String,
}

#[derive(Display, Serialize)]
#[serde(untagged)]
pub enum DefaultCmdMessage {
    #[display("{0}")]
    Change(ChangeDefaultIdentityMessage),

    #[display("{0}")]
    Display(DisplayDefaultIdentityMessage),
}
