use anyhow::{anyhow, bail};
use clap::Args;
use clap_complete::ArgValueCandidates;
use ic_agent::{Agent, AgentError, agent::RejectResponse, export::Principal};
use ic_management_canister_types::{CanisterIdRecord, CanisterStatusResult, EnvironmentVariable};
use icp::{
    canister::Visibility,
    context::{CanisterSelection, Context, EnvironmentSelection, NetworkSelection},
    identity::IdentitySelection,
};
use serde::Serialize;
use std::fmt::Write;
use tracing::debug;

use icp::operations::{proxy::UpdateOrProxyError, proxy_management};

use crate::{
    commands::{
        args,
        canister::{format_controllers, format_visibility},
    },
    options,
};

/// Error code returned by the replica if the target canister is not found
const E_CANISTER_NOT_FOUND: &str = "IC0301";
/// Error codes the replica returns when the caller may not read the status.
///
/// Which one comes back depends on the replica version and on whether the
/// subnet has administrators: `IC0542` since status visibility was introduced,
/// `IC0541` on subnets with administrators before that, and `IC0512` otherwise.
const E_STATUS_ACCESS_DENIED: [&str; 3] = ["IC0512", "IC0541", "IC0542"];

/// The reject carried by a direct update call, however it was delivered.
///
/// The replica checks who may read the status both when accepting the ingress
/// message and again during execution, so the same denial arrives uncertified
/// from the first and certified from the second.
fn direct_call_reject(err: &UpdateOrProxyError) -> Option<&RejectResponse> {
    match err {
        UpdateOrProxyError::DirectUpdateCall {
            source:
                AgentError::CertifiedReject { reject, .. }
                | AgentError::UncertifiedReject { reject, .. },
        } => Some(reject),
        _ => None,
    }
}

/// Show the status of canister(s).
///
/// By default this queries the status endpoint of the management canister.
/// If the caller may not read the status, falls back on fetching public
/// information from the state tree.
#[derive(Debug, Args)]
#[command(after_long_help = "\
Examples:

    # Status of all canisters in the local environment
    icp canister status

    # Status of one canister by name
    icp canister status backend -e local

    # Print only canister IDs (useful for scripting)
    icp canister status -i

    # JSON output for all canisters
    icp canister status --json
")]
pub(crate) struct StatusArgs {
    /// An optional canister name or principal to target.
    /// When using a name, an environment must be specified.
    /// If omitted, shows status for all canisters in the environment.
    #[arg(add = ArgValueCandidates::new(crate::complete::canisters))]
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

    /// Principal of a proxy canister to route the management canister call through.
    #[arg(long)]
    pub proxy: Option<Principal>,
}

