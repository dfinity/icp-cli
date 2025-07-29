use crate::{
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError},
};
use clap::Parser;
use icp_adapter::sync::{Adapter, AdapterSyncError};
use icp_canister::model::SyncStep;
use snafu::Snafu;

#[derive(Parser, Debug)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: Option<String>,

    #[clap(flatten)]
    pub identity: IdentityOpt,

    #[clap(flatten)]
    pub environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be synced.
    let pm = ctx.project()?;

    // Choose canisters to sync
    let cs = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.name {
            Some(name) => name == &c.name,
            None => true,
        })
        .cloned()
        .collect::<Vec<_>>();

    // Check if selected canister exists
    if let Some(name) = &cmd.name {
        if cs.is_empty() {
            return Err(CommandError::CanisterNotFound {
                name: name.to_owned(),
            });
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
    if let Some(name) = &cmd.name {
        if !ecs.contains(name) {
            return Err(CommandError::EnvironmentCanister {
                environment: env.name.to_owned(),
                canister: name.to_owned(),
            });
        }
    }

    // Verify at least one canister is available to sync
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

    // Iterate through each resolved canister and trigger its sync process.
    for (canister_path, c) in cs {
        // Get canister principal ID
        let cid = ctx.id_store.lookup(&Key {
            network: network.to_owned(),
            environment: env.name.to_owned(),
            canister: c.name.to_owned(),
        })?;

        for step in &c.sync.steps {
            match step {
                // Synchronize the canister using the custom script adapter.
                SyncStep::Script(adapter) => adapter.sync(canister_path, &cid, agent).await?,

                // Synchronize the canister using the assets adapter.
                SyncStep::Assets(adapter) => adapter.sync(canister_path, &cid, agent).await?,
            };
        }
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(display("no canisters available to sync"))]
    NoCanisters,

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

    #[snafu(transparent)]
    IdLookup { source: LookupError },

    #[snafu(transparent)]
    SyncAdapter { source: AdapterSyncError },
}
