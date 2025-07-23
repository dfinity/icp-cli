use crate::context::{ContextGetAgentError, GetProjectError};
use crate::options::{EnvironmentOpt, IdentityOpt};
use crate::{context::Context, store_id::LookupError as LookupIdError};
use clap::Parser;
use ic_agent::AgentError;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterDeleteCmd {
    /// The name of the canister within the current project
    pub name: String,

    #[clap(flatten)]
    identity: IdentityOpt,

    #[clap(flatten)]
    environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: CanisterDeleteCmd) -> Result<(), CanisterDeleteError> {
    // Load the project manifest, which defines the canisters to be built.
    let pm = ctx.project()?;

    // Load target environment
    let env = pm
        .environments
        .iter()
        .find(|&v| v.name == cmd.environment.name())
        .ok_or(CanisterDeleteError::EnvironmentNotFound {
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
        .ok_or(CanisterDeleteError::CanisterNotFound { name: cmd.name })?;

    // Ensure canister is included in the environment
    if !ecs.contains(&c.name) {
        return Err(CanisterDeleteError::EnvironmentCanister {
            environment: env.name.to_owned(),
            canister: c.name.to_owned(),
        });
    }

    // Lookup the canister id
    let cid = ctx.id_store.lookup(&c.name)?;

    // Load identity
    ctx.require_identity(cmd.identity.name());

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    ctx.require_network(
        env.network
            .as_ref()
            .expect("no network specified in environment"),
    );

    // Prepare agent
    let agent = ctx.agent()?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(agent);

    // Instruct management canister to delete canister
    mgmt.delete_canister(&cid).await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterDeleteError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

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
    Agent { source: AgentError },
}
