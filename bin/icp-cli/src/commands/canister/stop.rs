use crate::env::GetProjectError;
use crate::{env::Env, store_id::LookupError as LookupIdError};
use clap::Parser;
use ic_agent::{Agent, AgentError};
use icp_identity::key::LoadIdentityInContextError;
use icp_project::model::LoadProjectManifestError;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterStopCmd {
    /// The name of the canister within the current project
    pub name: String,

    /// The URL of the IC network endpoint
    #[clap(long, default_value = "http://127.0.0.1:8000")]
    pub network_url: String,
}

pub async fn exec(env: &Env, cmd: CanisterStopCmd) -> Result<(), CanisterStopError> {
    // Load the project manifest, which defines the canisters to be built.
    let pm = env.project()?;

    // Select canister to query
    let (_, c) = pm
        .canisters
        .iter()
        .find(|(_, c)| cmd.name == c.name)
        .ok_or(CanisterStopError::CanisterNotFound { name: cmd.name })?;

    // Lookup the canister id
    let cid = env.id_store.lookup(&c.name)?;

    // Load the currently selected identity
    let identity = env.load_identity()?;

    // Create an agent pointing to the desired network endpoint
    let agent = Agent::builder()
        .with_url(&cmd.network_url)
        .with_arc_identity(identity)
        .build()?;

    if cmd.network_url.contains("127.0.0.1") || cmd.network_url.contains("localhost") {
        agent.fetch_root_key().await?;
    }

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Instruct management canister to stop canister
    mgmt.stop_canister(&cid).await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterStopError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectNotFound,

    #[snafu(transparent)]
    ProjectLoad { source: LoadProjectManifestError },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    Agent { source: AgentError },
}
