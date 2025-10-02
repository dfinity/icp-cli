use std::collections::{HashMap, HashSet};

use candid::{Decode, Encode, Nat};
use clap::Parser;
use futures::{StreamExt, future::try_join_all, stream::FuturesOrdered};
use ic_agent::{AgentError, export::Principal};
use icp::prelude::*;
use rand::seq::IndexedRandom;
use snafu::Snafu;

use crate::{
    context::{Context, ContextAgentError, ContextProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    progress::ProgressManager,
    store_id::{Key, RegisterError},
};
use icp_canister_interfaces::{
    cycles_ledger::{
        CYCLES_LEDGER_PRINCIPAL, CanisterSettingsArg, CreateCanisterArgs, CreateCanisterResponse,
        CreationArgs, SubnetSelectionArg,
    },
    cycles_minting_canister::{CYCLES_MINTING_CANISTER_PRINCIPAL, GetDefaultSubnetsResponse},
    registry::{GetSubnetForCanisterRequest, GetSubnetForCanisterResult, REGISTRY_PRINCIPAL},
};

pub const DEFAULT_CANISTER_CYCLES: u128 = 2 * TRILLION;

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
}

#[derive(Clone, Debug, Parser)]
pub struct Cmd {
    /// The names of the canister within the current project
    pub names: Vec<String>,

    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,

    /// One or more controllers for the canister. Repeat `--controller` to specify multiple.
    #[arg(long)]
    pub controller: Vec<Principal>,

    // Resource-related settings and thresholds for the new canister.
    #[command(flatten)]
    pub settings: CanisterSettings,

    /// Suppress human-readable output; print only canister IDs, one per line, to stdout.
    #[arg(long, short = 'q')]
    pub quiet: bool,

    /// Cycles to fund canister creation (in raw cycles).
    #[arg(long, default_value_t = DEFAULT_CANISTER_CYCLES)]
    pub cycles: u128,

    /// The subnet to create canisters on.
    #[arg(long)]
    pub subnet: Option<Principal>,
}

