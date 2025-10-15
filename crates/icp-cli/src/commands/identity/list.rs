use clap::Parser;
use icp::identity::manifest::{
    LoadIdentityManifestError, load_identity_defaults, load_identity_list,
};
use itertools::Itertools;
use snafu::Snafu;

use crate::commands::Context;

#[derive(Debug, Parser)]
pub struct ListCmd;

#[derive(Debug, Snafu)]
pub enum ListKeysError {
    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityManifestError },
}

pub fn exec(ctx: &Context, _cmd: ListCmd) -> Result<(), ListKeysError> {
    let dir = ctx.dirs.identity();

    let list = load_identity_list(&dir)?;
    let defaults = load_identity_defaults(&dir)?;

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
        let padded_name = format!("{name: <longest_identity_name_length$}");
        if **name == defaults.default {
            ctx.println(&format!("* {padded_name} {principal}"));
        } else {
            ctx.println(&format!("  {padded_name} {principal}"));
        }
    }

    Ok(())
}
