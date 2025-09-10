use clap::Parser;
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::CanisterStatusResult;
use itertools::Itertools;
use snafu::Snafu;

use crate::{
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Parser)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let pm = ctx.project()?;

    // Select canister to query
    let (_, c) = pm
        .canisters
        .iter()
        .find(|(_, c)| cmd.name == c.name)
        .ok_or(CommandError::CanisterNotFound { name: cmd.name })?;

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

    // Retrieve canister status from management canister
    let (result,) = mgmt.canister_status(&cid).await?;

    // Info printout
    print_info(&result);

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

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

    #[snafu(transparent)]
    Agent { source: AgentError },
}

pub fn print_info(result: &CanisterStatusResult) {
    let controllers: Vec<String> = result
        .settings
        .controllers
        .iter()
        .map(|p| p.to_string())
        .sorted()
        .collect();

    eprintln!("Controllers: {}", controllers.join(", "));

    match &result.module_hash {
        Some(hash) => {
            let hex_string: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
            eprintln!("Module hash: 0x{}", hex_string);
        }
        None => eprintln!("Module hash: <none>"),
    }
}
