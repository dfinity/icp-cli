use std::io::stdout;

use anyhow::anyhow;
use candid::{Nat, Principal};
use clap::{ArgGroup, Args, Parser};
use icp::context::Context;
use icp::parsers::{CyclesAmount, DurationAmount, MemoryAmount};
use icp::{Canister, context::CanisterSelection, prelude::*};
use icp_canister_interfaces::management_canister::CanisterSettingsArg;
use serde::Serialize;
use tracing::info;

use crate::{
    commands::args,
    operations::create::{CreateOperation, CreateTarget},
};

pub(crate) const DEFAULT_CANISTER_CYCLES: u128 = 2 * TRILLION;

#[derive(Clone, Debug, Default, Args)]
pub(crate) struct CanisterSettings {
    /// Optional compute allocation (0 to 100). Represents guaranteed compute capacity.
    #[arg(long)]
    pub(crate) compute_allocation: Option<u64>,

    /// Optional memory allocation in bytes. If unset, memory is allocated dynamically.
    /// Supports suffixes: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb").
    #[arg(long)]
    pub(crate) memory_allocation: Option<MemoryAmount>,

    /// Optional freezing threshold. Controls how long a canister can be inactive before being frozen.
    /// Supports duration suffixes: s (seconds), m (minutes), h (hours), d (days), w (weeks).
    /// A bare number is treated as seconds.
    #[arg(long)]
    pub(crate) freezing_threshold: Option<DurationAmount>,

    /// Optional upper limit on cycles reserved for future resource payments.
    /// Memory allocations that would push the reserved balance above this limit will fail.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long)]
    pub(crate) reserved_cycles_limit: Option<CyclesAmount>,
}

/// Create a canister on a network.
#[derive(Debug, Parser)]
#[command(after_long_help = "\
This command can be used to create canisters defined in a project
or a \"detached\" canister on a network.

Examples:

    # Create on a network by url
    icp canister create -n http://localhost:8000 -k $ROOT_KEY --detached

    # Create on mainnet outside of a project context
    icp canister create -n ic --detached

    # Create a detached canister inside the scope of a project
    icp canister create -n mynetwork --detached
")]
#[command(group(
    ArgGroup::new("canister_sel")
        .args(["canister", "detached"])
        .required(true)
))]
pub(crate) struct CreateArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::OptionalCanisterCommandArgs,

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
    #[arg(long, default_value_t = CyclesAmount::from(DEFAULT_CANISTER_CYCLES))]
    pub(crate) cycles: CyclesAmount,

    /// The subnet to create canisters on.
    #[arg(long)]
    pub(crate) subnet: Option<Principal>,

    /// Create a canister detached from any project configuration. The canister id will be
    /// printed out but not recorded in the project configuration. Not valid if `Canister`
    /// is provided.
    #[arg(
        long,
        conflicts_with = "canister",
        required_unless_present = "canister"
    )]
    pub detached: bool,

    /// Output command results as JSON
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) json: bool,
}

impl CreateArgs {
    pub(crate) fn canister_settings_with_default(&self, default: &Canister) -> CanisterSettingsArg {
        CanisterSettingsArg {
            freezing_threshold: self
                .settings
                .freezing_threshold
                .clone()
                .or(default.settings.freezing_threshold.clone())
                .map(|d| Nat::from(d.get())),
            controllers: if self.controller.is_empty() {
                None
            } else {
                Some(self.controller.clone())
            },
            reserved_cycles_limit: self
                .settings
                .reserved_cycles_limit
                .clone()
                .or(default.settings.reserved_cycles_limit.clone())
                .map(|c| Nat::from(c.get())),
            // TODO This should be configurable from the CLI
            log_visibility: default.settings.log_visibility.clone().map(Into::into),
            memory_allocation: self
                .settings
                .memory_allocation
                .clone()
                .or(default.settings.memory_allocation.clone())
                .map(|m| Nat::from(m.get())),
            compute_allocation: self
                .settings
                .compute_allocation
                .or(default.settings.compute_allocation)
                .map(Nat::from),
        }
    }

    pub(crate) fn create_target(&self) -> CreateTarget {
        match self.subnet {
            Some(subnet) => CreateTarget::Subnet(subnet),
            None => CreateTarget::None,
        }
    }

    pub(crate) fn canister_settings(&self) -> CanisterSettingsArg {
        CanisterSettingsArg {
            freezing_threshold: self
                .settings
                .freezing_threshold
                .clone()
                .map(|d| Nat::from(d.get())),
            controllers: if self.controller.is_empty() {
                None
            } else {
                Some(self.controller.clone())
            },
            reserved_cycles_limit: self
                .settings
                .reserved_cycles_limit
                .clone()
                .map(|c| Nat::from(c.get())),
            // TODO This should be configurable from the CLI
            log_visibility: None,
            memory_allocation: self
                .settings
                .memory_allocation
                .clone()
                .map(|m| Nat::from(m.get())),
            compute_allocation: self.settings.compute_allocation.map(Nat::from),
        }
    }
}

// Creates canister(s) by asking the cycles ledger to create them.
// The cycles ledger will take cycles out of the user's account, and attaches them to a call to CMC::create_canister.
// The CMC will then pick a subnet according to the user's preferences and permissions, and create a canister on that subnet.
pub(crate) async fn exec(ctx: &Context, args: &CreateArgs) -> Result<(), anyhow::Error> {
    if args.detached {
        create_canister(ctx, args).await
    } else {
        create_project_canister(ctx, args).await
    }
}

// Attemtps to create a canister on the target network without recording it in the project metadata
async fn create_canister(ctx: &Context, args: &CreateArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();
    assert!(
        selections.canister.is_none(),
        "This path should not be called if canister is_some()"
    );

    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;

    let create_operation =
        CreateOperation::new(agent, args.create_target(), args.cycles.get(), vec![]);

    let canister_settings = args.canister_settings();

    let id = create_operation.create(&canister_settings).await?;

    if args.quiet {
        println!("{id}");
    } else if args.json {
        serde_json::to_writer(
            stdout(),
            &JsonCreate {
                canister_id: id,
                canister_name: None,
            },
        )?;
    } else {
        println!("Created canister with ID {id}");
    }

    Ok(())
}

// Attempts to create a canister and record it in the project metadata
async fn create_project_canister(ctx: &Context, args: &CreateArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();

    let canister = match selections
        .canister
        .expect("Canister must be Some() when --detached is not used")
    {
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
        info!("Canister {canister} already exists");
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

    let create_operation = CreateOperation::new(
        agent,
        args.create_target(),
        args.cycles.get(),
        existing_canisters,
    );

    let canister_settings = args.canister_settings_with_default(&canister_info);
    let id = create_operation.create(&canister_settings).await?;

    ctx.set_canister_id_for_env(&canister, id, &selections.environment)
        .await?;
    ctx.update_custom_domains(&selections.environment).await;

    if args.quiet {
        println!("{id}");
    } else if args.json {
        serde_json::to_writer(
            stdout(),
            &JsonCreate {
                canister_id: id,
                canister_name: Some(canister.clone()),
            },
        )?;
    } else {
        println!("Created canister {canister} with ID {id}");
    }

    Ok(())
}

#[derive(Serialize)]
struct JsonCreate {
    canister_id: Principal,
    canister_name: Option<String>,
}
