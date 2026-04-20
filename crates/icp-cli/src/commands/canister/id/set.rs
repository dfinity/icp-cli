use anyhow::bail;
use candid::Principal;
use clap::Args;
use icp::context::{CanisterSelection, Context, EnvironmentSelection};

use crate::options::EnvironmentOpt;

/// Set the canister ID for a canister in an environment.
///
/// Use this to register a pre-existing canister ID without creating a new
/// canister on-chain. Fails if the canister already has an ID unless
/// `--force` is given.
#[derive(Debug, Args)]
pub(crate) struct SetArgs {
    /// Name of the canister as defined in icp.yaml.
    canister: String,

    /// The canister principal to set.
    canister_id: Principal,

    #[command(flatten)]
    environment: EnvironmentOpt,

    /// Overwrite the canister ID if one is already set.
    #[arg(long, short)]
    force: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &SetArgs) -> Result<(), anyhow::Error> {
    let environment: EnvironmentSelection = args.environment.clone().into();
    let selection = CanisterSelection::Named(args.canister.clone());

    if let Ok(existing) = ctx.get_canister_id_for_env(&selection, &environment).await {
        if !args.force {
            bail!(
                "canister '{}' already has ID {} in environment '{}'. Use --force to overwrite",
                args.canister,
                existing,
                environment.name()
            );
        }
        ctx.remove_canister_id_for_env(&args.canister, &environment)
            .await?;
    }

    ctx.set_canister_id_for_env(&args.canister, args.canister_id, &environment)
        .await?;

    println!(
        "Set canister ID for {} to {} in environment {}",
        args.canister,
        args.canister_id,
        environment.name()
    );

    Ok(())
}
