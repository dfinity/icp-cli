use std::collections::HashMap;

use clap::Args;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::builders::CanisterInstallMode;
use icp::{agent, identity, network};
use tracing::debug;

use crate::{
    commands::Context,
    options::{EnvironmentOpt, IdentityOpt},
    progress::{ProgressManager, ProgressManagerSettings},
    store_artifact::LookupError as LookupArtifactError,
    store_id::{Key, LookupError as LookupIdError},
};

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

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("project does not contain a canister named '{name}'")]
    CanisterNotFound { name: String },

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error("no canisters available to install")]
    NoCanisters,

    #[error(transparent)]
    LookupCanisterId(#[from] LookupIdError),

    #[error(transparent)]
    LookupCanisterArtifact(#[from] LookupArtifactError),

    #[error(transparent)]
    InstallAgent(#[from] AgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &InstallArgs) -> Result<(), CommandError> {
    // Load the project
    let p = ctx.project.load().await?;

    // Load identity
    let id = ctx.identity.load(args.identity.clone().into()).await?;

    // Load target environment
    let env =
        p.environments
            .get(args.environment.name())
            .ok_or(CommandError::EnvironmentNotFound {
                name: args.environment.name().to_owned(),
            })?;

    // Access network
    let access = ctx.network.access(&env.network).await?;

    // Agent
    let agent = ctx.agent.create(id, &access.url).await?;

    if let Some(k) = access.root_key {
        agent.set_root_key(k);
    }

    let cnames = match args.names.is_empty() {
        // No canisters specified
        true => env.canisters.keys().cloned().collect(),

        // Individual canisters specified
        false => args.names.clone(),
    };

    for name in &cnames {
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

    let cs = env
        .canisters
        .iter()
        .filter(|(k, _)| cnames.contains(k))
        .collect::<HashMap<_, _>>();

    // Ensure at least one canister has been selected
    if cs.is_empty() {
        return Err(CommandError::NoCanisters);
    }

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Prepare a futures set for concurrent operations
    let mut futs = FuturesOrdered::new();

    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });

    for (_, (_, c)) in cs {
        // Create progress bar with standard configuration
        let pb = progress_manager.create_progress_bar(&c.name);

        // Create an async closure that handles the operation for this specific canister
        let install_fn = {
            let cmd = args.clone();
            let mgmt = mgmt.clone();
            let pb = pb.clone();

            async move {
                // Indicate to user that the canister is being installed
                pb.set_message("Installing...");

                // Lookup the canister id
                let cid = ctx.ids.lookup(&Key {
                    network: env.network.name.to_owned(),
                    environment: env.name.to_owned(),
                    canister: c.name.to_owned(),
                })?;

                // Lookup the canister build artifact
                let wasm = ctx.artifacts.lookup(&c.name)?;

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
