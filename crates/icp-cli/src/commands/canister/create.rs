use anyhow::anyhow;
use candid::{Nat, Principal};
use clap::Args;
use icp::context::Context;
use icp::parsers::parse_cycles_amount;
use icp::{Canister, context::CanisterSelection, prelude::*};
use icp_canister_interfaces::cycles_ledger::CanisterSettingsArg;

use crate::{
    commands::args,
    operations::create::CreateOperation,
    progress::{ProgressManager, ProgressManagerSettings},
};

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

    /// Optional upper limit on cycles reserved for future resource payments.
    /// Memory allocations that would push the reserved balance above this limit will fail.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long, value_parser = parse_cycles_amount)]
    pub(crate) reserved_cycles_limit: Option<u128>,
}

/// Create a canister on a network
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

    /// Cycles to fund canister creation.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long, default_value_t = DEFAULT_CANISTER_CYCLES, value_parser = parse_cycles_amount)]
    pub(crate) cycles: u128,

    /// The subnet to create canisters on.
    #[arg(long)]
    pub(crate) subnet: Option<Principal>,
}

impl CreateArgs {
    pub(crate) fn canister_settings_with_default(&self, default: &Canister) -> CanisterSettingsArg {
        CanisterSettingsArg {
            freezing_threshold: self
                .settings
                .freezing_threshold
                .or(default.settings.freezing_threshold)
                .map(Nat::from),
            controllers: if self.controller.is_empty() {
                None
            } else {
                Some(self.controller.clone())
            },
            reserved_cycles_limit: self
                .settings
                .reserved_cycles_limit
                .or(default.settings.reserved_cycles_limit.map(u128::from))
                .map(Nat::from),
            log_visibility: default.settings.log_visibility.clone().map(Into::into),
            memory_allocation: self
                .settings
                .memory_allocation
                .or(default.settings.memory_allocation)
                .map(Nat::from),
            compute_allocation: self
                .settings
                .compute_allocation
                .or(default.settings.compute_allocation)
                .map(Nat::from),
        }
    }
}

// Creates canister(s) by asking the cycles ledger to create them.
// The cycles ledger will take cycles out of the user's account, and attaches them to a call to CMC::create_canister.
// The CMC will then pick a subnet according to the user's preferences and permissions, and create a canister on that subnet.
pub(crate) async fn exec(ctx: &Context, args: &CreateArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();
    let canister = match selections.canister {
        CanisterSelection::Named(name) => name,
        CanisterSelection::Principal(_) => Err(anyhow!("Cannot create a canister by principal"))?,
    };

    let env = ctx.get_environment(&selections.environment).await?;
    let (_, canister_info) = env.get_canister_info(&canister).map_err(|e| anyhow!(e))?;

    if ctx
        .get_canister_id_for_env(
            &icp::context::CanisterSelection::Named(canister.clone()),
            &selections.environment,
        )
        .await
        .is_ok()
    {
        let _ = ctx
            .term
            .write_line(&format!("Canister {canister} already exists"));
        return Ok(());
    }

    let agent = ctx
        .get_agent_for_env(&selections.identity, &selections.environment)
        .await?;
    let existing_canisters = ctx
        .ids_by_environment(&selections.environment)
        .await
        .map_err(|e| anyhow!(e))?
        .into_values()
        .collect();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });
    let create_operation =
        CreateOperation::new(agent, args.subnet, args.cycles, existing_canisters);

    let canister_settings = args.canister_settings_with_default(&canister_info);
    let pb = progress_manager.create_progress_bar(&canister);
    pb.set_message("Creating...");
    let id = ProgressManager::execute_with_custom_progress(
        &pb,
        create_operation.create(&canister_settings),
        || "Created successfully".to_string(),
        |err: &_| err.to_string(),
        |_| false,
    )
    .await?;

    ctx.set_canister_id_for_env(&canister, id, &selections.environment)
        .await?;

    let _ = ctx
        .term
        .write_line(&format!("Created canister {canister} with ID {id}"));

    Ok(())
}
