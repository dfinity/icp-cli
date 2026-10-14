use candid::Principal;
use clap::Args;
use ic_management_canister_types::UninstallCodeArgs;
use icp_app::context::Context;

use crate::commands::args;
use icp_project::operations::proxy_management;

/// Uninstall a canister's code, deleting its state
///
/// The canister keeps its id, cycles, and settings, and is left empty.
#[derive(Debug, Args)]
pub(crate) struct UninstallArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Principal of a proxy canister to route the management canister call through.
    #[arg(long)]
    pub(crate) proxy: Option<Principal>,
}

pub(crate) async fn exec(ctx: &Context, args: &UninstallArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();
    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;
    let calls = icp_app::calls::calls(agent.clone(), args.proxy)?;
    let cid = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;

    proxy_management::uninstall_code(
        calls.as_ref(),
        UninstallCodeArgs {
            canister_id: cid,
            sender_canister_version: None,
        },
    )
    .await?;

    Ok(())
}
