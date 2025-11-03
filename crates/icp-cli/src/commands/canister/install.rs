use clap::Args;
use futures::{StreamExt, future::try_join_all, stream::FuturesOrdered};
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::builders::CanisterInstallMode;
use icp::{
    agent,
    context::{GetAgentForEnvError, GetCanisterIdForEnvError, GetEnvironmentError},
    identity, network,
};
use tracing::debug;

use icp::context::Context;

use crate::{
    options::{EnvironmentOpt, IdentityOpt},
    progress::{ProgressManager, ProgressManagerSettings},
};
use icp::store_artifact::LookupArtifactError;
use icp::store_id::LookupIdError;

#[derive(Clone, Debug, Args)]
pub(crate) struct InstallArgs {
    /// The names of the canisters within the current project
    pub(crate) names: Vec<String>,

    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub(crate) mode: String,

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
    Identity(#[from] identity::LoadError),

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error(transparent)]
    LookupCanisterId(#[from] LookupIdError),

    #[error(transparent)]
    LookupCanisterArtifact(#[from] LookupArtifactError),

    #[error(transparent)]
    InstallAgent(#[from] AgentError),

    #[error(transparent)]
    GetEnvironment(#[from] GetEnvironmentError),

    #[error(transparent)]
    GetAgentForEnv(#[from] GetAgentForEnvError),

    #[error(transparent)]
    GetCanisterIdForEnv(#[from] GetCanisterIdForEnvError),
}

pub(crate) async fn exec(ctx: &Context, args: &InstallArgs) -> Result<(), CommandError> {
    // Load target environment
    let env = ctx.get_environment(args.environment.name()).await?;

    // Agent
    let agent = ctx
        .get_agent_for_env(&args.identity.clone().into(), args.environment.name())
        .await?;

    let target_canisters = match args.names.is_empty() {
        true => env.get_canister_names(),
        false => args.names.clone(),
    };

    let canisters = try_join_all(target_canisters.into_iter().map(|name| {
        let env_name = args.environment.name();
        async move {
            let cid = ctx.get_canister_id_for_env(&name, env_name).await?;
            Ok::<_, CommandError>((name, cid))
        }
    }))
    .await?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Prepare a futures set for concurrent operations
    let mut futs = FuturesOrdered::new();

    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });

    for (name, cid) in canisters {
        // Create progress bar with standard configuration
        let pb = progress_manager.create_progress_bar(&name);

        // Create an async closure that handles the operation for this specific canister
        let install_fn = {
            let cmd = args.clone();
            let mgmt = mgmt.clone();
            let pb = pb.clone();

            async move {
                // Indicate to user that the canister is being installed
                pb.set_message("Installing...");

                // Lookup the canister build artifact
                let wasm = ctx.artifacts.lookup(&name).await?;

                // Retrieve canister status
                let (status,) = mgmt.canister_status(&cid).await?;

                let install_mode = match cmd.mode.as_ref() {
                    // Auto
                    "auto" => match status.module_hash {
                        // Canister has had code installed to it.
                        Some(_) => CanisterInstallMode::Upgrade(None),

                        // Canister has not had code installed to it.
                        None => CanisterInstallMode::Install,
                    },

                    // Install
                    "install" => CanisterInstallMode::Install,

                    // Reinstall
                    "reinstall" => CanisterInstallMode::Reinstall,

                    // Upgrade
                    "upgrade" => CanisterInstallMode::Upgrade(None),

                    // invalid
                    _ => panic!("invalid install mode"),
                };

                // Install code to canister
                debug!("Install new canister code");
                mgmt.install_code(&cid, &wasm)
                    .with_mode(install_mode)
                    .await?;

                Ok::<_, CommandError>(())
            }
        };

        futs.push_back(async move {
            // Execute the install function with progress tracking
            ProgressManager::execute_with_progress(
                &pb,
                install_fn,
                || "Installed successfully".to_string(),
                |err| format!("Failed to install canister: {err}"),
            )
            .await
        });
    }

    // Consume the set of futures and abort if an error occurs
    while let Some(res) = futs.next().await {
        // TODO(or.ricon): Handle canister creation failures
        res?;
    }

    Ok(())
}
