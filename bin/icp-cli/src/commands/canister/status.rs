use clap::Parser;
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::StatusCallResult;
use snafu::Snafu;

use crate::{
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::LookupError as LookupIdError,
};

#[derive(Debug, Parser)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: String,

    #[clap(flatten)]
    identity: IdentityOpt,

    #[clap(flatten)]
    environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest
    let pm = ctx.project()?;

    // Select canister to query
    let (_, c) = pm
        .canisters
        .iter()
        .find(|(_, c)| cmd.name == c.name)
        .ok_or(CommandError::CanisterNotFound { name: cmd.name })?;

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

    // Ensure canister is included in the environment
    if !ecs.contains(&c.name) {
        return Err(CommandError::EnvironmentCanister {
            environment: env.name.to_owned(),
            canister: c.name.to_owned(),
        });
    }

    // Lookup the canister id
    let cid = ctx.id_store.lookup(&c.name)?;

    // Load identity
    ctx.require_identity(cmd.identity.name());

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    ctx.require_network(
        env.network
            .as_ref()
            .expect("no network specified in environment"),
    );

    // Prepare agent
    let agent = ctx.agent()?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(agent);

    // Retrieve canister status from management canister
    let (result,) = mgmt.canister_status(&cid).await?;

    // Status printout
    print_status(&result);

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

    #[snafu(transparent)]
    Agent { source: AgentError },
}

pub fn print_status(result: &StatusCallResult) {
    eprintln!("Canister Status Report:");
    eprintln!("  Status: {}", result.status);

    let settings = &result.settings;
    let controllers: Vec<String> = settings.controllers.iter().map(|p| p.to_string()).collect();
    eprintln!("  Controllers: {}", controllers.join(", "));
    eprintln!("  Compute allocation: {}", settings.compute_allocation);
    eprintln!("  Memory allocation: {}", settings.memory_allocation);
    eprintln!("  Freezing threshold: {}", settings.freezing_threshold);

    if let Some(limit) = &settings.reserved_cycles_limit {
        eprintln!("  Reserved cycles limit: {}", limit);
    }
    if let Some(limit) = &settings.wasm_memory_limit {
        eprintln!("  Wasm memory limit: {}", limit);
    }
    if let Some(threshold) = &settings.wasm_memory_threshold {
        eprintln!("  Wasm memory threshold: {}", threshold);
    }
    eprintln!("  Log visibility: {:?}", settings.log_visibility);

    match &result.module_hash {
        Some(hash) => {
            let hex_string: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
            eprintln!("  Module hash: 0x{}", hex_string);
        }
        None => eprintln!("  Module hash: <none>"),
    }

    eprintln!("  Memory size: {}", result.memory_size);
    eprintln!("  Cycles: {}", result.cycles);
    eprintln!("  Reserved cycles: {}", result.reserved_cycles);
    eprintln!(
        "  Idle cycles burned per day: {}",
        result.idle_cycles_burned_per_day
    );

    let stats = &result.query_stats;
    eprintln!("  Query stats:");
    eprintln!("    Calls: {}", stats.num_calls_total);
    eprintln!("    Instructions: {}", stats.num_instructions_total);
    eprintln!(
        "    Req payload bytes: {}",
        stats.request_payload_bytes_total
    );
    eprintln!(
        "    Res payload bytes: {}",
        stats.response_payload_bytes_total
    );
}
