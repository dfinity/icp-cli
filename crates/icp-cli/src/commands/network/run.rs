use std::{
    process::{Child, Command},
    time::Duration,
};

use clap::Parser;
use ic_agent::{Agent, AgentError};
use icp::{
    identity::manifest::{LoadIdentityManifestError, load_identity_list},
    manifest,
    network::{
        Configuration, NetworkDirectory, RunNetworkError, config::NetworkDescriptorModel,
        run_network,
    },
};

use crate::commands::Context;

const BACKGROUND_ENV_VAR: &str = "ICP_CLI_RUN_NETWORK_BACKGROUND";

/// Run a given network
#[derive(Parser, Debug)]
pub struct Cmd {
    /// Name of the network to run
    #[arg(default_value = "local")]
    name: String,

    /// Starts the network in a background process. This command will exit once the network is running.
    #[arg(long)]
    background: bool,

    /// Set if this is the process that runs the network in the background
    #[arg(long, env = BACKGROUND_ENV_VAR, hide = true)]
    run_in_background: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Locate(#[from] manifest::LocateError),

    #[error(transparent)]
    Agent(#[from] AgentError),

    #[error("project does not contain a network named '{name}'")]
    Network { name: String },

    #[error("network '{name}' must be a managed network")]
    Unmanaged { name: String },

    #[error("timed out waiting for network to start: {err}")]
    Timeout { err: String },

    #[error(transparent)]
    Identities(#[from] LoadIdentityManifestError),

    #[error(transparent)]
    RunNetwork(#[from] RunNetworkError),

    #[error(transparent)]
    SavePid(#[from] icp::network::SavePidError),
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await?;

    // Obtain network configuration
    let network = p.networks.get(&cmd.name).ok_or(CommandError::Network {
        name: cmd.name.to_owned(),
    })?;

    let cfg = match &network.configuration {
        // Locally-managed network
        Configuration::Managed(cfg) => cfg,

        // Non-managed networks cannot be started
        Configuration::Connected(_) => {
            return Err(CommandError::Unmanaged {
                name: cmd.name.to_owned(),
            });
        }
    };

    // Project root
    let pdir = ctx.workspace.locate()?;

    // Network root
    let ndir = pdir.join(".icp").join("networks").join(&network.name);

    // Network directory
    let nd = NetworkDirectory::new(
        &network.name,               // name
        &ndir,                       // network_root
        &ctx.dirs.port_descriptor(), // port_descriptor_dir
    );

    // Identities
    let ids = load_identity_list(&ctx.dirs.identity())?;

    // Determine ICP accounts to seed
    let seed_accounts = ids.identities.values().map(|id| id.principal());

    eprintln!("Project root: {pdir}");
    eprintln!("Network root: {ndir}");

    if cmd.background {
        let child = run_in_background()?;
        nd.save_background_network_runner_pid(child.id())?;
        wait_for_healthy_network(&nd).await?;
    } else {
        run_network(
            cfg,           // config
            nd,            // nd
            &pdir,         // project_root
            seed_accounts, // seed_accounts
        )
        .await?;
    }
    Ok(())
}

fn run_in_background() -> Result<Child, CommandError> {
    // Background strategy is different; we spawn `dfx` with the same arguments
    // (minus --background), ping and exit.
    let exe = std::env::current_exe().expect("Failed to get current executable.");
    let mut cmd = Command::new(exe);
    // Skip 1 because arg0 is this executable's path.
    cmd.args(std::env::args().skip(1).filter(|a| !a.eq("--background")))
        .env(BACKGROUND_ENV_VAR, "true"); // Set the environment variable which will be used by the second start.
    let child = cmd.spawn().expect("Failed to spawn child process.");
    Ok(child)
}

async fn retry_with_timeout<F, Fut, T>(mut f: F, max_retries: usize, delay_ms: u64) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>> + Send,
{
    let mut retries = 0;
    loop {
        if let Some(result) = f().await {
            return Some(result);
        }
        if retries > max_retries {
            return None;
        }
        retries += 1;
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }
}

async fn wait_for_healthy_network(nd: &NetworkDirectory) -> Result<(), CommandError> {
    let network = wait_for_network_descriptor(nd).await?;
    let port = network.gateway.port;
    let agent = Agent::builder()
        .with_url(format!("http://127.0.0.1:{port}"))
        .build()?;

    let max_retries = 30;
    let delay_ms = 1000;
    let result: Option<()> = retry_with_timeout(
        || {
            let agent = agent.clone();
            async move {
                let status = agent.status().await;
                if let Ok(status) = status {
                    if matches!(&status.replica_health_status, Some(status) if status == "healthy")
                    {
                        return Some(());
                    }
                }
                None
            }
        },
        max_retries,
        delay_ms,
    )
    .await;

    match result {
        Some(()) => Ok(()),
        None => Err(CommandError::Timeout {
            err: "timed out waiting for network to start".to_string(),
        }),
    }
}

async fn wait_for_network_descriptor(
    nd: &NetworkDirectory,
) -> Result<NetworkDescriptorModel, CommandError> {
    let max_retries = 30;
    let delay_ms = 1000;
    let result: Option<NetworkDescriptorModel> = retry_with_timeout(
        || {
            let nd = nd;
            async move {
                if let Ok(Some(descriptor)) = nd.load_network_descriptor() {
                    return Some(descriptor);
                }
                None
            }
        },
        max_retries,
        delay_ms,
    )
    .await;

    match result {
        Some(descriptor) => Ok(descriptor),
        None => Err(CommandError::Timeout {
            err: "timed out waiting for network descriptor".to_string(),
        }),
    }
}
