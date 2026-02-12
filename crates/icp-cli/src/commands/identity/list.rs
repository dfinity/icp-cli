use clap::Args;
use icp::identity::manifest::{IdentityDefaults, IdentityList};
use itertools::Itertools;

use icp::context::Context;

/// List the identities
#[derive(Debug, Args)]
pub(crate) struct ListArgs;

pub(crate) async fn exec(ctx: &Context, _: &ListArgs) -> Result<(), anyhow::Error> {
    let dirs = ctx.dirs.identity()?.into_read().await?;

    let list = IdentityList::load_from(dirs.as_ref())?;
    let defaults = IdentityDefaults::load_from(dirs.as_ref())?;

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
            println!("* {padded_name} {principal}");
        } else {
            println!("  {padded_name} {principal}");
        }
    }

    Ok(())
}
