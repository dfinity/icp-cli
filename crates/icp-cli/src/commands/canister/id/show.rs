use std::io::stdout;

use clap::Args;
use icp::context::{CanisterSelection, Context, EnvironmentSelection};
use serde::Serialize;

use crate::options::EnvironmentOpt;

/// Show the canister ID for a canister in an environment.
#[derive(Debug, Args)]
pub(crate) struct ShowArgs {
    /// Name of the canister as defined in icp.yaml.
    canister: String,

    #[command(flatten)]
    environment: EnvironmentOpt,

    /// Output as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
struct JsonOutput {
    canister: String,
    canister_id: String,
    environment: String,
}

pub(crate) async fn exec(ctx: &Context, args: &ShowArgs) -> Result<(), anyhow::Error> {
    let environment: EnvironmentSelection = args.environment.clone().into();
    let selection = CanisterSelection::Named(args.canister.clone());

    let canister_id = ctx
        .get_canister_id_for_env(&selection, &environment)
        .await?;

    if args.json {
        serde_json::to_writer(
            stdout(),
            &JsonOutput {
                canister: args.canister.clone(),
                canister_id: canister_id.to_string(),
                environment: environment.name().to_string(),
            },
        )?;
    } else {
        println!("{canister_id}");
    }

    Ok(())
}
