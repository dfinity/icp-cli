use crate::env::Env;
use clap::Parser;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct ListCmd;

pub fn exec(env: &Env, _cmd: ListCmd) -> Result<(), ListKeysError> {
    let list = icp_identity::manifest::load_identity_list(env.dirs())?;
    let defaults = icp_identity::manifest::load_identity_defaults(env.dirs())?;
    for id in list.identities.keys() {
        if *id == defaults.default {
            println!("* {id}");
        } else {
            println!("  {id}");
        }
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum ListKeysError {
    #[snafu(transparent)]
    LoadIdentity {
        source: icp_identity::LoadIdentityError,
    },
}
