use clap::Args;
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::CanisterStatusResult;
use icp::{agent, identity, network};
use itertools::Itertools;

use crate::{
    commands::{
        Context, ContextError,
        args::{ArgContext, ArgumentError},
    },
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Debug, Args)]
pub(crate) struct InfoArgs {
    /// The name of the canister within the current project
    pub(crate) name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,
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
    Argument(#[from] ArgumentError),

    #[error(transparent)]
    Context(#[from] ContextError),

    #[error(transparent)]
    Status(#[from] AgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &InfoArgs) -> Result<(), CommandError> {
    let arg_ctx = ArgContext::new(
        ctx,
        args.environment.clone(),
        None,
        args.identity.clone(),
        vec![&args.name],
    )
    .await?;

    let agent = ctx.get_agent(&arg_ctx).await?;
    let canister_id = ctx.resolve_canister_id(&arg_ctx, &args.name).await?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Retrieve canister status from management canister
    let (result,) = mgmt.canister_status(&canister_id).await?;

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
