use candid::Principal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::prelude::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkDescriptorGatewayPort {
    pub fixed: bool,
    pub port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkDescriptorModel {
    pub v: String,
    pub id: Uuid,
    pub project_dir: PathBuf,
    pub network: String,
    pub network_dir: PathBuf,
    pub gateway: NetworkDescriptorGatewayPort,
    pub child_locator: ChildLocator,
    #[serde(with = "hex::serde")]
    pub root_key: Vec<u8>,
    pub pocketic_config_port: Option<u16>,
    pub pocketic_instance_id: Option<usize>,
    pub candid_ui_canister_id: Option<Principal>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum ChildLocator {
    Pid {
        pid: u32,
        /// Process start time in seconds since UNIX epoch, used to detect PID reuse.
        start_time: u64,
    },
    Container {
        id: String,
        socket: String,
        rm_on_exit: bool,
    },
}

impl ChildLocator {
    /// Checks if the process or container referenced by this locator is still alive.
    pub fn is_alive(&self) -> bool {
        match self {
            ChildLocator::Pid { pid, start_time } => {
                use sysinfo::{Pid, ProcessesToUpdate, System};
                let mut system = System::new();
                let sysinfo_pid = Pid::from_u32(*pid);
                system.refresh_processes(ProcessesToUpdate::Some(&[sysinfo_pid]), true);
                system
                    .process(sysinfo_pid)
                    .is_some_and(|p| p.start_time() == *start_time)
            }
            ChildLocator::Container { id, socket, .. } => {
                // Check if the container exists and is running
                use std::process::Command;
                let socket_arg = format!("--host={socket}");
                Command::new("docker")
                    .args([&socket_arg, "inspect", "--format={{.State.Running}}", id])
                    .output()
                    .ok()
                    .and_then(|output| {
                        if output.status.success() {
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            Some(stdout.trim() == "true")
                        } else {
                            None
                        }
                    })
                    .unwrap_or(false)
            }
        }
    }
}

impl NetworkDescriptorModel {
    pub fn gateway_port(&self) -> Option<u16> {
        if self.gateway.fixed {
            return Some(self.gateway.port);
        }

        None
    }
}
