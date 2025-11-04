use clap::Args;
use icp::{
    fs::lock::LockError,
    identity::manifest::{IdentityDefaults, IdentityList, LoadIdentityManifestError},
};
use itertools::Itertools;

use icp::context::Context;

#[derive(Debug, Args)]
pub(crate) struct ListArgs;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ListKeysError {
    #[error(transparent)]
    LoadIdentity(#[from] LoadIdentityManifestError),
    #[error(transparent)]
    LoadLock(#[from] LockError),
}

pub(crate) async fn exec(ctx: &Context, _: &ListArgs) -> Result<(), ListKeysError> {
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
