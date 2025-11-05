use clap::Args;
use ic_agent::export::Principal;

use icp::{
    EnvironmentEnsureCanisterDeclaredError, ProjectEnsureCanisterDeclaredError,
    context::{Context, GetEnvironmentError},
};

use crate::{
    commands::{
        build,
        canister::{
            binding_env_vars,
            create::{self, CanisterSettings},
            install,
        },
        sync,
    },
    operations::create::create_canisters,
    options::{EnvironmentOpt, IdentityOpt},
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

    #[error(transparent)]
    Build(#[from] build::CommandError),

    #[error(transparent)]
    Create(#[from] create::CommandError),

    #[error("failed to create canisters: {0}")]
    CreateOperation(anyhow::Error),

    #[error(transparent)]
    Install(#[from] install::CommandError),

    #[error(transparent)]
    SetEnvironmentVariables(#[from] binding_env_vars::CommandError),

    #[error(transparent)]
    Sync(#[from] sync::CommandError),

    #[error(transparent)]
    GetEnvironment(#[from] GetEnvironmentError),

    #[error(transparent)]
    EnvironmentEnsureCanisterDeclared(#[from] EnvironmentEnsureCanisterDeclaredError),

    #[error(transparent)]
    ProjectEnsureCanisterDeclared(#[from] ProjectEnsureCanisterDeclaredError),
}

pub(crate) async fn exec(ctx: &Context, args: &DeployArgs) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let p = ctx.project.load().await?;

    // Load target environment
    let env = ctx
        .get_environment(&args.environment.clone().into())
        .await?;

    let cnames = match args.names.is_empty() {
        // No canisters specified
        true => env.canisters.keys().cloned().collect(),

        // Individual canisters specified
        false => args.names.clone(),
    };

    for name in &cnames {
        p.ensure_canister_declared(name)?;
        env.ensure_canister_declared(name)?;
    }

    // Skip doing any work if no canisters are targeted
    if cnames.is_empty() {
        return Ok(());
    }

    // Build the selected canisters
    let _ = ctx.term.write_line("Building canisters:");
    build::exec(
        ctx,
        &build::BuildArgs {
            names: cnames.to_owned(),
        },
    )
    .await?;

    // Create the selected canisters
    let _ = ctx.term.write_line("\n\nCreating canisters:");
    let canister_names: Vec<&str> = cnames.iter().map(|s| s.as_str()).collect();
    create_canisters(
        canister_names,
        ctx,
        &args.environment.clone().into(),
        &args.identity.clone().into(),
        args.subnet,
        args.controller.to_owned(),
        CanisterSettings {
            ..Default::default()
        },
        args.cycles,
    )
    .await
    .map_err(CommandError::CreateOperation)?;

    let _ = ctx.term.write_line("\n\nSetting environment variables:");
    binding_env_vars::exec(
        ctx,
        &binding_env_vars::BindingArgs {
            names: cnames.to_owned(),
            identity: args.identity.clone(),
            environment: args.environment.clone(),
        },
    )
    .await?;

    // Install the selected canisters
    let _ = ctx.term.write_line("\n\nInstalling canisters:");
    install::exec(
        ctx,
        &install::InstallArgs {
            names: cnames.to_owned(),
            mode: args.mode.to_owned(),
            identity: args.identity.clone(),
            environment: args.environment.clone(),
        },
    )
    .await?;

    // Sync the selected canisters
    let _ = ctx.term.write_line("\n\nSyncing canisters:");
    let out = sync::exec(
        ctx,
        &sync::SyncArgs {
            names: cnames.to_owned(),
            identity: args.identity.clone(),
            environment: args.environment.clone(),
        },
    )
    .await;

    if let Err(err) = out
        && !matches!(err, sync::CommandError::NoCanisters)
    {
        return Err(err.into());
    }

    Ok(())
}
