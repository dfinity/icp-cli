use std::collections::HashSet;

use clap::Parser;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{AgentError, export::Principal};
use ic_utils::interfaces::management_canister::{LogVisibility, builders::EnvironmentVariable};
use snafu::Snafu;

use crate::{
    context::{Context, ContextAgentError, ContextProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    progress::ProgressManager,
    store_id::{Key, LookupError, RegisterError},
};

/// This CID is dependent on the toplogy being served by pocket-ic
/// NOTE: If the topology is changed (another subnet is added, etc) the CID may change.
/// References:
/// - http://localhost:8000/_/topology
/// - http://localhost:8000/_/dashboard
pub const DEFAULT_EFFECTIVE_ID: &str = "tqzl2-p7777-77776-aaaaa-cai";

#[derive(Clone, Debug, Parser)]
pub struct CanisterIDs {
    /// The effective canister ID to use when calling the management canister.
    #[arg(long, default_value = DEFAULT_EFFECTIVE_ID)]
    pub effective_id: Principal,

    /// The specific canister ID to assign if creating with a fixed principal.
    #[arg(long)]
    pub specific_id: Option<Principal>,
}

#[derive(Clone, Debug, Default, Parser)]
pub struct CanisterSettings {
    /// Optional compute allocation (0 to 100). Represents guaranteed compute capacity.
    #[arg(long)]
    pub compute_allocation: Option<u64>,

    /// Optional memory allocation in bytes. If unset, memory is allocated dynamically.
    #[arg(long)]
    pub memory_allocation: Option<u64>,

    /// Optional freezing threshold in seconds. Controls how long a canister can be inactive before being frozen.
    #[arg(long)]
    pub freezing_threshold: Option<u64>,

    /// Optional reserved cycles limit. If set, the canister cannot consume more than this many cycles.
    #[arg(long)]
    pub reserved_cycles_limit: Option<u64>,

    /// Optional Wasm memory limit in bytes. Sets an upper bound for Wasm heap growth.
    #[arg(long)]
    pub wasm_memory_limit: Option<u64>,

    /// Optional Wasm memory threshold in bytes. Triggers a callback when exceeded.
    #[arg(long)]
    pub wasm_memory_threshold: Option<u64>,
}

#[derive(Clone, Debug, Parser)]
pub struct Cmd {
    /// The names of the canister within the current project
    pub names: Vec<String>,

    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,

    // Canister ID configuration, including the effective and optionally specific ID.
    #[command(flatten)]
    pub ids: CanisterIDs,

    /// One or more controllers for the canister. Repeat `--controller` to specify multiple.
    #[arg(long)]
    pub controller: Vec<Principal>,

    // Resource-related settings and thresholds for the new canister.
    #[command(flatten)]
    pub settings: CanisterSettings,

    /// Suppress human-readable output; print only canister IDs, one per line, to stdout.
    #[arg(long, short = 'q')]
    pub quiet: bool,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load project
    let pm = ctx.project()?;

    // Load target environment
    let env = pm
        .environments
        .iter()
        .find(|&v| v.name == cmd.environment.name())
        .ok_or(CommandError::EnvironmentNotFound {
            name: cmd.environment.name().to_owned(),
        })?;

    // Choose canisters to create
    let cs = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.names.is_empty() {
            // If no names specified, create all canisters
            true => true,

            // If names specified, only create matching canisters
            false => cmd.names.contains(&c.name),
        })
        .collect::<Vec<_>>();

    // Check if selected canisters exists
    if !cmd.names.is_empty() {
        let names = cs.iter().map(|(_, c)| &c.name).collect::<HashSet<_>>();

        for name in &cmd.names {
            if !names.contains(name) {
                return Err(CommandError::CanisterNotFound {
                    name: name.to_owned(),
                });
            }
        }
    }

    // Collect environment canisters
    let ecs = env.canisters.clone().unwrap_or(
        pm.canisters
            .iter()
            .map(|(_, c)| c.name.to_owned())
            .collect(),
    );

    // Filter for environment canisters
    let cs = cs
        .iter()
        .filter(|(_, c)| ecs.contains(&c.name))
        .collect::<Vec<_>>();

    // Ensure canister is included in the environment
    if !cmd.names.is_empty() {
        for name in &cmd.names {
            if !ecs.contains(name) {
                return Err(CommandError::EnvironmentCanister {
                    environment: env.name.to_owned(),
                    canister: name.to_owned(),
                });
            }
        }
    }

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

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

    // Prepare a futures set for concurrent operations
    let mut futs = FuturesOrdered::new();

    let progress_manager = ProgressManager::new();

    for (_, c) in cs {
        // Create progress bar with standard configuration
        let pb = progress_manager.create_progress_bar(&c.name);

        // Create an async closure that handles the operation for this specific canister
        let create_fn = {
            let cmd = cmd.clone();
            let mgmt = mgmt.clone();
            let pb = pb.clone();

            async move {
                // Indicate to user that the canister is created
                pb.set_message("Creating...");

                // Create canister-network association-key
                let k = Key {
                    network: network.to_owned(),
                    environment: env.name.to_owned(),
                    canister: c.name.to_owned(),
                };

                match ctx.id_store.lookup(&k) {
                    // Exists (skip)
                    Ok(principal) => {
                        return Err(CommandError::CanisterExists { principal });
                    }

                    // Doesn't exist (include)
                    Err(LookupError::IdNotFound { .. }) => {}

                    // Lookup failed
                    Err(err) => panic!("{err}"),
                };

                // Create canister
                let mut builder = mgmt.create_canister();

                // Cycles amount
                builder = builder.as_provisional_create_with_amount(None);

                // Canister ID (effective)
                builder = builder.with_effective_canister_id(cmd.ids.effective_id);

                // Configure canister environment variables
                // Environment variables are key-value pairs that are accessible within the canister
                // and can be used to configure behavior without hardcoding values in the WASM
                builder = {
                    let environment_variables = c
                        .settings
                        .environment_variables
                        .to_owned()
                        .unwrap_or_default();

                    // Convert from HashMap<String, String> to Vec<EnvironmentVariable>
                    // as required by the IC management canister interface
                    let environment_variables = environment_variables
                        .into_iter()
                        .map(|(name, value)| EnvironmentVariable { name, value })
                        .collect::<Vec<_>>();

                    builder.with_environment_variables(environment_variables)
                };

                // Logs
                builder = builder.with_optional_log_visibility(Some(LogVisibility::Public));

                // Canister ID (specific)
                if let Some(id) = cmd.ids.specific_id {
                    builder = builder.as_provisional_create_with_specified_id(id);
                }

                // Controllers
                for c in &cmd.controller {
                    builder = builder.with_controller(c.to_owned());
                }

                // Compute
                builder = builder.with_optional_compute_allocation(
                    cmd.settings
                        .compute_allocation
                        .or(c.settings.compute_allocation),
                );

                // Memory
                builder = builder.with_optional_memory_allocation(
                    cmd.settings
                        .memory_allocation
                        .or(c.settings.memory_allocation),
                );

                // Freezing Threshold
                builder = builder.with_optional_freezing_threshold(
                    cmd.settings
                        .freezing_threshold
                        .or(c.settings.freezing_threshold),
                );

                // Reserved Cycles (limit)
                builder = builder.with_optional_reserved_cycles_limit(
                    cmd.settings
                        .reserved_cycles_limit
                        .or(c.settings.reserved_cycles_limit),
                );

                // Wasm (memory limit)
                builder = builder.with_optional_wasm_memory_limit(
                    cmd.settings
                        .wasm_memory_limit
                        .or(c.settings.wasm_memory_limit),
                );

                // Wasm (memory threshold)
                builder = builder.with_optional_wasm_memory_threshold(
                    cmd.settings
                        .wasm_memory_threshold
                        .or(c.settings.wasm_memory_threshold),
                );

                // Logs
                builder = builder.with_optional_log_visibility(
                    Some(LogVisibility::Public), //
                );

                // Create the canister
                let (cid,) = builder.await?;

                // Register the canister ID
                ctx.id_store.register(&k, &cid)?;

                Ok::<_, CommandError>(())
            }
        };

        futs.push_back(async move {
            // Execute the create function with custom progress tracking
            let mut result = ProgressManager::execute_with_custom_progress(
                pb,
                create_fn,
                || "Created successfully".to_string(),
                |err| match err {
                    CommandError::CanisterExists { principal } => {
                        format!("Canister already created: {principal}")
                    }
                    _ => format!("Failed to create canister: {err}"),
                },
                |err| matches!(err, CommandError::CanisterExists { .. }),
            )
            .await;

            // If canister already exists, it is not considered an error
            if let Err(CommandError::CanisterExists { .. }) = result {
                result = Ok(());
            }

            result
        });
    }

    // Consume the set of futures and abort if an error occurs
    while let Some(res) = futs.next().await {
        // TODO(or.ricon): Handle canister creation failures
        res?;
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextAgentError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(display("no canisters available to create"))]
    NoCanisters,

    #[snafu(display("canister exists already: {principal}"))]
    CanisterExists { principal: Principal },

    #[snafu(transparent)]
    CreateCanister { source: AgentError },

    #[snafu(transparent)]
    RegisterCanister { source: RegisterError },
}
