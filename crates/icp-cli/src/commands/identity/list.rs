use std::io::stdout;

use candid::Principal;
use clap::Args;
use icp::identity::manifest::{IdentityDefaults, IdentityList};
use itertools::Itertools;
use serde::Serialize;

use icp::context::Context;

/// List the identities
#[derive(Debug, Args)]
pub(crate) struct ListArgs {
    /// Output command results as JSON
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) json: bool,

    /// Suppress human-readable output; print only identity names
    #[arg(long, short)]
    pub(crate) quiet: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &ListArgs) -> Result<(), anyhow::Error> {
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

    if args.json {
        serde_json::to_writer(
            stdout(),
            &JsonIdentityList {
                default_identity: defaults.default.clone(),
                identities: sorted_identities
                    .iter()
                    .map(|(name, id)| JsonIdentity {
                        name: name.to_string(),
                        principal: id.principal(),
                    })
                    .collect(),
            },
        )?;
        return Ok(());
    }

    if args.quiet {
        for (name, _) in &sorted_identities {
            println!("{name}");
        }
        return Ok(());
    }

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

#[derive(Serialize)]
struct JsonIdentityList {
    default_identity: String,
    identities: Vec<JsonIdentity>,
}

#[derive(Serialize)]
struct JsonIdentity {
    name: String,
    principal: Principal,
}
