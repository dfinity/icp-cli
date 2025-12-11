use clap::Args;
use ic_agent::export::Principal;
use ic_management_canister_types::{CanisterStatusResult, EnvironmentVariable, LogVisibility};
use icp::{
    context::{CanisterSelection, Context, EnvironmentSelection, NetworkSelection},
    identity::IdentitySelection,
};
use serde::Serialize;
use std::fmt::Write;

use crate::{commands::args, options};

#[derive(Debug, Args)]
pub(crate) struct StatusArgs {
    /// An optional canister name or principal to target.
    /// When using a name, an enviroment must be specified
    pub(crate) canister: Option<args::Canister>,

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

pub(crate) async fn exec(ctx: &Context, args: &StatusArgs) -> Result<(), anyhow::Error> {
    struct Selection {
        environment: EnvironmentSelection,
        network: NetworkSelection,
        identity: IdentitySelection,
    }

    let selections = Selection {
        environment: args.environment.clone().into(),
        network: args.network.clone().into(),
        identity: args.identity.clone().into(),
    };

    let cids = get_principals(
        ctx,
        args.canister.clone(),
        &selections.environment,
        &selections.network,
    )
    .await?;

    if args.id_only {
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
        // Retrieve canister status from management canister
        let (result,) = mgmt.canister_status(cid).await?;
        let status = CliCanisterStatusResult {
            id: cid.to_owned(),
            status: SerializableCanisterStatusResult::from(&result),
        };

        let output = match args.json_format {
            true => {
                serde_json::to_string(&status).expect("Serializing status result to json failed")
            }
            false => build_output(&status).expect("Failed to build canister status output"),
        };

        ctx.term
            .write_line(output.trim())
            .expect("Failed to write output to the terminal");
    }

    Ok(())
}

#[derive(Serialize)]
struct CliCanisterStatusResult {
    id: Principal,

    #[serde(flatten)]
    status: SerializableCanisterStatusResult,
}

/// Serializable wrapper for CanisterStatusResult that converts Nat fields to String
#[derive(Serialize)]
struct SerializableCanisterStatusResult {
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
    fn from(result: &CanisterStatusResult) -> Self {
        Self {
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

fn build_output(result: &CliCanisterStatusResult) -> Result<String, anyhow::Error> {
    let mut buf = String::new();
    let status = &result.status;

    writeln!(&mut buf, "Canister Id: {}", result.id)?;
    writeln!(&mut buf, "Canister Status Report:")?;
    writeln!(&mut buf, "  Status: {:?}", status.status)?;

    let settings = &status.settings;
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
        status.module_hash.clone().unwrap_or("<none>".to_string())
    )?;

    writeln!(&mut buf, "  Memory size: {}", status.memory_size)?;
    writeln!(&mut buf, "  Cycles: {}", status.cycles)?;
    writeln!(&mut buf, "  Reserved cycles: {}", status.reserved_cycles)?;
    writeln!(
        &mut buf,
        "  Idle cycles burned per day: {}",
        status.idle_cycles_burned_per_day
    )?;

    let stats = &status.query_stats;
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
