// This is a temporary placeholder command
// For now it's only used to set environment variables
// Eventually we will add support for canister settings operation

use std::collections::HashSet;

use clap::Parser;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::builders::EnvironmentVariable;
use snafu::Snafu;
use tracing::debug;

use crate::{
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    progress::ProgressManager,
    store_artifact::LookupError as LookupArtifactError,
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Clone, Debug, Parser)]
pub struct Cmd {
    /// The names of the canisters within the current project
    pub names: Vec<String>,

    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load project
    let pm = ctx.project()?;

    // Choose canisters to install
    let cs = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.names.is_empty() {
            // If no names specified, create all canisters
            true => true,

            // If names specified, only create matching canisters
            false => cmd.names.contains(&c.name),
        })
        .collect::<Vec<_>>();

    // Check if selected canisters exists
    if !cmd.names.is_empty() {
        let names = cs.iter().map(|(_, c)| &c.name).collect::<HashSet<_>>();

        for name in &cmd.names {
            if !names.contains(name) {
                return Err(CommandError::CanisterNotFound {
                    name: name.to_owned(),
                });
            }
        }
    }

    // Load target environment
    let env = pm
        .environments
        .iter()
        .find(|&v| v.name == cmd.environment.name())
        .ok_or(CommandError::EnvironmentNotFound {
            name: cmd.environment.name().to_owned(),
        })?;

    // Collect environment canisters
    let ecs = env.canisters.clone().unwrap_or(
        pm.canisters
            .iter()
            .map(|(_, c)| c.name.to_owned())
            .collect(),
    );

    // Filter for environment canisters
    let cs = cs
        .iter()
        .filter(|(_, c)| ecs.contains(&c.name))
        .collect::<Vec<_>>();

    // Ensure canister is included in the environment
    if !cmd.names.is_empty() {
        for name in &cmd.names {
            if !ecs.contains(name) {
                return Err(CommandError::EnvironmentCanister {
                    environment: env.name.to_owned(),
                    canister: name.to_owned(),
                });
            }
        }
    }

    // Ensure at least one canister has been selected
    if cs.is_empty() {
        return Err(CommandError::NoCanisters);
    }

    // Load identity
    ctx.require_identity(cmd.identity.name());

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

    // Setup network
    ctx.require_network(network);

    // Prepare agent
    let agent = ctx.agent()?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(agent);

    // Prepare a futures set for concurrent operations
    let mut futs = FuturesOrdered::new();

    let progress_manager = ProgressManager::new();

    // Get the list of name to canister id for this environment
    // We need this to inject the `ICP_CANISTER_ID:` environment variables
    // as we're installing the canisters
    let canister_list = ctx.id_store.lookup_by_environment(&env.name)?;

    debug!("Found canisters: {:?}", canister_list);
    let binding_vars = canister_list
        .iter()
        .map(|(n, p)| (format!("ICP_CANISTER_ID:{}", n), p.to_text()))
        .collect::<Vec<(_, _)>>();

    for (_, c) in cs {
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
                let cid = ctx.id_store.lookup(&Key {
                    network: network.to_owned(),
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

                debug!("Update environment variables with new canister bindings");
                mgmt.update_settings(&cid)
                    .with_environment_variables(environment_variables)
                    .await?;

                Ok::<_, CommandError>(())
            }
        };

        futs.push_back(async move {
            // Execute the install function with progress tracking
            ProgressManager::execute_with_progress(
                pb,
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

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("no canisters available to install"))]
    NoCanisters,

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    LookupCanisterArtifact { source: LookupArtifactError },

    #[snafu(transparent)]
    InstallAgent { source: AgentError },
}
