use clap::Parser;
use ic_agent::{AgentError, export::Principal};
use ic_management_canister_types::{CanisterStatusResult, LogVisibility};
use snafu::Snafu;

use crate::context::{Context, ContextAgentError, ContextProjectError};
use crate::options::{EnvironmentOpt, IdentityOpt};
use crate::store_id::{Key, LookupError as LookupIdError};

#[derive(Debug, Parser)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest.
    let pm = ctx.project()?;

    // Load target environment
    let env = pm
        .environments
        .iter()
        .find(|&v| v.name == cmd.environment.name())
        .ok_or(CommandError::EnvironmentNotFound {
            name: cmd.environment.name().to_owned(),
        })?;

    // Collect environment canisters
    let ecs = env.canisters.clone().unwrap_or(
        pm.canisters
            .iter()
            .map(|(_, c)| c.name.to_owned())
            .collect(),
    );

    // Select canister to query
    let (_, c) = pm
        .canisters
        .iter()
        .find(|(_, c)| cmd.name == c.name)
        .ok_or_else(|| CommandError::CanisterNotFound {
            name: cmd.name.clone(),
        })?;

    // Ensure canister is included in the environment
    if !ecs.contains(&c.name) {
        return Err(CommandError::EnvironmentCanister {
            environment: env.name.to_owned(),
            canister: c.name.to_owned(),
        });
    }

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

    // Lookup the canister id
    let cid = ctx.id_store.lookup(&Key {
        network: network.to_owned(),
        environment: env.name.to_owned(),
        canister: c.name.to_owned(),
    })?;

    // Load identity
    ctx.require_identity(cmd.identity.name());

    // Setup network
    ctx.require_network(network);

    // Prepare agent
    let agent = ctx.agent()?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(agent);

    // Get canister settings
    let (result,) = mgmt.canister_status(&cid).await?;
    print_settings(&result);

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    GetAgent { source: ContextAgentError },

    #[snafu(transparent)]
    Agent { source: AgentError },
}

pub fn print_settings(result: &CanisterStatusResult) {
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
    eprintln!("  Log visibility: {}", log_visibility);

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
