#![allow(dead_code)]

use std::process::{Child, Command};

use httptest::{Expectation, Server, matchers::*, responders::*};

pub(crate) mod clients;
mod context;

pub(crate) use context::TestContext;

#[cfg(unix)]
pub(crate) const PATH_SEPARATOR: &str = ":";

#[cfg(windows)]
pub(crate) const PATH_SEPARATOR: &str = ";";

/// A network manifest for a network using a random port
pub(crate) const NETWORK_RANDOM_PORT: &str = r#"
networks:
  - name: my-network
    mode: managed
    gateway:
      port: 0
"#;

/// An environment manifest utilizing the above network
pub(crate) const ENVIRONMENT_RANDOM_PORT: &str = r#"
environments:
  - name: my-environment
    network: my-network
"#;

/// This ID is dependent on the toplogy being served by pocket-ic
/// NOTE: If the topology is changed (another subnet is added, etc) the ID may change.
/// References:
/// - http://localhost:8000/_/topology
/// - http://localhost:8000/_/dashboard
pub(crate) const SUBNET_ID: &str =
    "cok7q-nnbiu-4xwf6-7gpqg-kwzft-mqypn-uepxh-mx2hy-q4wuy-5s5my-eae";

// Spawns a test server that expects a single request and responds with a 200 status code and the given body
pub(crate) fn spawn_test_server(method: &str, path: &str, body: &[u8]) -> httptest::Server {
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
pub(crate) struct TestNetwork {
    pub(crate) gateway_port: u16,
    pub(crate) root_key: Vec<u8>,
}

pub(crate) struct ChildGuard {
    child: Child,
}

impl ChildGuard {
    pub(crate) fn spawn(cmd: &mut Command) -> std::io::Result<Self> {
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
                    eprintln!("Failed to kill child process: {e}");
                }
                if let Err(e) = self.child.wait() {
                    eprintln!("Failed to wait on child process: {e}");
                }
            }
            Err(e) => {
                eprintln!("Failed to check child process status: {e}");
            }
        }
    }
}
