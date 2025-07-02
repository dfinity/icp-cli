use crate::env::{EnvGetAgentError, GetProjectError};
use crate::options::NetworkOpt;
use crate::{env::Env, store_id::LookupError as LookupIdError};
use clap::Parser;
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::StatusCallResult;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterStatusCmd {
    /// The name of the canister within the current project
    pub name: String,

    #[clap(flatten)]
    network: NetworkOpt,
}

pub async fn exec(env: &Env, cmd: CanisterStatusCmd) -> Result<(), CanisterStatusError> {
    env.require_network(cmd.network.name());

    // Load the project manifest, which defines the canisters to be built.
    let pm = env.project()?;

    // Select canister to query
    let (_, c) = pm
        .canisters
        .iter()
        .find(|(_, c)| cmd.name == c.name)
        .ok_or(CanisterStatusError::CanisterNotFound { name: cmd.name })?;

    // Lookup the canister id
    let cid = env.id_store.lookup(&c.name)?;

    let agent = env.agent()?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(agent);

    // Retrieve canister status from management canister
    let (result,) = mgmt.canister_status(&cid).await?;

    // Status printout
    print_status(&result);

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterStatusError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(transparent)]
    EnvGetAgent { source: EnvGetAgentError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

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
