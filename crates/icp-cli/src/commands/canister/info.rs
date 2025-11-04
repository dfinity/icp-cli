use clap::Args;
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::CanisterStatusResult;
use icp::{agent, context::GetCanisterIdAndAgentError, identity, network};
use itertools::Itertools;

use icp::context::Context;

use crate::commands::args;
use icp::store_id::LookupIdError;

#[derive(Debug, Args)]
pub(crate) struct InfoArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error(transparent)]
    Lookup(#[from] LookupIdError),

    #[error(transparent)]
    Status(#[from] AgentError),

    #[error(transparent)]
    GetCanisterIdAndAgent(#[from] GetCanisterIdAndAgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &InfoArgs) -> Result<(), CommandError> {
    let selections = args.cmd_args.selections();
    let (cid, agent) = ctx
        .get_canister_id_and_agent(
            &selections.canister,
            &selections.environment,
            &selections.network,
            &selections.identity,
        )
        .await?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Retrieve canister status from management canister
    let (result,) = mgmt.canister_status(&cid).await?;

    // Info printout
    print_info(&result);

    Ok(())
}

pub(crate) fn print_info(result: &CanisterStatusResult) {
    let controllers: Vec<String> = result
        .settings
        .controllers
        .iter()
        .map(|p| p.to_string())
        .sorted()
        .collect();

    eprintln!("Controllers: {}", controllers.join(", "));

    match &result.module_hash {
        Some(hash) => {
            let hex_string: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            eprintln!("Module hash: 0x{hex_string}");
        }
        None => eprintln!("Module hash: <none>"),
    }
}
