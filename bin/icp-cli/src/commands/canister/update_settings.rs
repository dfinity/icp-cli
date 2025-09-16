use clap::{ArgAction, Parser};
use ic_agent::{AgentError, export::Principal};
use ic_utils::interfaces::management_canister::CanisterStatusResult;
use snafu::Snafu;
use std::collections::HashSet;

use crate::context::{Context, ContextGetAgentError, GetProjectError};
use crate::options::{EnvironmentOpt, IdentityOpt};
use crate::store_id::{Key, LookupError as LookupIdError};

#[derive(Debug, Parser)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,

    #[arg(long, action = ArgAction::Append, conflicts_with = "set_controller")]
    add_controller: Option<Vec<Principal>>,

    #[arg(long, action = ArgAction::Append, conflicts_with = "set_controller")]
    remove_controller: Option<Vec<Principal>>,

    #[arg(long, action = ArgAction::Append)]
    set_controller: Option<Vec<Principal>>,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest.
    let pm = ctx.project()?;

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

    // Select canister to query
    let (_, c) = pm
        .canisters
        .iter()
        .find(|(_, c)| cmd.name == c.name)
        .ok_or(CommandError::CanisterNotFound { name: cmd.name })?;

    // Ensure canister is included in the environment
    if !ecs.contains(&c.name) {
        return Err(CommandError::EnvironmentCanister {
            environment: env.name.to_owned(),
            canister: c.name.to_owned(),
        });
    }

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

    // Lookup the canister id
    let cid = ctx.id_store.lookup(&Key {
        network: network.to_owned(),
        environment: env.name.to_owned(),
        canister: c.name.to_owned(),
    })?;

    // Load identity
    ctx.require_identity(cmd.identity.name());

    // Setup network
    ctx.require_network(network);

    // Prepare agent
    let agent = ctx.agent()?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(agent);

    let mut current_status: Option<CanisterStatusResult> = None;

    // Handle controllers.
    let mut controllers: Option<Vec<Principal>> = None;
    if let Some(to_be_set) = cmd.set_controller {
        controllers = Some(to_be_set);
    }
    if let Some(to_be_added) = cmd.add_controller {
        current_status = Some(mgmt.canister_status(&cid).await?.0);
        let current_controllers: Vec<Principal> = current_status
            .as_ref()
            .unwrap()
            .settings
            .controllers
            .clone();

        let new_controllers: Vec<Principal> = {
            let mut set: HashSet<Principal> = current_controllers.into_iter().collect();
            set.extend(to_be_added.into_iter());
            set.into_iter().collect()
        };

        // Only update controllers if there're new controllers to be added.
        if new_controllers.len() > current_status.as_ref().unwrap().settings.controllers.len() {
            controllers = Some(new_controllers);
        }
    }
    if let Some(to_be_removed) = cmd.remove_controller {
        if controllers.is_none() {
            if current_status.is_none() {
                current_status = Some(mgmt.canister_status(&cid).await?.0);
            }
            controllers = Some(
                current_status
                    .as_ref()
                    .unwrap()
                    .settings
                    .controllers
                    .clone(),
            );
        }

        let controllers = controllers.as_mut().unwrap();
        for removed in to_be_removed {
            if let Some(idx) = controllers.iter().position(|x| *x == removed) {
                controllers.swap_remove(idx);
            }
        }
    }

    // Update settings.
    let mut update = mgmt.update_settings(&cid);
    if let Some(controllers) = controllers {
        for controller in controllers {
            update = update.with_controller(controller);
        }
    }
    update.await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

    #[snafu(transparent)]
    Agent { source: AgentError },
}
