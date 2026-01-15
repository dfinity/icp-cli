use anyhow::bail;
use clap::Args;
use icp::context::{CanisterSelection, Context, EnvironmentSelection, NetworkSelection};
use icp::identity::IdentitySelection;

use crate::{commands::args, operations::misc::fetch_canister_metadata, options};

#[derive(Debug, Args)]
pub(crate) struct MetadataArgs {
    /// The canister name or principal to target.
    /// When using a name, an environment must be specified.
    pub(crate) canister: args::Canister,

    /// The name of the metadata section to read
    pub(crate) metadata_name: String,

    #[command(flatten)]
    pub(crate) network: options::NetworkOpt,

    #[command(flatten)]
    pub(crate) environment: options::EnvironmentOpt,

    #[command(flatten)]
    pub(crate) identity: options::IdentityOpt,
}

pub(crate) async fn exec(ctx: &Context, args: &MetadataArgs) -> Result<(), anyhow::Error> {
    let canister_selection: CanisterSelection = args.canister.clone().into();
    let environment: EnvironmentSelection = args.environment.clone().into();
    let network: NetworkSelection = args.network.clone().into();
    let identity: IdentitySelection = args.identity.clone().into();

    // Get the canister principal
    let canister_id = ctx
        .get_canister_id(&canister_selection, &network, &environment)
        .await?;

    // Get the agent
    let agent = ctx.get_agent(&identity, &network, &environment).await?;

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
