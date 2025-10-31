// This is a temporary placeholder command
// For now it's only used to set environment variables
// Eventually we will add support for canister settings operation

use std::collections::{HashMap, HashSet};

use clap::Args;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::builders::EnvironmentVariable;
use icp::{agent, identity, network};
use tracing::debug;

use crate::{
    commands::Context,
    options::{EnvironmentOpt, IdentityOpt},
    progress::{ProgressManager, ProgressManagerSettings},
    store_artifact::LookupArtifactError,
    store_id::{Key, LookupIdError},
};

#[derive(Clone, Debug, Args)]
pub(crate) struct BindingArgs {
    /// The names of the canisters within the current project
    pub(crate) names: Vec<String>,

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

    #[error("no canisters available to install")]
    NoCanisters,

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error("Could not find canister id(s) for '{}' in environment '{environment}' make sure they are created first", canister_names.join(", "))]
    CanisterNotCreated {
        environment: String,
        canister_names: Vec<String>,
    },

    #[error(transparent)]
    LookupId(#[from] LookupIdError),

    #[error(transparent)]
    LookupArtifact(#[from] LookupArtifactError),

    #[error(transparent)]
    InstallAgent(#[from] AgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &BindingArgs) -> Result<(), CommandError> {
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

    // Get the list of name to canister id for this environment
    // We need this to inject the `PUBLIC_CANISTER_ID:` environment variables
    // as we're installing the canisters
    let canister_list = ctx.ids.lookup_by_environment(&env.name)?;
    debug!("Canister list: {:?}", canister_list);

    // Check that all the canisters in this environment have an id
    // We need to have all the ids to generate environment variables
    // for the bindings
    let canisters_with_ids: HashSet<&String> = canister_list.iter().map(|(n, _p)| n).collect();
    debug!("Canisters with ids: {:?}", canisters_with_ids);

    let missing_canisters: Vec<String> = env
        .canisters
        .iter()
        .map(|(_, (_, c))| &c.name)
        .filter(|c| !canisters_with_ids.contains(c))
        .map(|c| c.to_string())
        .collect();

    debug!("missing canisters: {:?}", missing_canisters);

    if !missing_canisters.is_empty() {
        return Err(CommandError::CanisterNotCreated {
            environment: env.name.to_owned(),
            canister_names: missing_canisters,
        });
    }

    debug!("Found canisters: {:?}", canister_list);
    let binding_vars = canister_list
        .iter()
        .map(|(n, p)| (format!("PUBLIC_CANISTER_ID:{n}"), p.to_text()))
        .collect::<Vec<(_, _)>>();

    for (_, (_, c)) in cs {
        // Create progress bar with standard configuration
        let pb = progress_manager.create_progress_bar(&c.name);

        // Create an async closure that handles the operation for this specific canister
        let settings_fn = {
            let mgmt = mgmt.clone();
            let pb = pb.clone();
            let binding_vars = binding_vars.clone();

            async move {
                // Indicate to user that the canister's environment variables are being set
                pb.set_message("Updating environment variables...");

                // Lookup the canister id
                let cid = ctx.ids.lookup(&Key {
                    network: env.network.name.to_owned(),
                    environment: env.name.to_owned(),
                    canister: c.name.to_owned(),
                })?;

                // Load the variables from the config files
                let mut environment_variables = c
                    .settings
                    .environment_variables
                    .to_owned()
                    .unwrap_or_default();

                // inject the ids of the other canisters
                for (k, v) in binding_vars.iter() {
                    environment_variables.insert(k.to_string(), v.to_string());
                }

                // Convert from HashMap<String, String> to Vec<EnvironmentVariable>
                // as required by the IC management canister interface
                let environment_variables = environment_variables
                    .into_iter()
                    .map(|(name, value)| EnvironmentVariable { name, value })
                    .collect::<Vec<_>>();

                debug!(
                    "Update environment variables with new canister bindings: {:?}",
                    environment_variables
                );
                mgmt.update_settings(&cid)
                    .with_environment_variables(environment_variables)
                    .await?;

                Ok::<_, CommandError>(())
            }
        };

        futs.push_back(async move {
            // Execute the install function with progress tracking
            ProgressManager::execute_with_progress(
                &pb,
                settings_fn,
                || "Environment variables updated successfully".to_string(),
                |err| format!("Failed to update environment variables: {err}"),
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
