use std::fmt::{self, Display, Formatter};

use clap::Parser;
use itertools::Itertools;
use serde::Serialize;
use snafu::Snafu;

use crate::env::Env;

#[derive(Parser)]
pub struct ListCmd;

pub fn exec(env: &Env, _cmd: ListCmd) -> Result<ListKeysMessage, ListKeysError> {
    let list = icp_identity::load_identity_list(env.dirs())?;
    let mut identities = icp_identity::special_identities();
    identities.extend(list.identities.into_keys());
    Ok(ListKeysMessage { identities })
}

#[derive(Serialize)]
pub struct ListKeysMessage {
    identities: Vec<String>,
}

impl Display for ListKeysMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.identities.iter().format("\n"))
    }
}

#[derive(Debug, Snafu)]
pub enum ListKeysError {
    #[snafu(transparent)]
    LoadIdentity {
        source: icp_identity::LoadIdentityError,
    },
}
