use anyhow::anyhow;
use clap::Args;
use futures::{StreamExt, future::try_join_all, stream::FuturesOrdered};
use ic_agent::export::Principal;
use icp::{
    context::{Context, EnvironmentSelection},
    identity::IdentitySelection,
};
use std::sync::Arc;

use crate::{
    commands::canister::create::{self},
    operations::{
        binding_env_vars::set_binding_env_vars_many,
        build::build_many_with_progress_bar,
        create::CreateOperation,
        install::{InstallOperationError, install_many},
        sync::{SyncOperationError, sync_many},
    },
    options::{EnvironmentOpt, IdentityOpt},
    progress::{ProgressManager, ProgressManagerSettings},
};

#[derive(Args, Debug)]
pub(crate) struct DeployArgs {
    /// Canister names
    pub(crate) names: Vec<String>,

    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub(crate) mode: String,

    /// The subnet to use for the canisters being deployed.
    #[clap(long)]
    pub(crate) subnet: Option<Principal>,

    /// One or more controllers for the canisters being deployed. Repeat `--controller` to specify multiple.
    #[arg(long)]
    pub(crate) controller: Vec<Principal>,

    /// Cycles to fund canister creation (in cycles).
    #[arg(long, default_value_t = create::DEFAULT_CANISTER_CYCLES)]
    pub(crate) cycles: u128,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error(transparent)]
    Create(#[from] create::CommandError),

    #[error(transparent)]
    InstallOperation(#[from] InstallOperationError),

    #[error(transparent)]
    SyncOperation(#[from] SyncOperationError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub(crate) async fn exec(ctx: &Context, args: &DeployArgs) -> Result<(), CommandError> {
    let environment_selection: EnvironmentSelection = args.environment.clone().into();
    let identity_selection: IdentitySelection = args.identity.clone().into();

    // Load the project manifest, which defines the canisters to be built.
    let p = ctx.project.load().await?;

    // Load target environment
    let env =
        p.environments
            .get(args.environment.name())
            .ok_or(CommandError::EnvironmentNotFound {
                name: args.environment.name().to_owned(),
            })?;

    let cnames = match args.names.is_empty() {
        // No canisters specified
        true => env.canisters.keys().cloned().collect(),

        // Individual canisters specified
        false => args.names.clone(),
    };

    for name in &cnames {
        ctx.assert_env_contains_canister(name, &environment_selection)
            .await
            .map_err(|e| anyhow!(e))?;
    }

    // Skip doing any work if no canisters are targeted
    if cnames.is_empty() {
        return Ok(());
    }

    // Build the selected canisters
    let _ = ctx.term.write_line("Building canisters:");
    let canisters_to_build = p
        .canisters
        .iter()
        .filter(|(k, _)| cnames.contains(k))
        .map(|(_, (path, canister))| (path.clone(), canister.clone()))
        .collect::<Vec<_>>();

    build_many_with_progress_bar(
        canisters_to_build,
        ctx.builder.clone(),
        ctx.artifacts.clone(),
        &ctx.term,
        ctx.debug,
    )
    .await?;

    // Create the selected canisters
    let _ = ctx.term.write_line("\n\nCreating canisters:");

    let env = ctx
        .get_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;
    let agent = ctx
        .get_agent_for_env(&identity_selection, &environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;
    let existing_canisters = ctx
        .ids_by_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;
    let canisters_to_create = cnames
        .iter()
        .filter(|name| !existing_canisters.contains_key(*name))
        .collect::<Vec<_>>();

    if canisters_to_create.is_empty() {
        let _ = ctx.term.write_line("All canisters already exist");
    } else {
        let create_operation = CreateOperation::new(
            agent.clone(),
            args.subnet,
            args.cycles,
            existing_canisters.into_values().collect(),
        );
        let mut futs = FuturesOrdered::new();
        let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });
        for name in canisters_to_create.iter() {
            let pb = progress_manager.create_progress_bar(name);
            pb.set_message("Creating...");
            let create_op = create_operation.clone();
            let (_, canister_info) = env.get_canister_info(name).map_err(|e| anyhow!(e))?;
            futs.push_back(async move {
                ProgressManager::execute_with_custom_progress(
                    &pb,
                    create_op.create(&canister_info.settings.into()),
                    || "Created successfully".to_string(),
                    |err: &_| err.to_string(),
                    |_| false,
                )
                .await
            });
        }

        // Cache errors until all futures are processed. Otherwise we risk dropping a canister id.
        let mut error: Option<anyhow::Error> = None;
        let mut idx = 0;
        while let Some(res) = futs.next().await {
            match res {
                Ok(id) => {
                    let canister_name = canisters_to_create
                        .get(idx)
                        .expect("should have tried to create every canister");
                    let _ = ctx
                        .term
                        .write_line(&format!("Created canister {canister_name} with ID {id}"));
                    ctx.set_canister_id_for_env(canister_name, id, &environment_selection)
                        .await
                        .map_err(|e| anyhow!(e))?;
                }
                Err(err) => {
                    error = Some(err.into());
                }
            }
            idx += 1;
        }
        if let Some(err) = error {
            return Err(err.into());
        }
    }

    let _ = ctx.term.write_line("\n\nSetting environment variables:");
    let env = ctx
        .get_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;

    let env_canisters = &env.canisters;
    let target_canisters = try_join_all(cnames.iter().map(|name| {
        let environment_selection = environment_selection.clone();
        async move {
            let cid = ctx
                .get_canister_id_for_env(name, &environment_selection)
                .await
                .map_err(|e| anyhow!(e))?;
            let (_, info) = env_canisters
                .get(name)
                .ok_or_else(|| anyhow!("Canister id exists but no canister info"))?;
            Ok::<_, anyhow::Error>((cid, info.clone()))
        }
    }))
    .await?;

    let canister_list = ctx
        .ids_by_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;

    set_binding_env_vars_many(
        agent.clone(),
        &env.name,
        target_canisters,
        canister_list,
        ctx.debug,
    )
    .await
    .map_err(|e| anyhow!(e))?;

    // Install the selected canisters
    let _ = ctx.term.write_line("\n\nInstalling canisters:");

    let canisters = try_join_all(cnames.iter().map(|name| {
        let environment_selection = environment_selection.clone();
        async move {
            let cid = ctx
                .get_canister_id_for_env(name, &environment_selection)
                .await
                .map_err(|e| anyhow!(e))?;
            Ok::<_, anyhow::Error>((name.clone(), cid))
        }
    }))
    .await?;

    install_many(
        agent.clone(),
        canisters,
        &args.mode,
        ctx.artifacts.clone(),
        ctx.debug,
    )
    .await?;

    // Sync the selected canisters
    let _ = ctx.term.write_line("\n\nSyncing canisters:");

    // Prepare list of canisters with their info for syncing
    let env = ctx
        .get_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;

    let env_canisters = &env.canisters;
    let sync_canisters = try_join_all(cnames.iter().map(|name| {
        let environment_selection = environment_selection.clone();
        async move {
            let cid = ctx
                .get_canister_id_for_env(name, &environment_selection)
                .await
                .map_err(|e| anyhow!(e))?;
            let (canister_path, info) = env_canisters
                .get(name)
                .ok_or_else(|| anyhow!("Canister id exists but no canister info"))?;
            Ok::<_, anyhow::Error>((cid, canister_path.clone(), info.clone()))
        }
    }))
    .await?;

    // Filter out canisters with no sync steps
    let sync_canisters: Vec<_> = sync_canisters
        .into_iter()
        .filter(|(_, _, info)| !info.sync.steps.is_empty())
        .collect();

    if !sync_canisters.is_empty() {
        sync_many(
            ctx.syncer.clone(),
            agent.clone(),
            Arc::new(ctx.term.clone()),
            sync_canisters,
            ctx.debug,
        )
        .await?;
    }

    Ok(())
}
