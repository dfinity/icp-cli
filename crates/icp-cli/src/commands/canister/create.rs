use std::collections::HashMap;

use anyhow::anyhow;
use candid::{Decode, Encode, Nat};
use clap::Args;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, AgentError, export::Principal};
use icp::{
    agent,
    context::{EnvironmentSelection, GetAgentForEnvError, GetEnvironmentError},
    identity, network,
    prelude::*,
};
use icp_canister_interfaces::{
    cycles_ledger::{
        CYCLES_LEDGER_PRINCIPAL, CanisterSettingsArg, CreateCanisterArgs, CreateCanisterResponse,
        CreationArgs, SubnetSelectionArg,
    },
    cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL,
    registry::{GetSubnetForCanisterRequest, GetSubnetForCanisterResult, REGISTRY_PRINCIPAL},
};
use rand::seq::IndexedRandom;

use icp::context::Context;

use crate::{
    options::{EnvironmentOpt, IdentityOpt},
    progress::{ProgressManager, ProgressManagerSettings},
};
use icp::store_id::{LookupIdError, RegisterError};

pub(crate) const DEFAULT_CANISTER_CYCLES: u128 = 2 * TRILLION;

#[derive(Clone, Debug, Default, Args)]
pub(crate) struct CanisterSettings {
    /// Optional compute allocation (0 to 100). Represents guaranteed compute capacity.
    #[arg(long)]
    pub(crate) compute_allocation: Option<u64>,

    /// Optional memory allocation in bytes. If unset, memory is allocated dynamically.
    #[arg(long)]
    pub(crate) memory_allocation: Option<u64>,

    /// Optional freezing threshold in seconds. Controls how long a canister can be inactive before being frozen.
    #[arg(long)]
    pub(crate) freezing_threshold: Option<u64>,

    /// Optional reserved cycles limit. If set, the canister cannot consume more than this many cycles.
    #[arg(long)]
    pub(crate) reserved_cycles_limit: Option<u64>,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct CreateArgs {
    /// The names of the canister within the current project
    pub(crate) names: Vec<String>,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

    /// One or more controllers for the canister. Repeat `--controller` to specify multiple.
    #[arg(long)]
    pub(crate) controller: Vec<Principal>,

    // Resource-related settings and thresholds for the new canister.
    #[command(flatten)]
    pub(crate) settings: CanisterSettings,

    /// Suppress human-readable output; print only canister IDs, one per line, to stdout.
    #[arg(long, short = 'q')]
    pub(crate) quiet: bool,

    /// Cycles to fund canister creation (in raw cycles).
    #[arg(long, default_value_t = DEFAULT_CANISTER_CYCLES)]
    pub(crate) cycles: u128,

    /// The subnet to create canisters on.
    #[arg(long)]
    pub(crate) subnet: Option<Principal>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateAgentError),

