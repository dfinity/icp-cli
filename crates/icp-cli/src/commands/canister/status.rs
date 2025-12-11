use anyhow::{anyhow, bail};
use clap::Args;
use ic_agent::{Agent, AgentError, export::Principal};
use ic_management_canister_types::{CanisterStatusResult, EnvironmentVariable, LogVisibility};
use icp::{
    context::{CanisterSelection, Context, EnvironmentSelection, NetworkSelection},
    identity::IdentitySelection,
};
use serde::Serialize;
use std::fmt::Write;
use tracing::debug;

use crate::{commands::args, options};

/// Error code returned by the replica if the target canister is not found
const E_CANISTER_NOT_FOUND: &str = "IC0301";
/// Error code returned by the replica if the caller is not a controller
const E_NOT_A_CONTROLLER: &str = "IC0512";

#[derive(Debug, Args)]
pub(crate) struct StatusArgs {
    /// An optional canister name or principal to target.
    /// When using a name, an enviroment must be specified.
    pub(crate) canister: Option<args::Canister>,

    #[command(flatten)]
    pub(crate) options: StatusArgsOptions,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct StatusArgsOptions {
    #[command(flatten)]
    pub(crate) network: options::NetworkOpt,

    #[command(flatten)]
    pub(crate) environment: options::EnvironmentOpt,

    #[command(flatten)]
    pub(crate) identity: options::IdentityOpt,

    /// Only print the canister ids
    #[arg(short, long, conflicts_with_all = ["json_format"])]
    pub id_only: bool,

    /// Format output in json
    #[arg(long = "json")]
    pub json_format: bool,

    /// Show the only the public information.
    /// Skips trying to get the status from the management canister and
    /// looks up public information from the state tree.
    #[arg(short, long)]
    pub public: bool,
}

/// Fetch the list of canister ids from the id_store
/// This will throw an error if the canisters have not been created yet
async fn get_principals(
    ctx: &Context,
    canister: Option<args::Canister>,
    environment: &EnvironmentSelection,
    network: &NetworkSelection,
) -> Result<Vec<Principal>, anyhow::Error> {
    let mut cids = Vec::<Principal>::new();

    match canister {
        Some(canister) => {
            let canister_selection: CanisterSelection = canister.clone().into();
            let cid = ctx
                .get_canister_id(&canister_selection, network, environment)
                .await?;
            cids.push(cid);
        }
        None => {
            let env = ctx.get_environment(environment).await?;
            for (_, c) in env.canisters.values() {
                let cid = ctx
                    .get_canister_id(
                        &CanisterSelection::Named(c.name.clone()),
                        network,
                        environment,
                    )
                    .await?;
                cids.push(cid);
            }
        }
    };

    Ok(cids)
}

async fn read_state_tree_canister_controllers(
    agent: &Agent,
    cid: Principal,
) -> Result<Option<Vec<Principal>>, anyhow::Error> {
    let controllers = match agent.read_state_canister_controllers(cid).await {
        Ok(controllers) => controllers,
        Err(AgentError::LookupPathAbsent(_)) => {
            debug!("Couldn't find a path to the controllers in the state tree for {cid}");
            return Err(anyhow!("Canister {cid} was not found."));
        }
        Err(AgentError::InvalidCborData(_)) => {
            return Err(anyhow!(
                "Invalid cbor data in controllers canister info for canister {cid}"
            ));
        }
        Err(e) => {
            return Err(anyhow!(
                "Error fetching controllers from the state tree for {cid}: {e}"
            ));
        }
    };
    Ok(Some(controllers))
}

/// None can indicate either of these, but we can't tell from here:
/// - the canister doesn't exist
/// - the canister exists but does not have a module installed
async fn read_state_tree_canister_module_hash(
    agent: &Agent,
    cid: Principal,
) -> Result<Option<Vec<u8>>, anyhow::Error> {
    let module_hash = match agent.read_state_canister_module_hash(cid).await {
        Ok(blob) => Some(blob),
        Err(AgentError::LookupPathAbsent(_)) => None,
        Err(e) => {
            return Err(anyhow!(
                "Error reading the module hash from the state tree for {cid}: {e}"
            ));
        }
    };

    Ok(module_hash)
}

async fn build_public_status(
    agent: &Agent,
    cid: Principal,
) -> Result<PublicCanisterStatusResult, anyhow::Error> {
    let controllers = match read_state_tree_canister_controllers(agent, cid).await? {
        Some(controllers) => controllers.iter().map(|p| p.to_string()).collect(),
        None => Vec::new(),
    };
    let module_hash = read_state_tree_canister_module_hash(agent, cid)
        .await?
        .map(|hash| {
            format!(
                "0x{}",
                hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
            )
        });

    Ok(PublicCanisterStatusResult {
        id: cid,
        controllers,
        module_hash,
    })
}

pub(crate) async fn exec(ctx: &Context, args: &StatusArgs) -> Result<(), anyhow::Error> {
    struct Selection {
        environment: EnvironmentSelection,
        network: NetworkSelection,
        identity: IdentitySelection,
    }

    let selections = Selection {
        environment: args.options.environment.clone().into(),
        network: args.options.network.clone().into(),
        identity: args.options.identity.clone().into(),
    };

    let cids = get_principals(
        ctx,
        args.canister.clone(),
        &selections.environment,
        &selections.network,
    )
    .await?;

    if args.options.id_only {
        for cid in cids.iter() {
            let _ = ctx.term.write_line(&format!("{cid}"));
        }
        return Ok(());
    }

    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    for cid in cids.iter() {
        let output = match args.options.public {
            true => {
                // We construct the status out of the state tree
                let status = build_public_status(&agent, cid.to_owned()).await?;

                match args.options.json_format {
                    true => serde_json::to_string(&status)
                        .expect("Serializing status result to json failed"),
                    false => build_public_output(&status)
                        .expect("Failed to build canister status output"),
                }
            }
            false => {
                // Retrieve canister status from management canister
                match mgmt.canister_status(cid).await {
                    Ok((result,)) => {
                        let status =
                            SerializableCanisterStatusResult::from(cid.to_owned(), &result);

                        match args.options.json_format {
                            true => serde_json::to_string(&status)
                                .expect("Serializing status result to json failed"),
                            false => build_output(&status)
                                .expect("Failed to build canister status output"),
                        }
                    }
                    Err(AgentError::UncertifiedReject {
                        reject,
                        operation: _,
                    }) => {
                        if reject.error_code.as_deref() == Some(E_CANISTER_NOT_FOUND) {
                            // The canister does not exist
                            bail!("Canister {cid} was not found.");
                        }

                        if reject.error_code.as_deref() != Some(E_NOT_A_CONTROLLER) {
                            // We don't know this error code
                            bail!("Error looking up canister {cid}: {:?}", reject.error_code);
                        }

                        // We got E_NOT_A_CONTROLLER so we fallback on fetching the public status
                        let status = build_public_status(&agent, cid.to_owned()).await?;

                        match args.options.json_format {
                            true => serde_json::to_string(&status)
                                .expect("Serializing status result to json failed"),
                            false => build_public_output(&status)
                                .expect("Failed to build canister status output"),
                        }
                    }
                    Err(e) => {
                        bail!("Unknown error fetching canister {cid} status: {e}");
                    }
                }
            }
        };

        ctx.term
            .write_line(output.trim())
            .expect("Failed to write output to the terminal");
    }

    Ok(())
}

#[derive(Serialize)]
struct PublicCanisterStatusResult {
    id: Principal,
    controllers: Vec<String>,
    module_hash: Option<String>,
}

/// Serializable wrapper for CanisterStatusResult that converts Nat fields to String
#[derive(Serialize)]
struct SerializableCanisterStatusResult {
    id: Principal,
    status: String,
    settings: SerializableCanisterSettings,
    module_hash: Option<String>,
    memory_size: String,
    cycles: String,
    reserved_cycles: String,
    idle_cycles_burned_per_day: String,
    query_stats: SerializableQueryStats,
}

#[derive(Serialize)]
struct SerializableCanisterSettings {
    controllers: Vec<String>,
    compute_allocation: String,
    memory_allocation: String,
    freezing_threshold: String,
    reserved_cycles_limit: String,
    wasm_memory_limit: String,
    wasm_memory_threshold: String,
    log_visibility: SerializableLogVisibility,
    environment_variables: Vec<EnvironmentVariable>,
}

#[derive(Serialize, Clone)]
#[serde(tag = "type", content = "value")]
enum SerializableLogVisibility {
    Controllers,
    Public,
    AllowedViewers(Vec<String>),
}

#[derive(Serialize)]
struct SerializableQueryStats {
    num_calls_total: String,
    num_instructions_total: String,
    request_payload_bytes_total: String,
    response_payload_bytes_total: String,
}

impl SerializableCanisterStatusResult {
    fn from(id: Principal, result: &CanisterStatusResult) -> Self {
        Self {
            id,
            status: format!("{:?}", result.status),
            settings: SerializableCanisterSettings::from(&result.settings),
            module_hash: result.module_hash.as_ref().map(|hash| {
                format!(
                    "0x{}",
                    hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
                )
            }),
            memory_size: result.memory_size.to_string(),
            cycles: result.cycles.to_string(),
            reserved_cycles: result.reserved_cycles.to_string(),
            idle_cycles_burned_per_day: result.idle_cycles_burned_per_day.to_string(),
            query_stats: SerializableQueryStats::from(&result.query_stats),
        }
    }
}

impl SerializableCanisterSettings {
    fn from(settings: &ic_management_canister_types::DefiniteCanisterSettings) -> Self {
        Self {
            controllers: settings.controllers.iter().map(|p| p.to_string()).collect(),
            compute_allocation: settings.compute_allocation.to_string(),
            memory_allocation: settings.memory_allocation.to_string(),
            freezing_threshold: settings.freezing_threshold.to_string(),
            reserved_cycles_limit: settings.reserved_cycles_limit.to_string(),
            wasm_memory_limit: settings.wasm_memory_limit.to_string(),
            wasm_memory_threshold: settings.wasm_memory_threshold.to_string(),
            log_visibility: SerializableLogVisibility::from(&settings.log_visibility),
            environment_variables: settings.environment_variables.clone(),
        }
    }
}

impl SerializableLogVisibility {
    fn from(visibility: &LogVisibility) -> Self {
        match visibility {
            LogVisibility::Controllers => Self::Controllers,
            LogVisibility::Public => Self::Public,
            LogVisibility::AllowedViewers(viewers) => {
                Self::AllowedViewers(viewers.iter().map(|p| p.to_string()).collect())
            }
        }
    }
}

impl SerializableQueryStats {
    fn from(stats: &ic_management_canister_types::QueryStats) -> Self {
        Self {
            num_calls_total: stats.num_calls_total.to_string(),
            num_instructions_total: stats.num_instructions_total.to_string(),
            request_payload_bytes_total: stats.request_payload_bytes_total.to_string(),
            response_payload_bytes_total: stats.response_payload_bytes_total.to_string(),
        }
    }
}

fn build_public_output(result: &PublicCanisterStatusResult) -> Result<String, anyhow::Error> {
    let mut buf = String::new();
    writeln!(&mut buf, "Canister Id: {}", result.id)?;
    writeln!(&mut buf, "Canister Status Report:")?;

    writeln!(&mut buf, "  Controllers: {}", result.controllers.join(", "))?;
    writeln!(
        &mut buf,
        "  Module hash: {}",
        &result.module_hash.clone().unwrap_or("<none>".to_string())
    )?;

    Ok(buf)
}

fn build_output(result: &SerializableCanisterStatusResult) -> Result<String, anyhow::Error> {
    let mut buf = String::new();

    writeln!(&mut buf, "Canister Id: {}", result.id)?;
    writeln!(&mut buf, "Canister Status Report:")?;
    writeln!(&mut buf, "  Status: {}", &result.status)?;

    let settings = &result.settings;
    writeln!(
        &mut buf,
        "  Controllers: {}",
        settings.controllers.join(", ")
    )?;
    writeln!(
        &mut buf,
        "  Compute allocation: {}",
        settings.compute_allocation
    )?;
    writeln!(
        &mut buf,
        "  Memory allocation: {}",
        settings.memory_allocation
    )?;
    writeln!(
        &mut buf,
        "  Freezing threshold: {}",
        settings.freezing_threshold
    )?;

    writeln!(
        &mut buf,
        "  Reserved cycles limit: {}",
        settings.reserved_cycles_limit
    )?;
    writeln!(
        &mut buf,
        "  Wasm memory limit: {}",
        settings.wasm_memory_limit
    )?;
    writeln!(
        &mut buf,
        "  Wasm memory threshold: {}",
        settings.wasm_memory_threshold
    )?;

    let log_visibility = match settings.log_visibility.clone() {
        SerializableLogVisibility::Controllers => "Controllers".to_string(),
        SerializableLogVisibility::Public => "Public".to_string(),
        SerializableLogVisibility::AllowedViewers(mut viewers) => {
            if viewers.is_empty() {
                "Allowed viewers list is empty".to_string()
            } else {
                viewers.sort();
                format!("Allowed viewers: {}", viewers.join(", "))
            }
        }
    };
    writeln!(&mut buf, "  Log visibility: {log_visibility}")?;

    // Display environment variables configured for this canister
    // Environment variables are key-value pairs that can be accessed within the canister
    if settings.environment_variables.is_empty() {
        writeln!(&mut buf, "  Environment Variables: N/A",)?;
    } else {
        writeln!(&mut buf, "  Environment Variables:")?;
        for v in &settings.environment_variables {
            writeln!(&mut buf, "    Name: {}, Value: {}", v.name, v.value)?;
        }
    }

    writeln!(
        &mut buf,
        "  Module hash: {}",
        &result.module_hash.clone().unwrap_or("<none>".to_string())
    )?;

    writeln!(&mut buf, "  Memory size: {}", result.memory_size)?;
    writeln!(&mut buf, "  Cycles: {}", result.cycles)?;
    writeln!(&mut buf, "  Reserved cycles: {}", result.reserved_cycles)?;
    writeln!(
        &mut buf,
        "  Idle cycles burned per day: {}",
        result.idle_cycles_burned_per_day
    )?;

    let stats = &result.query_stats;
    writeln!(&mut buf, "  Query stats:")?;
    writeln!(&mut buf, "    Calls: {}", stats.num_calls_total)?;
    writeln!(
        &mut buf,
        "    Instructions: {}",
        stats.num_instructions_total
    )?;
    writeln!(
        &mut buf,
        "    Req payload bytes: {}",
        stats.request_payload_bytes_total
    )?;
    writeln!(
        &mut buf,
        "    Res payload bytes: {}",
        stats.response_payload_bytes_total
    )?;

    Ok(buf)
}
