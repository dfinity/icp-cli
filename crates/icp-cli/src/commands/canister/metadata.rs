use anyhow::bail;
use clap::Args;
use icp::context::Context;

use crate::{commands::args, operations::misc::fetch_canister_metadata};

/// Read a metadata section from a canister
#[derive(Debug, Args)]
pub(crate) struct MetadataArgs {
    #[command(flatten)]
    pub(crate) common: args::CanisterCommandArgs,

    /// The name of the metadata section to read
    pub(crate) metadata_name: String,
}

pub(crate) async fn exec(ctx: &Context, args: &MetadataArgs) -> Result<(), anyhow::Error> {
    let selections = args.common.selections();

    // Get the canister principal
    let canister_id = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;

    // Get the agent
    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;

    // Fetch the metadata
    let metadata = fetch_canister_metadata(&agent, canister_id, &args.metadata_name).await;

    match metadata {
        Some(value) => {
            ctx.term.write_line(&value)?;
            Ok(())
        }
        None => bail!(
            "Metadata section '{}' not found in canister {}",
            args.metadata_name,
            canister_id
        ),
    }
}
