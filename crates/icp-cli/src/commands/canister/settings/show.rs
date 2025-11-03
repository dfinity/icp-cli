use clap::Args;
use ic_agent::{AgentError, export::Principal};
use ic_management_canister_types::{CanisterStatusResult, LogVisibility};
use icp::{agent, identity, network};

use icp::context::{Context, GetAgentForEnvError, GetCanisterIdAndAgentError, GetEnvironmentError};

use crate::commands::args;
use icp::store_id::LookupError as LookupIdError;

#[derive(Debug, Args)]
pub(crate) struct ShowArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error(transparent)]
    Lookup(#[from] LookupIdError),

    #[error(transparent)]
    Status(#[from] AgentError),

    #[error(transparent)]
    GetAgentForEnv(#[from] GetAgentForEnvError),

    #[error(transparent)]
    GetCanisterIdAndAgent(#[from] GetCanisterIdAndAgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &ShowArgs) -> Result<(), CommandError> {
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

    // Get canister settings
    let (result,) = mgmt.canister_status(&cid).await?;
    print_settings(&result);

    Ok(())
}

pub(crate) fn print_settings(result: &CanisterStatusResult) {
    eprintln!("Canister Settings:");

    let settings = &result.settings;
    let controllers: Vec<String> = settings.controllers.iter().map(|p| p.to_string()).collect();
    eprintln!("  Controllers: {}", controllers.join(", "));
    eprintln!("  Compute allocation: {}", settings.compute_allocation);
    eprintln!("  Memory allocation: {}", settings.memory_allocation);
    eprintln!("  Freezing threshold: {}", settings.freezing_threshold);

    eprintln!(
        "  Reserved cycles limit: {}",
        settings.reserved_cycles_limit
    );
    eprintln!("  Wasm memory limit: {}", settings.wasm_memory_limit);
    eprintln!(
        "  Wasm memory threshold: {}",
        settings.wasm_memory_threshold
    );

    let log_visibility = match &settings.log_visibility {
        LogVisibility::Controllers => "Controllers".to_string(),
        LogVisibility::Public => "Public".to_string(),
        LogVisibility::AllowedViewers(viewers) => {
            if viewers.is_empty() {
                "Allowed viewers list is empty".to_string()
            } else {
                let mut viewers: Vec<_> = viewers.iter().map(Principal::to_text).collect();
                viewers.sort();
                format!("Allowed viewers: {}", viewers.join(", "))
            }
        }
    };
    eprintln!("  Log visibility: {log_visibility}");

    // Display environment variables configured for this canister
    // Environment variables are key-value pairs that can be accessed within the canister
    if settings.environment_variables.is_empty() {
        eprintln!("  Environment Variables: N/A",);
    } else {
        eprintln!("  Environment Variables:");
        for v in &settings.environment_variables {
            eprintln!("    Name: {}, Value: {}", v.name, v.value);
        }
    }
}
