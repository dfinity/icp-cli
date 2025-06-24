use crate::env::Env;
use crate::store_id::{LookupError, RegisterError};
use clap::Parser;
use ic_agent::{Agent, AgentError, export::Principal};
use ic_utils::interfaces::management_canister::LogVisibility;
use icp_identity::key::LoadIdentityInContextError;
use icp_project::{
    directory::{FindProjectError, ProjectDirectory},
    model::{LoadProjectManifestError, ProjectManifest},
};
use snafu::Snafu;

const DEFAULT_EFFECTIVE_ID: &str = "uqqxf-5h777-77774-qaaaa-cai";

#[derive(Debug, Parser)]
pub struct CanisterIDs {
    /// The effective canister ID to use when calling the management canister.
    #[clap(long, default_value = DEFAULT_EFFECTIVE_ID)]
    pub effective_id: Principal,

    /// The specific canister ID to assign if creating with a fixed principal.
    #[clap(long)]
    pub specific_id: Option<Principal>,
}

#[derive(Debug, Default, Parser)]
pub struct CanisterOptions {
    /// Optional compute allocation (0 to 100). Represents guaranteed compute capacity.
    #[clap(long)]
    pub compute_allocation: Option<u64>,

    /// Optional memory allocation in bytes. If unset, memory is allocated dynamically.
    #[clap(long)]
    pub memory_allocation: Option<u64>,

    /// Optional freezing threshold in seconds. Controls how long a canister can be inactive before being frozen.
    #[clap(long)]
    pub freezing_threshold: Option<u64>,

    /// Optional reserved cycles limit. If set, the canister cannot consume more than this many cycles.
    #[clap(long)]
    pub reserved_cycles_limit: Option<u64>,

    /// Optional Wasm memory limit in bytes. Sets an upper bound for Wasm heap growth.
    #[clap(long)]
    pub wasm_memory_limit: Option<u64>,

    /// Optional Wasm memory threshold in bytes. Triggers a callback when exceeded.
    #[clap(long)]
    pub wasm_memory_threshold: Option<u64>,
}

#[derive(Debug, Parser)]
pub struct CanisterCreateCmd {
    /// The name of the canister within the current project
    pub name: Option<String>,

    /// The URL of the IC network endpoint
    #[clap(long, default_value = "http://localhost:8000")]
    pub network_url: String,

    // Canister ID configuration, including the effective and optionally specific ID.
    #[clap(flatten)]
    pub ids: CanisterIDs,

    /// One or more controllers for the canister. Repeat `--controller` to specify multiple.
    #[clap(long)]
    pub controller: Vec<Principal>,

    // Resource-related options and thresholds for the new canister.
    #[clap(flatten)]
    pub options: CanisterOptions,

    /// Suppress human-readable output; print only canister IDs, one per line, to stdout.
    #[clap(long, short = 'q')]
    pub quiet: bool,
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

    // TODO(or.ricon): This is to be replaced with a centralized agent or agent-builder
    if cmd.network_url.contains("127.0.0.1") || cmd.network_url.contains("localhost") {
        agent.fetch_root_key().await?;
    }

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Choose canisters to create
    let canisters = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.name {
            Some(name) => name == &c.name,
            None => true,
        })
        .collect::<Vec<_>>();

    // Case 1 (canister not found)
    if let Some(name) = cmd.name {
        if canisters.is_empty() {
            return Err(CanisterCreateError::CanisterNotFound { name });
        }
    }

    // Case 2 (skip created canisters)
    let canisters = canisters
        .into_iter()
        .filter(|&(_, c)| {
            match env.id_store.lookup(&c.name) {
                // Exists (skip)
                Ok(_) => false,

                // Doesn't exist (include)
                Err(LookupError::IdNotFound { .. }) => true,

                // Lookup failed
                Err(err) => panic!("{err}"),
            }
        })
        .collect::<Vec<_>>();

    // Case 3 (no canisters)
    if canisters.is_empty() {
        return Err(CanisterCreateError::NoCanisters);
    }

    for (_, c) in canisters {
        // Create canister
        let mut builder = mgmt.create_canister();

        // Cycles amount
        builder = builder.as_provisional_create_with_amount(None);

        // Canister ID (effective)
        builder = builder.with_effective_canister_id(cmd.ids.effective_id);

        // Canister ID (specific)
        if let Some(id) = cmd.ids.specific_id {
            builder = builder.as_provisional_create_with_specified_id(id);
        }

        // Controllers
        for c in &cmd.controller {
            builder = builder.with_controller(c.to_owned());
        }

        // Compute
        builder = builder.with_optional_compute_allocation(
            cmd.options
                .compute_allocation
                .or(c.deploy.options.compute_allocation),
        );

        // Memory
        builder = builder.with_optional_memory_allocation(
            cmd.options
                .memory_allocation
                .or(c.deploy.options.memory_allocation),
        );

        // Freezing Threshold
        builder = builder.with_optional_freezing_threshold(
            cmd.options
                .freezing_threshold
                .or(c.deploy.options.freezing_threshold),
        );

        // Reserved Cycles (limit)
        builder = builder.with_optional_reserved_cycles_limit(
            cmd.options
                .reserved_cycles_limit
                .or(c.deploy.options.reserved_cycles_limit),
        );

        // Wasm (memory limit)
        builder = builder.with_optional_wasm_memory_limit(
            cmd.options
                .wasm_memory_limit
                .or(c.deploy.options.wasm_memory_limit),
        );

        // Wasm (memory threshold)
        builder = builder.with_optional_wasm_memory_threshold(
            cmd.options
                .wasm_memory_threshold
                .or(c.deploy.options.wasm_memory_threshold),
        );

        // Logs
        builder = builder.with_optional_log_visibility(
            Some(LogVisibility::Public), //
        );

        // Create the canister
        let (cid,) = builder.await?;

        // Register the canister ID
        env.id_store.register(&c.name, &cid)?;

        if cmd.quiet {
            println!("{}", cid);
        } else {
            eprintln!("Created canister '{}' with ID: '{}'", c.name, cid);
        }
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

    #[snafu(display("no canisters available to create"))]
    NoCanisters,

    #[snafu(transparent)]
    BuildAgent { source: AgentError },

    #[snafu(transparent)]
    RegisterCanister { source: RegisterError },
}
