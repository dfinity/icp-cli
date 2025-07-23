use crate::context::{EnvGetAgentError, GetProjectError};
use crate::options::{IdentityOpt, NetworkOpt};
use crate::{context::Context, store_id::LookupError as LookupIdError};
use clap::Parser;
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::StatusCallResult;
use itertools::Itertools;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterInfoCmd {
    /// The name of the canister within the current project
    pub name: String,

    #[clap(flatten)]
    identity: IdentityOpt,

    #[clap(flatten)]
    network: NetworkOpt,
}

pub async fn exec(ctx: &Context, cmd: CanisterInfoCmd) -> Result<(), CanisterInfoError> {
    ctx.require_identity(cmd.identity.name());
    ctx.require_network(cmd.network.name());

    // Load the project manifest, which defines the canisters to be built.
    let pm = ctx.project()?;

    // Select canister to query
    let (_, c) = pm
        .canisters
        .iter()
        .find(|(_, c)| cmd.name == c.name)
        .ok_or(CanisterInfoError::CanisterNotFound { name: cmd.name })?;

    // Lookup the canister id
    let cid = ctx.id_store.lookup(&c.name)?;

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
pub enum CanisterInfoError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(transparent)]
    EnvGetAgent { source: EnvGetAgentError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    Agent { source: AgentError },
}

pub fn print_info(result: &StatusCallResult) {
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