/// Fetch the list of canister ids from the id_store
/// This will throw an error if the canisters have not been created yet
async fn get_principals(
    ctx: &Context,
    canister: Option<args::Canister>,
    environment: &EnvironmentSelection,
    network: &NetworkSelection,
) -> Result<Vec<(Option<String>, Principal)>, anyhow::Error> {
    let mut cids = Vec::<(Option<String>, Principal)>::new();

    match canister {
        Some(canister) => {
            let canister_selection: CanisterSelection = canister.clone().into();
            let cid = ctx
                .get_canister_id(&canister_selection, network, environment)
                .await?;
            match canister {
                args::Canister::Name(name) => cids.push((Some(name), cid)),
                args::Canister::Principal(_) => cids.push((None, cid)),
            };
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
                cids.push((Some(c.name.clone()), cid));
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
    maybe_name: Option<String>,
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
        name: maybe_name,
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
        for (_, cid) in cids.iter() {
            println!("{cid}");
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

    for (i, (maybe_name, cid)) in cids.iter().enumerate() {
        let output = match args.options.public {
            true => {
                // We construct the status out of the state tree
                let status =
                    build_public_status(&agent, cid.to_owned(), maybe_name.clone()).await?;

                match args.options.json_format {
                    true => serde_json::to_string(&status)
                        .expect("Serializing status result to json failed"),
                    false => build_public_output(&status)
                        .expect("Failed to build canister status output"),
                }
            }
            false => {
                // Retrieve canister status from management canister
                match proxy_management::canister_status(
                    &agent,
                    args.options.proxy,
                    CanisterIdRecord { canister_id: *cid },
                )
                .await
                {
                    Ok(result) => {
                        let status = SerializableCanisterStatusResult::from(
                            cid.to_owned(),
                            maybe_name.clone(),
                            &result,
                        );

                        match args.options.json_format {
                            true => serde_json::to_string(&status)
                                .expect("Serializing status result to json failed"),
                            false => build_output(&status)
                                .expect("Failed to build canister status output"),
                        }
                    }
                    Err(e) => {
                        let Some(reject) = direct_call_reject(&e) else {
                            bail!("Unknown error fetching canister {cid} status: {e}");
                        };

                        if reject.error_code.as_deref() == Some(E_CANISTER_NOT_FOUND) {
                            bail!("Canister {cid} was not found.");
                        }

                        if !reject
                            .error_code
                            .as_deref()
                            .is_some_and(|code| E_STATUS_ACCESS_DENIED.contains(&code))
                        {
                            bail!(
                                "Error looking up canister {cid}: {:?} - {}",
                                reject.error_code,
                                reject.reject_message
                            );
                        }

                        // Access was denied, so fall back on fetching the public status
                        let status =
                            build_public_status(&agent, cid.to_owned(), maybe_name.clone()).await?;

                        match args.options.json_format {
                            true => serde_json::to_string(&status)
                                .expect("Serializing status result to json failed"),
                            false => build_public_output(&status)
                                .expect("Failed to build canister status output"),
                        }
                    }
                }
            }
        };

        // Space records out to make things more readable
        if i > 0 && !args.options.json_format {
            println!();
        }
        println!("{}", output.trim());
    }

    Ok(())
}

#[derive(Serialize)]
struct PublicCanisterStatusResult {
    id: Principal,

    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    controllers: Vec<String>,
    module_hash: Option<String>,
}

/// Serializable wrapper for CanisterStatusResult that converts Nat fields to String
#[derive(Serialize)]
struct SerializableCanisterStatusResult {
    id: Principal,

    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,

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
    log_memory_limit: String,
    log_visibility: SerializableVisibility,
    snapshot_visibility: SerializableVisibility,
    status_visibility: SerializableVisibility,
    environment_variables: Vec<EnvironmentVariable>,
}

/// `--json` renders a visibility setting as `{"type": ..., "value": ...}`,
/// which differs from the manifest form [`Visibility`] serializes to.
#[derive(Clone)]
struct SerializableVisibility(Visibility);

#[derive(Serialize)]
#[serde(tag = "type", content = "value")]
enum VisibilityRepr {
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
    fn from(id: Principal, maybe_name: Option<String>, result: &CanisterStatusResult) -> Self {
        Self {
            id,
            name: maybe_name,
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
            log_memory_limit: settings.log_memory_limit.to_string(),
            log_visibility: SerializableVisibility(settings.log_visibility.clone().into()),
            snapshot_visibility: SerializableVisibility(
                settings.snapshot_visibility.clone().into(),
            ),
            status_visibility: SerializableVisibility(settings.status_visibility.clone().into()),
            environment_variables: settings.environment_variables.clone(),
        }
    }
}

impl Serialize for SerializableVisibility {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let repr = match &self.0 {
            Visibility::Controllers => VisibilityRepr::Controllers,
            Visibility::Public => VisibilityRepr::Public,
            Visibility::AllowedViewers(viewers) => {
                VisibilityRepr::AllowedViewers(viewers.iter().map(|p| p.to_string()).collect())
            }
        };
        repr.serialize(serializer)
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
    if let Some(name) = &result.name {
        writeln!(&mut buf, "Canister Name: {}", name)?;
    }
    writeln!(&mut buf, "Canister Status Report:")?;

    writeln!(
        &mut buf,
        "{}",
        format_controllers(result.controllers.iter().cloned(), "  ")
    )?;
    writeln!(
        &mut buf,
        "  Module hash: {}",
        result.module_hash.clone().unwrap_or("<none>".to_string())
    )?;

    Ok(buf)
}

fn build_output(result: &SerializableCanisterStatusResult) -> Result<String, anyhow::Error> {
    let mut buf = String::new();

    writeln!(&mut buf, "Canister Id: {}", result.id)?;
    if let Some(name) = &result.name {
        writeln!(&mut buf, "Canister Name: {}", name)?;
    }
    writeln!(&mut buf, "Canister Status Report:")?;
    writeln!(&mut buf, "  Status: {}", result.status)?;

    let settings = &result.settings;
    writeln!(
        &mut buf,
        "{}",
        format_controllers(settings.controllers.iter().cloned(), "  ")
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
    writeln!(
        &mut buf,
        "  Log memory limit: {}",
        settings.log_memory_limit
    )?;

    writeln!(
        &mut buf,
        "  Log visibility: {}",
        format_visibility(&settings.log_visibility.0, "log viewer", "  ")
    )?;
    writeln!(
        &mut buf,
        "  Snapshot visibility: {}",
        format_visibility(&settings.snapshot_visibility.0, "snapshot viewer", "  ")
    )?;
    writeln!(
        &mut buf,
        "  Status visibility: {}",
        format_visibility(&settings.status_visibility.0, "status viewer", "  ")
    )?;

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
        result.module_hash.clone().unwrap_or("<none>".to_string())
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

#[cfg(test)]
mod tests {
    use ic_agent::agent::{RejectCode, RejectResponse};

    use super::*;

    /// A denial arrives uncertified when ingress inspection catches it and
    /// certified when execution does, so the fallback has to see both. Anything
    /// that is not a reject must keep surfacing as an error.
    #[test]
    fn both_reject_forms_are_recognised() {
        let reject = || RejectResponse {
            reject_code: RejectCode::CanisterError,
            reject_message: "denied".to_string(),
            error_code: Some("IC0542".to_string()),
        };

        for source in [
            AgentError::CertifiedReject {
                reject: reject(),
                operation: None,
            },
            AgentError::UncertifiedReject {
                reject: reject(),
                operation: None,
            },
        ] {
            let err = UpdateOrProxyError::DirectUpdateCall { source };
            let found = direct_call_reject(&err).expect("reject should be extracted");
            assert!(E_STATUS_ACCESS_DENIED.contains(&found.error_code.as_deref().unwrap()));
        }

        assert!(
            direct_call_reject(&UpdateOrProxyError::ProxyCall {
                message: "boom".to_string(),
            })
            .is_none()
        );
    }

    /// `--json` renders visibility as a tagged `{"type", "value"}` object, which
    /// is a different shape from the manifest form `Visibility` serializes to,
    /// so it is pinned here rather than left to a derive.
    #[test]
    fn json_visibility_shape() {
        let json = |v: Visibility| serde_json::to_string(&SerializableVisibility(v)).unwrap();

        assert_eq!(json(Visibility::Controllers), r#"{"type":"Controllers"}"#);
        assert_eq!(json(Visibility::Public), r#"{"type":"Public"}"#);
        assert_eq!(
            json(Visibility::AllowedViewers(vec![
                Principal::from_text("aaaaa-aa").unwrap()
            ])),
            r#"{"type":"AllowedViewers","value":["aaaaa-aa"]}"#
        );
    }
}
