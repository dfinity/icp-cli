use clap::Args;
use icp::identity::manifest::{
    LoadIdentityManifestError, load_identity_defaults, load_identity_list,
};
use itertools::Itertools;

use crate::commands::{Context, Mode};

#[derive(Debug, Args)]
pub(crate) struct ListArgs;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ListKeysError {
    #[error(transparent)]
    LoadIdentity(#[from] LoadIdentityManifestError),
}

pub(crate) async fn exec(ctx: &Context, _: &ListArgs) -> Result<(), ListKeysError> {
    match &ctx.mode {
        Mode::Global | Mode::Project(_) => {
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
                    println!("* {padded_name} {principal}");
                } else {
                    println!("  {padded_name} {principal}");
                }
            }
        }
    }

    Ok(())
}