    #[error("project does not contain a canister named '{name}'")]
    CanisterNotFound { name: String },

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error("canister exists already: {principal}")]
    CanisterExists { principal: Principal },

    #[error(transparent)]
    CreateCanister(#[from] AgentError),

    #[error(transparent)]
    RegisterCanister(#[from] RegisterError),

    #[error(transparent)]
    Candid(#[from] candid::Error),

    #[error("{err}")]
    LedgerCreate { err: String },

    #[error("Failed to get subnet for canister: {err}")]
    GetSubnet { err: String },

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),

    #[error(transparent)]
    GetAgentForEnv(#[from] GetAgentForEnvError),

    #[error(transparent)]
    GetEnvironment(#[from] GetEnvironmentError),
}

// Creates canister(s) by asking the cycles ledger to create them.
// The cycles ledger will take cycles out of the user's account, and attaches them to a call to CMC::create_canister.
// The CMC will then pick a subnet according to the user's preferences and permissions, and create a canister on that subnet.
pub(crate) async fn exec(ctx: &Context, args: &CreateArgs) -> Result<(), CommandError> {
    let environment_selection: EnvironmentSelection = args.environment.clone().into();

    // Load project
    let p = ctx.project.load().await?;

    // Load target environment
    let env = ctx.get_environment(&environment_selection).await?;

    let target_canisters = match args.names.is_empty() {
        true => env.get_canister_names(),
        false => args.names.clone(),
    };

    for name in &target_canisters {
        if !p.canisters.contains_key(name) {
            return Err(CommandError::CanisterNotFound {
                name: name.to_owned(),
            });
        }

        if !env.canisters.contains_key(name) {
            return Err(CommandError::EnvironmentCanister {
                environment: env.name.to_owned(),
                canister: name.to_owned(),
            });
        }
    }

    let canister_infos = env
        .canisters
        .iter()
        .filter(|(k, _)| target_canisters.contains(k))
        .collect::<HashMap<_, _>>();

    // Do we have any already existing canisters?
    let cexist: Vec<_> = env
        .canisters
        .values()
        .filter_map(|(_, c)| ctx.ids.lookup(&env.name, &c.name).ok())
        .collect();

    // Agent
    let agent = ctx
        .get_agent_for_env(&args.identity.clone().into(), &environment_selection)
        .await?;

    // Select which subnet to deploy the canisters to
    //
    // If we don't specify a subnet, then the CMC will choose a random subnet
    // for each canister. Ideally, a project's canister should all live on the same subnet.
    let subnet = match args.subnet {
        // Target specified subnet
        Some(v) => v,

        // No subnet specified, and no canisters exist
        // Target a random subnet
        None if cexist.is_empty() => {
            let vs = get_available_subnets(&agent).await?;

            // Choose a random subnet
            vs.choose(&mut rand::rng())
                .expect("missing subnet id")
                .to_owned()
        }

        // No subnet specified, and some canisters exist
        // Target the same subnet as the first canister
        None => {
            get_canister_subnet(
                &agent,                                       // agent
                cexist.first().expect("missing canister id"), // id
            )
            .await?
        }
    };

    // Prepare a futures set for concurrent operations
    let mut futs = FuturesOrdered::new();

    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });

    let env_ref = &env;
    for (name, (_path, info)) in canister_infos.iter() {
        // Create progress bar with standard configuration
        let pb = progress_manager.create_progress_bar(name);

        // Create an async closure that handles the operation for this specific canister
        let create_fn = {
            let cmd = args.clone();
            let agent = agent.clone();
            let pb = pb.clone();

            async move {
                // Indicate to user that the canister is created
                pb.set_message("Creating...");

                match ctx.ids.lookup(&env_ref.name, name) {
                    // Exists (skip)
                    Ok(principal) => {
                        return Err(CommandError::CanisterExists { principal });
                    }

                    // Doesn't exist (include)
                    Err(LookupIdError::IdNotFound { .. }) => {}

                    // Lookup failed
                    Err(err) => panic!("{err}"),
                };

                // Build cycles ledger create_canister args
                let settings = CanisterSettingsArg {
                    freezing_threshold: cmd
                        .settings
                        .freezing_threshold
                        .or(info.settings.freezing_threshold)
                        .map(Nat::from),

                    controllers: if cmd.controller.is_empty() {
                        None
                    } else {
                        Some(cmd.controller.clone())
                    },

                    reserved_cycles_limit: cmd
                        .settings
                        .reserved_cycles_limit
                        .or(info.settings.reserved_cycles_limit)
                        .map(Nat::from),

                    memory_allocation: cmd
                        .settings
                        .memory_allocation
                        .or(info.settings.memory_allocation)
                        .map(Nat::from),

                    compute_allocation: cmd
                        .settings
                        .compute_allocation
                        .or(info.settings.compute_allocation)
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
                    .with_arg(Encode!(&arg)?)
                    .call_and_wait()
                    .await?;

                let resp: CreateCanisterResponse = Decode!(&resp, CreateCanisterResponse)?;

                let cid = match resp {
                    CreateCanisterResponse::Ok { canister_id, .. } => canister_id,
                    CreateCanisterResponse::Err(err) => {
                        return Err(CommandError::LedgerCreate {
                            err: err.format_error(cmd.cycles),
                        });
                    }
                };

                // Register the canister ID
                ctx.ids.register(&env_ref.name, name, cid)?;

                Ok::<_, CommandError>(())
            }
        };

        futs.push_back(async move {
            // Execute the create function with custom progress tracking
            let mut result = ProgressManager::execute_with_custom_progress(
                &pb,
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

async fn get_canister_subnet(agent: &Agent, id: &Principal) -> Result<Principal, anyhow::Error> {
    let args = &GetSubnetForCanisterRequest {
        principal: Some(*id),
    };

    let bs = agent
        .query(&REGISTRY_PRINCIPAL, "get_subnet_for_canister")
        .with_arg(Encode!(args)?)
        .call()
        .await
        .map_err(|err| CommandError::GetSubnet {
            err: err.to_string(),
        })?;

    let resp = Decode!(&bs, GetSubnetForCanisterResult)?;

    let out = resp
        .map_err(|err| CommandError::GetSubnet { err })?
        .subnet_id
        .ok_or(anyhow!("missing subnet id"))?;

    Ok(out)
}

async fn get_available_subnets(agent: &Agent) -> Result<Vec<Principal>, CommandError> {
    let bs = agent
        .query(&CYCLES_MINTING_CANISTER_PRINCIPAL, "get_default_subnets")
        .with_arg(Encode!(&())?)
        .call()
        .await
        .map_err(|err| CommandError::GetSubnet {
            err: err.to_string(),
        })?;

    let resp = Decode!(&bs, Vec<Principal>)?;

    // Check if any subnets are available
    if resp.is_empty() {
        return Err(anyhow!("no available subnets found").into());
    }

    Ok(resp)
}
