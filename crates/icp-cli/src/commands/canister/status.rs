use clap::Parser;
use ic_agent::{AgentError, export::Principal};
use ic_management_canister_types::{CanisterStatusResult, LogVisibility};
use icp::{agent, identity, network};

use crate::{
    commands::Context,
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Parser)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
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
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await?;

    // Load identity
    let id = ctx.identity.load(cmd.identity.into()).await?;

    // Load target environment
    let env =
        p.environments
            .get(cmd.environment.name())
            .ok_or(CommandError::EnvironmentNotFound {
                name: cmd.environment.name().to_owned(),
            })?;

    // Access network
    let access = ctx.network.access(&env.network).await?;

    // Agent
    let agent = ctx.agent.create(id, &access.url).await?;

    if let Some(k) = access.root_key {
        agent.set_root_key(k);
    }

    // Ensure canister is included in the environment
    if !env.canisters.contains_key(&cmd.name) {
        return Err(CommandError::EnvironmentCanister {
            environment: env.name.to_owned(),
            canister: cmd.name.to_owned(),
        });
    }

    // Lookup the canister id
    let cid = ctx.ids.lookup(&Key {
        network: env.network.name.to_owned(),
        environment: env.name.to_owned(),
        canister: cmd.name.to_owned(),
    })?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Retrieve canister status from management canister
    let (result,) = mgmt.canister_status(&cid).await?;

    // Status printout
    print_status(ctx, &result);

    Ok(())
}

pub fn print_status(_ctx: &Context, result: &CanisterStatusResult) {
    tracing::info!("Canister Status Report:");
    tracing::info!("  Status: {:?}", result.status);

    let settings = &result.settings;
    let controllers: Vec<String> = settings.controllers.iter().map(|p| p.to_string()).collect();
    tracing::info!("  Controllers: {}", controllers.join(", "));
    tracing::info!("  Compute allocation: {}", settings.compute_allocation);
    tracing::info!("  Memory allocation: {}", settings.memory_allocation);
    tracing::info!("  Freezing threshold: {}", settings.freezing_threshold);

    tracing::info!(
        "  Reserved cycles limit: {}",
        settings.reserved_cycles_limit
    );
    tracing::info!("  Wasm memory limit: {}", settings.wasm_memory_limit);
    tracing::info!(
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
    tracing::info!("  Log visibility: {log_visibility}");

    // Display environment variables configured for this canister
    // Environment variables are key-value pairs that can be accessed within the canister
    if settings.environment_variables.is_empty() {
        tracing::info!("  Environment Variables: N/A");
    } else {
        tracing::info!("  Environment Variables:");
        for v in &settings.environment_variables {
            tracing::info!("    Name: {}, Value: {}", v.name, v.value);
        }
    }

    match &result.module_hash {
        Some(hash) => {
            let hex_string: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            tracing::info!("  Module hash: 0x{hex_string}");
        }
        None => tracing::info!("  Module hash: <none>"),
    }

    tracing::info!("  Memory size: {}", result.memory_size);
    tracing::info!("  Cycles: {}", result.cycles);
    tracing::info!("  Reserved cycles: {}", result.reserved_cycles);
    tracing::info!(
        "  Idle cycles burned per day: {}",
        result.idle_cycles_burned_per_day
    );

    let stats = &result.query_stats;
    tracing::info!("  Query stats:");
    tracing::info!("    Calls: {}", stats.num_calls_total);
    tracing::info!("    Instructions: {}", stats.num_instructions_total);
    tracing::info!(
        "    Req payload bytes: {}",
        stats.request_payload_bytes_total
    );
    tracing::info!(
        "    Res payload bytes: {}",
        stats.response_payload_bytes_total
    );
}
