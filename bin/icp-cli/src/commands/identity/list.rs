use crate::env::Env;
use clap::Parser;
use icp_identity::manifest::{load_identity_defaults, load_identity_list};
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct ListCmd;

pub fn exec(env: &Env, _cmd: ListCmd) -> Result<(), ListKeysError> {
    let dirs = env.dirs();
    let list = load_identity_list(dirs)?;
    let defaults = load_identity_defaults(dirs)?;
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
        source: icp_identity::manifest::LoadIdentityManifestError,
    },
}
