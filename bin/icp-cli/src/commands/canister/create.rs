use crate::env::Env;
use clap::Parser;
use ic_agent::{Agent, AgentError, export::Principal};
use ic_utils::interfaces::management_canister::LogVisibility;
use icp_identity::key::LoadIdentityInContextError;
use icp_project::{
    directory::{FindProjectError, ProjectDirectory},
    model::{LoadProjectManifestError, ProjectManifest},
};
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterCreateCmd {
    /// The name of the canister within the current project
    name: Option<String>,

    /// The URL of the IC network endpoint
    #[clap(long, default_value = "http://127.0.0.1:8000")]
    network_url: String,
}

pub async fn exec(env: &Env, cmd: CanisterCreateCmd) -> Result<(), CanisterCreateError> {
    // Find the current ICP project directory.
    let pd = ProjectDirectory::find()?.ok_or(CanisterCreateError::ProjectNotFound)?;

    // Get the project directory structure for path resolution.
    let pds = pd.structure();

    // Load the project manifest, which defines the canisters to be built.
    let pm = ProjectManifest::load(pds)?;

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

    // Choose canisters to create
    let cs = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.name {
            Some(name) => name == &c.name,
            None => true,
        })
        .collect::<Vec<_>>();

    // Case 1 (canister not found)
    if let Some(name) = cmd.name {
        if cs.is_empty() {
            return Err(CanisterCreateError::CanisterNotFound { name });
        }
    }

    // Case 2 (no canisters)
    if cs.is_empty() {
        return Err(CanisterCreateError::NoCanisters);
    }

    for (_, c) in cs {
        // Create canister
        let mut builder = mgmt.create_canister();

        // Cycles amount
        builder = builder.as_provisional_create_with_amount(None);

        // Canister ID (effective)
        builder = builder.with_effective_canister_id(
            Principal::from_text("ghsi2-tqaaa-aaaan-aaaca-cai").unwrap(),
        );

        // Canister ID (specific)
        builder = builder.as_provisional_create_with_specified_id(
            Principal::from_text("ghsi2-tqaaa-aaaan-aaaca-cai").unwrap(),
        );

        // Controllers
        for c in [] {
            builder = builder.with_controller(
                Principal::from_text::<&str>(c).unwrap(), // controller
            );
        }

        // Compute
        builder = builder.with_optional_compute_allocation(
            Some(0), // best-effort basis
        );

        // Memory
        builder = builder.with_optional_memory_allocation(
            Some(0), // best-effort basis
        );

        // Freezing Threshold
        builder = builder.with_optional_freezing_threshold(
            Some(u64::MAX), //
        );

        // Reserved Cycles (limit)
        builder = builder.with_optional_reserved_cycles_limit(
            Some(0), // disable reservation mechanism
        );

        // Wasm (memory limit)
        builder = builder.with_optional_wasm_memory_limit(
            Some(0), //
        );

        // Wasm (memory threshold)
        builder = builder.with_optional_wasm_memory_threshold(
            Some(0), //
        );

        // Logs
        builder = builder.with_optional_log_visibility(
            Some(LogVisibility::Public), //
        );

        // Create the canister
        let cid = builder.await?;

        // TODO(or.ricon): Associate created canister ID with
        println!("Created canister '{}' with ID: '{}'", c.name, cid.0);
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterCreateError {
    #[snafu(transparent)]
    FindProjectError { source: FindProjectError },

    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectNotFound,

    #[snafu(transparent)]
    ProjectLoad { source: LoadProjectManifestError },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("no canisters available to build"))]
    NoCanisters,

    #[snafu(transparent)]
    BuildAgent { source: AgentError },
}