// Creates canister(s) by asking the cycles ledger to create them.
// The cycles ledger will take cycles out of the user's account, and attaches them to a call to CMC::create_canister.
// The CMC will then pick a subnet according to the user's preferences and permissions, and create a canister on that subnet.
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

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

    // Collect environment canisters
    let canisters_in_environment = env.canisters.clone().unwrap_or(
        pm.canisters
            .iter()
            .map(|(_, c)| c.name.to_owned())
            .collect(),
    );

    // Check if explicitly requested canisters are declared in the project AND environment
    for name in &cmd.names {
        pm.canisters.iter().find(|(_, c)| c.name == *name).ok_or(
            CommandError::CanisterNotFound {
                name: name.to_owned(),
            },
        )?;
        if !canisters_in_environment.contains(name) {
            return Err(CommandError::EnvironmentCanister {
                environment: env.name.to_owned(),
                canister: name.to_owned(),
            });
        }
    }

    // Determine canisters that shall exist at the end of the command
    let keys_to_exist = if cmd.names.is_empty() {
        canisters_in_environment
            .iter()
            .map(|name| Key {
                network: network.to_owned(),
                environment: env.name.to_owned(),
                canister: name.to_owned(),
            })
            .collect::<Vec<_>>()
    } else {
        cmd.names
            .clone()
            .into_iter()
            .filter(|name| canisters_in_environment.contains(name))
            .map(|name| Key {
                network: network.to_owned(),
                environment: env.name.to_owned(),
                canister: name.to_owned(),
            })
            .collect::<Vec<_>>()
    };

    let keys_in_environment = canisters_in_environment
        .iter()
        .map(|name| Key {
            network: network.to_owned(),
            environment: env.name.to_owned(),
            canister: name.to_owned(),
        })
        .collect::<Vec<_>>();

    let keys_to_create = keys_to_exist
        .into_iter()
        .filter(|key| ctx.id_store.lookup(key).is_err())
        .collect::<Vec<_>>();
    let existing_keys = keys_in_environment
        .into_iter()
        .filter(|key| ctx.id_store.lookup(key).is_ok())
        .collect::<Vec<_>>();

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

    // Subnet choice:
    // 1. If --subnet is provided, use it
    // 2. If there are existing canisters, if they are on the same subnet, use it, otherwise make the user choose explicitly
    // 3. If there are no existing canisters, pick an open subnet at random
    let subnet = if let Some(subnet) = cmd.subnet {
        subnet
    } else if !existing_keys.is_empty() {
        let mapping = try_join_all(existing_keys.iter().map(|key| async {
            let canister = ctx.id_store.lookup(key).expect("Canister already exists");
            let response_bytes = agent
                .query(&REGISTRY_PRINCIPAL, "get_subnet_for_canister")
                .with_arg(
                    Encode!(&GetSubnetForCanisterRequest {
                        principal: Some(canister),
                    })
                    .expect("Failed to encode GetSubnetForCanisterRequest"),
                )
                .call()
                .await
                .map_err(|source| CommandError::GetSubnet {
                    err: source.to_string(),
                })?;
            let response: GetSubnetForCanisterResult =
                Decode!(&response_bytes, GetSubnetForCanisterResult)
                    .expect("Failed to decode GetSubnetForCanisterResult");
            response
                .map(|success| {
                    (
                        key.canister.to_owned(),
                        success
                            .subnet_id
                            .expect("Canister exists, therefore it must be assigned to a subnet."),
                    )
                })
                .map_err(|err| CommandError::GetSubnet { err })
        }))
        .await?;
        if HashSet::<_>::from_iter(mapping.iter().map(|(_, subnet)| subnet)).len() == 1 {
            mapping.first().expect("existing_keys is not empty").1
        } else {
            return Err(CommandError::AmbiguousSubnet {
                mapping: mapping.into_iter().collect(),
            });
        }
    } else {
        let response_bytes = agent
            .query(&CYCLES_MINTING_CANISTER_PRINCIPAL, "get_default_subnets")
            .with_arg(Encode!(&()).expect("Failed to encode GetDefaultSubnetsRequest"))
            .call()
            .await
            .map_err(|err| CommandError::GetSubnet {
                err: err.to_string(),
            })?;
        let response: GetDefaultSubnetsResponse =
            Decode!(&response_bytes, GetDefaultSubnetsResponse)
                .expect("Failed to decode GetDefaultSubnetsResponse");
        *response
            .choose(&mut rand::rng())
            .expect("No default subnets")
    };

    // Prepare a futures set for concurrent operations
    let mut futs = FuturesOrdered::new();

    let progress_manager = ProgressManager::new();

    for key in keys_to_create {
        let (_, canister_declaration) = pm
            .canisters
            .iter()
            .find(|(_, c)| c.name == key.canister)
            .expect("Canister must be present in project");
        // Create progress bar with standard configuration
        let pb = progress_manager.create_progress_bar(&canister_declaration.name);

        // Create an async closure that handles the operation for this specific canister
        let create_fn = {
            let cmd = cmd.clone();
            let pb = pb.clone();

            async move {
                // Indicate to user that the canister is created
                pb.set_message("Creating...");

                // Build cycles ledger create_canister args
                let settings = CanisterSettingsArg {
                    freezing_threshold: cmd
                        .settings
                        .freezing_threshold
                        .or(canister_declaration.settings.freezing_threshold)
                        .map(Nat::from),
                    controllers: if cmd.controller.is_empty() {
                        None
                    } else {
                        Some(cmd.controller.clone())
                    },
                    reserved_cycles_limit: cmd
                        .settings
                        .reserved_cycles_limit
                        .or(canister_declaration.settings.reserved_cycles_limit)
                        .map(Nat::from),
                    memory_allocation: cmd
                        .settings
                        .memory_allocation
                        .or(canister_declaration.settings.memory_allocation)
                        .map(Nat::from),
                    compute_allocation: cmd
                        .settings
                        .compute_allocation
                        .or(canister_declaration.settings.compute_allocation)
                        .map(Nat::from),
                };

                let creation_args = CreationArgs {
                    subnet_selection: Some(SubnetSelectionArg::Subnet { subnet }),
                    settings: Some(settings),
                };

                let arg = CreateCanisterArgs {
                    from_subaccount: None,
                    created_at_time: None,
                    amount: Nat::from(cmd.cycles),
                    creation_args: Some(creation_args),
                };

                // Call cycles ledger create_canister
                let resp = agent
                    .update(&CYCLES_LEDGER_PRINCIPAL, "create_canister")
                    .with_arg(Encode!(&arg).map_err(|source| CommandError::Candid { source })?)
                    .call_and_wait()
                    .await
                    .map_err(|source| CommandError::CreateCanister { source })?;
                let resp: CreateCanisterResponse = Decode!(&resp, CreateCanisterResponse)
                    .map_err(|source| CommandError::Candid { source })?;
                let cid = match resp {
                    CreateCanisterResponse::Ok { canister_id, .. } => canister_id,
                    CreateCanisterResponse::Err(err) => {
                        return Err(CommandError::LedgerCreate {
                            err: err.format_error(cmd.cycles),
                        });
                    }
                };

                // Register the canister ID
                ctx.id_store.register(&key, &cid)?;

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

fn format_ambiguous_subnet(mappings: &HashMap<String, Principal>) -> String {
    let mut result = String::new();
    for (canister, subnet) in mappings {
        result.push_str(&format!("   {canister}: {subnet}\n"));
    }
    result
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

    #[snafu(transparent)]
    Candid { source: candid::Error },

    #[snafu(display("{err}"))]
    LedgerCreate { err: String },

    #[snafu(display("Failed to get subnet for canister: {err}"))]
    GetSubnet { err: String },

    #[snafu(display(
        "No obvious subnet choice. Use --subnet to manually pick a subnet. Current locations:\n{}",
        format_ambiguous_subnet(mapping)
    ))]
    AmbiguousSubnet { mapping: HashMap<String, Principal> },
}
