use std::time::Duration;

use clap::Parser;
use icp::{fs::remove_file, manifest, network::NetworkDirectory};
use sysinfo::{Pid, ProcessesToUpdate, Signal, System};

use crate::commands::Context;

const TIMEOUT_SECS: u64 = 30;

/// Stop a background network
#[derive(Parser, Debug)]
pub struct Cmd {
    /// Name of the network to stop
    #[arg(default_value = "local")]
    name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Locate(#[from] manifest::LocateError),

    #[error("project does not contain a network named '{name}'")]
    Network { name: String },

    #[error("network '{name}' is not running in the background")]
    NotRunning { name: String },

    #[error(transparent)]
    LoadPid(#[from] icp::network::LoadPidError),

    #[error("process {pid} did not exit within {timeout} seconds")]
    Timeout { pid: Pid, timeout: u64 },
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await?;

    // Check network exists
    p.networks.get(&cmd.name).ok_or(CommandError::Network {
        name: cmd.name.clone(),
    })?;

    // Project root
    let pdir = ctx.workspace.locate()?;

    // Network root
    let ndir = pdir.join(".icp").join("networks").join(&cmd.name);

    // Network directory
    let nd = NetworkDirectory::new(
        &cmd.name,                   // name
        &ndir,                       // network_root
        &ctx.dirs.port_descriptor(), // port_descriptor_dir
    );

    // Load PID from file
    let pid = nd
        .load_background_network_runner_pid()?
        .ok_or(CommandError::NotRunning {
            name: cmd.name.clone(),
        })?;

    let _ = ctx
        .term
        .write_line(&format!("Stopping background network (PID: {})...", pid));

    // Send SIGINT to the process
    send_sigint(pid);

    // Wait for process to exit
    wait_for_process_exit(pid)?;

    // Remove PID file
    let pid_file = nd.structure.background_network_runner_pid_file();
    let _ = remove_file(&pid_file); // Cleanup is nice, but optional

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
        // Check if process is still running
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

        // Sleep briefly before checking again
        std::thread::sleep(Duration::from_millis(100));
    }
}
