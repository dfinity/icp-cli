use anyhow::bail;
use candid::Principal;
use clap::Args;
use icp::context::{Context, EnvironmentSelection};
use tracing::info;

use crate::options::EnvironmentOpt;

/// Link an existing canister to the project by recording its ID in the canister ID store.
///
/// This associates an already-deployed canister with a name declared in the target
/// environment, without creating a new canister. It is the inverse of the record that
/// `icp canister create` writes automatically.
#[derive(Debug, Args)]
pub(crate) struct LinkArgs {
    /// Name of the project canister to associate the ID with.
    /// Must be declared in the target environment.
    pub(crate) name: String,

    /// Principal of the existing canister to link.
    pub(crate) principal: Principal,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

    /// Overwrite an ID already recorded for this canister.
    #[arg(long)]
    pub(crate) force: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &LinkArgs) -> Result<(), anyhow::Error> {
    let environment: EnvironmentSelection = args.environment.clone().into();

    // A principal must map to at most one canister within an environment; linking an
    // ID already claimed by another canister would create an ambiguous mapping.
    let existing = ctx.ids_by_environment(&environment).await?;
    if let Some((owner, _)) = existing
        .iter()
        .find(|(name, id)| **id == args.principal && *name != &args.name)
    {
        bail!(
            "canister ID {} is already linked to '{owner}' in environment '{}'",
            args.principal,
            environment.name()
        );
    }

    // Replacing an existing entry requires clearing it first; the id store refuses
    // to register a name that is already mapped.
    if args.force {
        ctx.remove_canister_id_for_env(&args.name, &environment)
            .await?;
    }

    ctx.set_canister_id_for_env(&args.name, args.principal, &environment)
        .await?;

    info!(
        "Linked canister '{}' to ID {} in environment '{}'",
        args.name,
        args.principal,
        environment.name()
    );

    Ok(())
}
