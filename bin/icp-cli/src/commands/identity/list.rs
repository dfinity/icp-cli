use std::fmt::{self, Display, Formatter};

use clap::Parser;
use serde::Serialize;
use snafu::Snafu;

use crate::env::Env;

#[derive(Debug, Parser)]
pub struct ListCmd;

pub fn exec(env: &Env, _cmd: ListCmd) -> Result<ListKeysMessage, ListKeysError> {
    let list = icp_identity::load_identity_list(env.dirs())?;
    let defaults = icp_identity::load_identity_defaults(env.dirs())?;
    Ok(ListKeysMessage {
        identities: list.to_valid_identity_names(),
        default: defaults.default,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ListKeysMessage {
    identities: Vec<String>,
    default: String,
}

impl Display for ListKeysMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for id in &self.identities {
            if *id == self.default {
                writeln!(f, "* {id}")?;
            } else {
                writeln!(f, "  {id}")?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum ListKeysError {
    #[snafu(transparent)]
    LoadIdentity {
        source: icp_identity::LoadIdentityError,
    },
}
