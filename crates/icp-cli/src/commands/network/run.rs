use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use clap::Args;
use ic_agent::{Agent, AgentError};
use icp::{
    identity::manifest::{LoadIdentityManifestError, load_identity_list},
    manifest,
    network::{Configuration, NetworkDirectory, RunNetworkError, run_network},
};
use sysinfo::Pid;
use tracing::debug;

use icp::context::Context;

/// Run a given network
#[derive(Args, Debug)]
pub(crate) struct RunArgs {
    /// Name of the network to run
    #[arg(default_value = "local")]
    name: String,

    /// Starts the network in a background process. This command will exit once the network is running.
    /// To stop the network, use 'icp network stop'.
    #[arg(long)]
    background: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
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

pub(crate) async fn exec(ctx: &Context, args: &RunArgs) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await?;

    // Obtain network configuration
    let network = p.networks.get(&args.name).ok_or(CommandError::Network {
        name: args.name.to_owned(),
    })?;

    let cfg = match &network.configuration {
        // Locally-managed network
        Configuration::Managed { managed: cfg } => cfg,

        // Non-managed networks cannot be started
        Configuration::Connected { connected: _ } => {
            return Err(CommandError::Unmanaged {
                name: args.name.to_owned(),
            });
        }
    };

    // Network root
    let pdir = &p.dir;
    let ndir = pdir.join(".icp").join("networks").join(&network.name);

    // Network directory
    let nd = NetworkDirectory::new(
        &network.name,               // name
        &ndir,                       // network_root
        &ctx.dirs.port_descriptor(), // port_descriptor_dir
    );
    nd.ensure_exists()
        .map_err(|e| RunNetworkError::CreateDirFailed { source: e })?;

    // Identities
    let ids = load_identity_list(&ctx.dirs.identity())?;

    // Determine ICP accounts to seed
    let seed_accounts = ids.identities.values().map(|id| id.principal());

    debug!("Project root: {pdir}");
    debug!("Network root: {ndir}");

    if args.background {
        let mut child = run_in_background()?;
        nd.save_background_network_runner_pid(Pid::from(child.id() as usize))?;
        relay_child_output_until_healthy(ctx, &mut child, &nd).await?;
    } else {
        run_network(
            cfg,          // config
            nd,            // nd
            pdir,          // project_root
            seed_accounts, // seed_accounts
        )
        .await?;
    }
    Ok(())
}

async fn relay_child_output_until_healthy(
    ctx: &Context,
    child: &mut Child,
    nd: &NetworkDirectory,
) -> Result<(), CommandError> {
    let stdout = child.stdout.take().expect("Failed to take child stdout");
    let stderr = child.stderr.take().expect("Failed to take child stderr");

    let stop_printing_child_output = Arc::new(AtomicBool::new(false));

    // Spawn threads to relay output
    let term = ctx.term.clone();
    let should_stop_clone = Arc::clone(&stop_printing_child_output);
    let stdout_thread = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if should_stop_clone.load(Ordering::Relaxed) {
                break;
            }
            if let Ok(line) = line {
                let _ = term.write_line(&line);
            }
        }
    });

    let term = ctx.term.clone();
    let should_stop_clone = Arc::clone(&stop_printing_child_output);
    let stderr_thread = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if should_stop_clone.load(Ordering::Relaxed) {
                break;
            }
            if let Ok(line) = line {
                let _ = term.write_line(&line);
            }
        }
    });

    wait_for_healthy_network(nd).await?;

    // Signal threads to stop
    stop_printing_child_output.store(true, Ordering::Relaxed);

    // Don't join the threads - they're likely blocked on I/O waiting for the next line.
    // They'll terminate naturally when the pipes close, or when the next line arrives.
    drop(stdout_thread);
    drop(stderr_thread);

    Ok(())
}

#[allow(clippy::result_large_err)]
fn run_in_background() -> Result<Child, CommandError> {
    let exe = std::env::current_exe().expect("Failed to get current executable.");
    let mut cmd = Command::new(exe);
    // Skip 1 because arg0 is this executable's path.
    cmd.args(std::env::args().skip(1).filter(|a| !a.eq("--background")))
        .stdin(Stdio::null())
        .stdout(Stdio::piped()) // Capture stdout so we can relay it
        .stderr(Stdio::piped()); // Capture stderr so we can relay it

    // On Unix, create a new process group so the child can continue running
    // independently after the run command exits
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

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
    let max_retries = 30;
    let delay_ms = 1000;

    // Wait for network descriptor to be written
    let network = retry_with_timeout(
        || async move { nd.load_network_descriptor().unwrap_or(None) },
        max_retries,
        delay_ms,
    )
    .await
    .ok_or(CommandError::Timeout {
        err: "timed out waiting for network descriptor".to_string(),
    })?;

    // Wait for network to report itself healthy
    let port = network.gateway.port;
    let agent = Agent::builder()
        .with_url(format!("http://127.0.0.1:{port}"))
        .build()?;
    retry_with_timeout(
        || {
            let agent = agent.clone();
            async move {
                let status = agent.status().await;
                if let Ok(status) = status
                    && matches!(&status.replica_health_status, Some(status) if status == "healthy")
                {
                    return Some(());
                }

                None
            }
        },
        max_retries,
        delay_ms,
    )
    .await
    .ok_or(CommandError::Timeout {
        err: "timed out waiting for network to start".to_string(),
    })
}
