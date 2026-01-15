use anyhow::bail;
use clap::Args;
use icp::context::Context;
use serde::Serialize;

use crate::{commands::args, operations::misc::fetch_canister_metadata};

#[derive(Debug, Args)]
pub(crate) struct MetadataArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// The name of the metadata section to read
    pub(crate) metadata_name: String,

    /// Format output in json
    #[arg(long = "json")]
    pub json_format: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &MetadataArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();
    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;
    let cid = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;

    // Fetch the metadata
    let metadata = fetch_canister_metadata(&agent, cid, &args.metadata_name).await;

    #[derive(Serialize)]
    struct MetadataResult {
        name: String,
        value: String,
    }

    match metadata {
        Some(value) => {
            let output = match args.json_format {
                true => serde_json::to_string(&MetadataResult {
                    name: args.metadata_name.clone(),
                    value,
                })
                .expect("Serializing status result to json failed"),
                false => value,
            };
            ctx.term.write_line(&output)?;
            Ok(())
        }
        None => bail!(
            "Metadata section '{}' not found in canister {}",
            args.metadata_name,
            cid
        ),
    }
}
