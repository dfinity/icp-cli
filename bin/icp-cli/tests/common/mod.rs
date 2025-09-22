#![allow(dead_code)]

use std::process::{Child, Command};
use url::Url;

use httptest::{Expectation, Server, matchers::*, responders::*};

pub mod clients;
mod context;

pub use context::TestContext;

/// ICP ledger on mainnet
pub const ICP_LEDGER_CID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";

/// Cycles ledger on mainnet
pub const CYCLES_LEDGER_CID: &str = "um5iw-rqaaa-aaaaq-qaaba-cai";

/// Governance on mainnet
pub const GOVERNANCE_ID: &str = "rrkah-fqaaa-aaaaa-aaaaq-cai";

#[cfg(unix)]
pub const PATH_SEPARATOR: &str = ":";

#[cfg(windows)]
pub const PATH_SEPARATOR: &str = ";";

/// This ID is dependent on the toplogy being served by pocket-ic
/// NOTE: If the topology is changed (another subnet is added, etc) the ID may change.
/// References:
/// - http://localhost:8000/_/topology
/// - http://localhost:8000/_/dashboard
pub const SUBNET_ID: &str = "cok7q-nnbiu-4xwf6-7gpqg-kwzft-mqypn-uepxh-mx2hy-q4wuy-5s5my-eae";

// Spawns a test server that expects a single request and responds with a 200 status code and the given body
pub fn spawn_test_server(method: &str, path: &str, body: &[u8]) -> httptest::Server {
    // Run the server
    let server = Server::run();

    // Set up the expectation
    server.expect(
        Expectation::matching(request::method_path(method.to_owned(), path.to_owned()))
            .times(1)
            .respond_with(status_code(200).body(body.to_owned())),
    );

    // Return the server instance
    server
}

// A network run by icp-cli for a test. These fields are read from the network descriptor
// after starting the network.
pub struct TestNetwork {
    pub pocketic_url: Url,
    pub pocketic_instance_id: usize,
    pub gateway_port: u16,
    pub root_key: String,
}

// A network run by icp-cli, but set up in ~/.config/dfx/networks.json for dfx to connect to.
pub struct TestNetworkForDfx {
    pub dfx_network_name: String,
    pub gateway_port: u16,
}

pub struct ChildGuard {
    child: Child,
}

impl ChildGuard {
    pub fn spawn(cmd: &mut Command) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let child = cmd.spawn()?;
        Ok(Self { child })
    }

    fn send_sigint_to_process_group(&mut self) {
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, killpg};
            use nix::unistd::Pid;

            let pid = self.child.id();
            let pgid = Pid::from_raw(pid as i32); // Child PID = PGID
            let _ = killpg(pgid, Signal::SIGINT);
            // Give the process some time to shut down gracefully
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => {
                // Already exited, nothing to do
            }
            Ok(None) => {
                self.send_sigint_to_process_group();
                if let Err(e) = self.child.kill() {
                    eprintln!("Failed to kill child process: {}", e);
                }
                if let Err(e) = self.child.wait() {
                    eprintln!("Failed to wait on child process: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Failed to check child process status: {}", e);
            }
        }
    }
}
