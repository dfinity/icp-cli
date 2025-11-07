use anyhow::anyhow;
use clap::Args;
use ic_agent::{AgentError, export::Principal};
use icp::{
    agent,
    context::{CanisterSelection, GetAgentForEnvError, GetEnvironmentError},
    identity::{self},
    network,
    prelude::*,
};

use icp::context::Context;

use crate::{
    commands::args,
    operations::create::CreateOperation,
    progress::{ProgressManager, ProgressManagerSettings},
};
use icp::store_id::RegisterError;

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

#[derive(Debug, Args)]
pub(crate) struct CreateArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

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

    #[error(transparent)]
    CreateCanister(#[from] AgentError),

    #[error(transparent)]
    RegisterCanister(#[from] RegisterError),

    #[error(transparent)]
    Candid(#[from] candid::Error),

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
    let selections = args.cmd_args.selections();
    let canister = match selections.canister {
        CanisterSelection::Named(name) => name,
        CanisterSelection::Principal(_) => Err(anyhow!("Cannot create a canister by principal"))?,
    };

    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });
    let create_operation = CreateOperation::new(
        ctx,
        &selections.environment,
        &selections.identity,
        args.subnet,
        args.controller.clone(),
        args.cycles,
        args.settings.clone(),
    );

    let pb = progress_manager.create_progress_bar(&canister);
    match ProgressManager::execute_with_custom_progress(
        &pb,
        create_operation.create(&canister, &pb),
        || "Created successfully".to_string(),
        |err| err.to_string(),
        |_| false,
    )
    .await?
    {
        (canister, Some(id)) => {
            let _ = ctx
                .term
                .write_line(&format!("Created canister {canister} with ID {id}"));
        }
        (canister, None) => {
            let _ = ctx
                .term
                .write_line(&format!("Canister {canister} already exists"));
        }
    };

    Ok(())
}
