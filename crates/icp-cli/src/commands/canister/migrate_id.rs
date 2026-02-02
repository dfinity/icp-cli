use std::time::Duration;

use anyhow::bail;
use clap::Args;
use dialoguer::Confirm;
use ic_management_canister_types::CanisterStatusType;
use ic_utils::interfaces::ManagementCanister;
use icp::context::Context;
use icp_canister_interfaces::nns_migration::{MigrationStatus, NNS_MIGRATION_PRINCIPAL};
use indicatif::{ProgressBar, ProgressStyle};
use num_traits::ToPrimitive;

use crate::commands::args::{self, Canister};
use crate::operations::canister_migration::{
    get_subnet_for_canister, migrate_canister, migration_status,
};
use crate::operations::misc::format_timestamp;
use icp::context::CanisterSelection;

/// Minimum cycles required for migration (10T).
const MIN_CYCLES_FOR_MIGRATION: u128 = 10_000_000_000_000;
/// Cycles threshold for warning (15T).
const WARN_CYCLES_THRESHOLD: u128 = 15_000_000_000_000;

#[derive(Debug, Args)]
pub(crate) struct MigrateIdArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// The canister to replace with the source canister's ID
    #[arg(long)]
    replace: String,

    /// Skip confirmation prompts
    #[arg(long, short)]
    yes: bool,

    /// Resume watching an already-initiated migration (skips validation and initiation)
    #[arg(long)]
    resume_watch: bool,

    /// Exit as soon as the migrated canister is deleted (don't wait for full completion)
    #[arg(long)]
    skip_watch: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &MigrateIdArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();

    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;

    let source_cid = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;

    // Resolve target canister - parse as principal or name
    let target_canister: Canister = args.replace.as_str().into();
    let target_selection: CanisterSelection = target_canister.clone().into();
    let target_cid = ctx
        .get_canister_id(
            &target_selection,
            &selections.network,
            &selections.environment,
        )
        .await?;

    let source_name = args.cmd_args.canister.to_string();
    let target_name = target_canister.to_string();

    if source_cid == target_cid {
        bail!("The source and target canisters are identical");
    }

    // If --resume-watch is set, skip all validation and migration initiation
    if !args.resume_watch {
        if !args.yes {
            ctx.term.write_line(&format!(
                "This will migrate canister '{source_name}' ({source_cid}) to replace '{target_name}' ({target_cid})."
            ))?;
            ctx.term.write_line(
                "The target canister will be deleted and the source canister will take over its ID.",
            )?;

            let confirmed = Confirm::new()
                .with_prompt("Do you want to proceed?")
                .default(false)
                .interact()?;

            if !confirmed {
                bail!("Operation cancelled by user");
            }
        }

        let mgmt = ManagementCanister::create(&agent);

        // Fetch status of both canisters
        let (source_status,) = mgmt.canister_status(&source_cid).await?;
        let (target_status,) = mgmt.canister_status(&target_cid).await?;

        // Check both are stopped
        ensure_canister_stopped(source_status.status, &source_name)?;
        ensure_canister_stopped(target_status.status, &target_name)?;

        // Check source canister is ready for migration
        if !source_status.ready_for_migration {
            bail!(
                "Canister '{source_name}' is not ready for migration. Wait a few seconds and try again"
            );
        }

        // Check cycles balance
        let cycles = source_status
            .cycles
            .0
            .to_u128()
            .expect("unable to parse cycles");

        if cycles < MIN_CYCLES_FOR_MIGRATION {
            bail!(
                "Canister '{source_name}' has less than 10T cycles ({cycles} cycles). Top up before migrating"
            );
        }

        if !args.yes && cycles > WARN_CYCLES_THRESHOLD {
            ctx.term.write_line(&format!(
                "Warning: Canister '{source_name}' has more than 15T cycles ({cycles} cycles)."
            ))?;
            ctx.term
                .write_line("The extra cycles will get burned during the migration.")?;

            let confirmed = Confirm::new()
                .with_prompt("Do you want to proceed?")
                .default(false)
                .interact()?;

            if !confirmed {
                bail!("Operation cancelled by user");
            }
        }

        // Check target canister has no snapshots
        let (snapshots,) = mgmt.list_canister_snapshots(&target_cid).await?;
        if !snapshots.is_empty() {
            bail!(
                "The target canister '{target_name}' ({target_cid}) has {} snapshot(s). \
                 Delete them before migration with `icp canister snapshot delete`",
                snapshots.len()
            );
        }

        // Check canisters are on different subnets
        let source_subnet = get_subnet_for_canister(&agent, source_cid).await?;
        let target_subnet = get_subnet_for_canister(&agent, target_cid).await?;

        if source_subnet == target_subnet {
            bail!(
                "The canisters '{source_name}' and '{target_name}' are on the same subnet ({source_subnet}). \
                 Canister ID migration requires canisters on different subnets"
            );
        }

        ctx.term.write_line(&format!(
            "Migrating canister '{source_name}' ({source_cid}) to replace '{target_name}' ({target_cid})"
        ))?;
        ctx.term
            .write_line(&format!("  Source subnet: {source_subnet}"))?;
        ctx.term
            .write_line(&format!("  Target subnet: {target_subnet}"))?;

        // Add NNS migration canister as controller to both canisters if not already
        let source_controllers = source_status.settings.controllers;
        if !source_controllers.contains(&NNS_MIGRATION_PRINCIPAL) {
            ctx.term.write_line(&format!(
                "Adding NNS migration canister as controller of '{source_name}'..."
            ))?;
            let mut new_controllers = source_controllers;
            new_controllers.push(NNS_MIGRATION_PRINCIPAL);
            let mut builder = mgmt.update_settings(&source_cid);
            for controller in new_controllers {
                builder = builder.with_controller(controller);
            }
            builder.await?;
        }

        let target_controllers = target_status.settings.controllers;
        if !target_controllers.contains(&NNS_MIGRATION_PRINCIPAL) {
            ctx.term.write_line(&format!(
                "Adding NNS migration canister as controller of '{target_name}'..."
            ))?;
            let mut new_controllers = target_controllers;
            new_controllers.push(NNS_MIGRATION_PRINCIPAL);
            let mut builder = mgmt.update_settings(&target_cid);
            for controller in new_controllers {
                builder = builder.with_controller(controller);
            }
            builder.await?;
        }

        // Initiate migration
        ctx.term.write_line("Initiating canister ID migration...")?;
        migrate_canister(&agent, source_cid, target_cid).await?;
    } else {
        ctx.term.write_line(&format!(
            "Resuming watch for migration of '{source_name}' ({source_cid}) to '{target_name}' ({target_cid})"
        ))?;
    }

    // Create spinner for polling
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::with_template("{spinner:.blue} {msg}")
            .expect("invalid style template")
            .tick_strings(&["✶", "✸", "✹", "✺", "✹", "✷", "✔"]),
    );
    spinner.enable_steady_tick(Duration::from_millis(120));
    spinner.set_message("Waiting for migration to complete...");

    // Poll for completion
    loop {
        match migration_status(&agent, source_cid, target_cid).await {
            Ok(Some(MigrationStatus::InProgress { status })) => {
                spinner.set_message(format!("Migration in progress: {status}"));

                // If --skip-watch is set and we've reached MigratedCanisterDeleted, exit early
                if args.skip_watch && status == "MigratedCanisterDeleted" {
                    spinner.finish_with_message(format!(
                        "Migration in progress: {status} (exiting early due to --skip-watch)"
                    ));
                    ctx.term.write_line(&format!(
                        "The source canister '{source_name}' has been deleted. Migration will continue in the background."
                    ))?;
                    ctx.term.write_line(&format!(
                        "Use `icp canister migrate-id {source_name} --replace {target_name} --resume-watch` to monitor completion."
                    ))?;
                    return Ok(());
                }
            }
            Ok(Some(MigrationStatus::Succeeded { time })) => {
                spinner.finish_with_message(format!(
                    "Migration succeeded at {}",
                    format_timestamp(time)
                ));
                break;
            }
            Ok(Some(MigrationStatus::Failed { reason, time })) => {
                spinner.finish_with_message(format!(
                    "Migration failed at {}: {}",
                    format_timestamp(time),
                    reason
                ));
                bail!("Migration failed at {}: {}", format_timestamp(time), reason);
            }
            Ok(None) => {
                // No status yet, keep polling
            }
            Err(e) => {
                spinner.set_message(format!("Warning: Could not fetch status: {e}"));
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    ctx.term.write_line(&format!(
        "Canister '{source_name}' ({source_cid}) has been successfully migrated to the new subnet, replacing {target_cid}"
    ))?;

    Ok(())
}

fn ensure_canister_stopped(status: CanisterStatusType, name: &str) -> Result<(), anyhow::Error> {
    match status {
        CanisterStatusType::Stopped => Ok(()),
        CanisterStatusType::Running => {
            bail!("Canister '{name}' is running. Run `icp canister stop {name}` first")
        }
        CanisterStatusType::Stopping => {
            bail!("Canister '{name}' is stopping. Wait a few seconds and try again")
        }
    }
}
