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

pub fn print_status(ctx: &Context, result: &CanisterStatusResult) {
    ctx.println("Canister Status Report:");
    ctx.println(&format!("  Status: {:?}", result.status));

    let settings = &result.settings;
    let controllers: Vec<String> = settings.controllers.iter().map(|p| p.to_string()).collect();
    ctx.println(&format!("  Controllers: {}", controllers.join(", ")));
    ctx.println(&format!(
        "  Compute allocation: {}",
        settings.compute_allocation
    ));
    ctx.println(&format!(
        "  Memory allocation: {}",
        settings.memory_allocation
    ));
    ctx.println(&format!(
        "  Freezing threshold: {}",
        settings.freezing_threshold
    ));

    ctx.println(&format!(
        "  Reserved cycles limit: {}",
        settings.reserved_cycles_limit
    ));
    ctx.println(&format!(
        "  Wasm memory limit: {}",
        settings.wasm_memory_limit
    ));
    ctx.println(&format!(
        "  Wasm memory threshold: {}",
        settings.wasm_memory_threshold
    ));

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
    ctx.println(&format!("  Log visibility: {log_visibility}"));

    // Display environment variables configured for this canister
    // Environment variables are key-value pairs that can be accessed within the canister
    if settings.environment_variables.is_empty() {
        ctx.println("  Environment Variables: N/A");
    } else {
        ctx.println("  Environment Variables:");
        for v in &settings.environment_variables {
            ctx.println(&format!("    Name: {}, Value: {}", v.name, v.value));
        }
    }

    match &result.module_hash {
        Some(hash) => {
            let hex_string: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            ctx.println(&format!("  Module hash: 0x{hex_string}"));
        }
        None => ctx.println("  Module hash: <none>"),
    }

    ctx.println(&format!("  Memory size: {}", result.memory_size));
    ctx.println(&format!("  Cycles: {}", result.cycles));
    ctx.println(&format!("  Reserved cycles: {}", result.reserved_cycles));
    ctx.println(&format!(
        "  Idle cycles burned per day: {}",
        result.idle_cycles_burned_per_day
    ));

    let stats = &result.query_stats;
    ctx.println("  Query stats:");
    ctx.println(&format!("    Calls: {}", stats.num_calls_total));
    ctx.println(&format!(
        "    Instructions: {}",
        stats.num_instructions_total
    ));
    ctx.println(&format!(
        "    Req payload bytes: {}",
        stats.request_payload_bytes_total
    ));
    ctx.println(&format!(
        "    Res payload bytes: {}",
        stats.response_payload_bytes_total
    ));
}
