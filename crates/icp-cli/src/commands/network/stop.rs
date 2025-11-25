use anyhow::{anyhow, bail};
use clap::Parser;
use icp::{fs::remove_file, network::Configuration, project::DEFAULT_LOCAL_NETWORK_NAME};
use std::time::Duration;
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

pub async fn exec(ctx: &Context, cmd: &Cmd) -> Result<(), anyhow::Error> {
    // Load project
    let p = ctx.project.load().await?;

    // Obtain network configuration
    let network = p
        .networks
        .get(&cmd.name)
        .ok_or_else(|| anyhow!("project does not contain a network named '{}'", cmd.name))?;

    if let Configuration::Connected { connected: _ } = &network.configuration {
        bail!("network '{}' is not a managed network", cmd.name)
    };

    // Network directory
    let nd = ctx.network.get_network_directory(network)?;

    // Load PID from file
    let pid = nd
        .load_background_network_runner_pid()
        .await?
        .ok_or_else(|| anyhow!("network '{}' is not running in the background", cmd.name))?;

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

            Ok::<_, anyhow::Error>(())
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

fn wait_for_process_exit(pid: Pid) -> Result<(), anyhow::Error> {
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(TIMEOUT_SECS);
    let mut system = System::new();

    loop {
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        if system.process(pid).is_none() {
            return Ok(());
        }

        if start.elapsed() > timeout {
            bail!(
                "process {} did not exit within {} seconds",
                pid,
                TIMEOUT_SECS
            );
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}
