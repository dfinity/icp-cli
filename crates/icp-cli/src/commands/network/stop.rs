use std::time::Duration;

use clap::Parser;
use icp::{
    fs::{lock::LockError, remove_file},
    manifest,
    network::Configuration,
    project::DEFAULT_LOCAL_NETWORK_NAME,
};
use sysinfo::{Pid, ProcessesToUpdate, Signal, System};

use icp::context::Context;

const TIMEOUT_SECS: u64 = 30;

/// Stop a background network
#[derive(Parser, Debug)]
pub struct Cmd {
    /// Name of the network to stop
    #[arg(default_value = DEFAULT_LOCAL_NETWORK_NAME)]
    name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Locate(#[from] manifest::ProjectRootLocateError),

    #[error("project does not contain a network named '{name}'")]
    Network { name: String },

    #[error("network '{name}' must be a managed network")]
    Unmanaged { name: String },

    #[error(transparent)]
    NetworkAccess(#[from] icp::network::AccessError),

    #[error("network '{name}' is not running in the background")]
    NotRunning { name: String },

    #[error(transparent)]
    LoadPid(#[from] icp::network::LoadPidError),

    #[error("process {pid} did not exit within {timeout} seconds")]
    Timeout { pid: Pid, timeout: u64 },

    #[error(transparent)]
    LockFile(#[from] LockError),
}

pub async fn exec(ctx: &Context, cmd: &Cmd) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await?;

    // Obtain network configuration
    let network = p.networks.get(&cmd.name).ok_or(CommandError::Network {
        name: cmd.name.clone(),
    })?;

    if let Configuration::Connected { connected: _ } = &network.configuration {
        return Err(CommandError::Unmanaged {
            name: cmd.name.to_owned(),
        });
    };

    // Network directory
    let nd = ctx.network.get_network_directory(network)?;

    // Load PID from file
    let pid = nd
        .load_background_network_runner_pid()
        .await?
        .ok_or(CommandError::NotRunning {
            name: cmd.name.clone(),
        })?;

    let _ = ctx
        .term
        .write_line(&format!("Stopping background network (PID: {})...", pid));

    send_sigint(pid);
    wait_for_process_exit(pid)?;

    nd.root()?
        .with_write(async |root| {
            let pid_file = root.background_network_runner_pid_file();
            let _ = remove_file(&pid_file); // Cleanup is nice, but optional
            let descriptor_file = root.network_descriptor_path();
            // Desciptor file must be deleted to allow the network to be restarted, but if it doesn't exist, that's fine too
            let _ = remove_file(&descriptor_file);

            Ok::<_, CommandError>(())
        })
        .await??;

    let _ = ctx.term.write_line("Network stopped successfully");

    Ok(())
}

fn send_sigint(pid: Pid) {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    if let Some(process) = system.process(pid) {
        process.kill_with(Signal::Interrupt);
    }
}

fn wait_for_process_exit(pid: Pid) -> Result<(), CommandError> {
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(TIMEOUT_SECS);
    let mut system = System::new();

    loop {
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        if system.process(pid).is_none() {
            return Ok(());
        }

        if start.elapsed() > timeout {
            return Err(CommandError::Timeout {
                pid,
                timeout: TIMEOUT_SECS,
            });
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}
