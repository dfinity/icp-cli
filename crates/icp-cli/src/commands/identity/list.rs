use crate::commands::Context;
use clap::Parser;
use icp_identity::manifest::{load_identity_defaults, load_identity_list};
use itertools::Itertools;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct ListCmd;

pub fn exec(ctx: &Context, _cmd: ListCmd) -> Result<(), ListKeysError> {
    let dirs = ctx.dirs();
    let list = load_identity_list(dirs)?;
    let defaults = load_identity_defaults(dirs)?;
    // sorted alphabetically by name
    let sorted_identities = list
        .identities
        .iter()
        .sorted_by_key(|(name, _)| name.len())
        .rev()
        .collect::<Vec<_>>();
    let longest_identity_name_length = sorted_identities
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    for (name, id) in sorted_identities.iter() {
        let principal = id.principal();
        let padded_name = format!("{: <1$}", name, longest_identity_name_length);
        if **name == defaults.default {
            println!("* {padded_name} {principal}");
        } else {
            println!("  {padded_name} {principal}");
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
