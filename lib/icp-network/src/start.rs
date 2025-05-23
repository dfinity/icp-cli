use crate::config::model::managed::BindPort::Fixed;
use crate::config::model::managed::{BindPort, ManagedNetworkModel};
use crate::config::model::network_descriptor::NetworkDescriptorModel;
use crate::structure::NetworkDirectoryStructure;
use fd_lock::RwLock;
use icp_support::fs::{RemoveFileError, WriteFileError, create_dir_all, remove_file, write};
use icp_support::json::{LoadJsonFileError, SaveJsonFileError, save_json_file};
use icp_support::process::process_running;
use snafu::prelude::*;
use std::fs::{OpenOptions, read_to_string};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;
use tokio::process::Child;
use tokio::signal::ctrl_c;
use tokio::time::sleep;

#[derive(Debug, Snafu)]
pub enum StartLocalNetworkError {
    #[snafu(transparent)]
    LoadJsonFile { source: LoadJsonFileError },

    #[snafu(display("already running (this project)"))]
    AlreadyRunningThisProject,

    #[snafu(display("already running (other project)"))]
    AlreadyRunningOtherProject,

    #[snafu(display("failed to open lock file"))]
    OpenLockFile { source: std::io::Error },

    #[snafu(transparent)]
    ReadNetworkDescriptor { source: ReadNetworkDescriptorError },

    #[snafu(transparent)]
    RemoveFile { source: RemoveFileError },

    #[snafu(transparent)]
    SaveJsonFile { source: SaveJsonFileError },

    #[snafu(transparent)]
    WriteFile { source: WriteFileError },
}

pub async fn run_local_network(
    config: ManagedNetworkModel,
    nds: NetworkDirectoryStructure,
) -> Result<(), StartLocalNetworkError> {
    eprintln!("Run local network");
    let project_descriptor_path = nds.project_descriptor_path();

    let mut file = RwLock::new(
        OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(true)
            .open(nds.lock_path())
            .map_err(|source| StartLocalNetworkError::OpenLockFile { source })?,
    );
    let _guard = file
        .try_write()
        .map_err(|_| StartLocalNetworkError::AlreadyRunningThisProject)?;
    eprintln!("Got lock on {}", nds.lock_path().display());

    // from here we know the project network is not running (it would hold the network lock)

    // If we're going to use a fixed port, we could check to make sure another
    // project isn't running a network on the same port. But we can detect this when
    // starting the server.

    if let Fixed(port) = config.bind.port {
        eprintln!("Checking for existing network on port {}", port);

        let port_descriptor_path = NetworkDirectoryStructure::port_descriptor_path(port);
        if let Some(port_des) = read_network_descriptor(&port_descriptor_path).await? {
            if let Some(pid) = port_des.pid {
                if process_running(pid) {
                    return Err(StartLocalNetworkError::AlreadyRunningThisProject);
                }
            }
            remove_file(&port_descriptor_path)?;
        }
    }

    run_pocketic(config, nds).await;
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum ReadNetworkDescriptorError {
    #[snafu(display("Failed to open descriptor file: {source}"))]
    Open {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("Failed to read descriptor file: {source}"))]
    Read {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("Failed to parse descriptor JSON: {source}"))]
    Parse {
        source: serde_json::Error,
        path: PathBuf,
    },
}

pub async fn read_network_descriptor(
    p: &Path,
) -> Result<Option<NetworkDescriptorModel>, ReadNetworkDescriptorError> {
    let path = p.to_owned();

    let result = tokio::task::spawn_blocking(move || {
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(ReadNetworkDescriptorError::Open { source: e, path }),
        };

        let lock = RwLock::new(file);
        let guard = lock.read().context(ReadSnafu { path: path.clone() })?;

        let model: NetworkDescriptorModel =
            serde_json::from_reader(&*guard).context(ParseSnafu { path })?;

        Ok(Some(model))
    })
    .await
    .unwrap(); // join error is not expected unless panicked

    result
}
async fn run_pocketic(config: ManagedNetworkModel, nds: NetworkDirectoryStructure) {
    let pocketic_path =
        PathBuf::from("/Users/ericswanson/.cache/dfinity/versions/0.26.1/pocket-ic");
    eprintln!("PocketIC path: {}", pocketic_path.display());

    create_dir_all(&nds.pocketic_dir()).unwrap();
    let port_file = nds.pocketic_port_file();
    if port_file.exists() {
        remove_file(&port_file).unwrap();
    }
    eprintln!("Port file: {}", port_file.display());
    let mut child = spawn_pocketic(&pocketic_path, &config.bind.port, &port_file);
    let port = wait_for_port(&port_file, &mut child).await.unwrap();
    eprintln!("PocketIC started on port {port}");
    // let mut pic = PocketIcBuilder::new_with_config(SubnetConfigSet {
    //     application: 1,
    //     bitcoin: true,
    //     fiduciary: true,
    //     ii: true,
    //     nns: true,
    //     sns: true,
    //     system: 1,
    //     verified_application: 1,
    // })
    //     .build_async()
    //     .await;
    // let instance = pic.instance_id;
    // let config_port = pic.get_server_url().port().unwrap();
    // let webserver_port = pic.make_live(None).await.port().unwrap();
    //
    // let network_descriptor = NetworkDescriptorModel {
    //     id: uuid::Uuid::new_v4(),
    //     pid: Some(std::process::id()),
    //     path: network_directory_structure.network_root().to_path_buf(),
    //     gateway_port: Some(webserver_port),
    //     root_key: "".to_string(),
    // };
    //
    // todo!()
}

fn spawn_pocketic(
    pocketic_path: &Path,
    port: &BindPort,
    port_file: &Path,
) -> tokio::process::Child {
    // form the pocket-ic command here similar to the ic-starter command
    let mut cmd = tokio::process::Command::new(pocketic_path);
    if let Fixed(port) = port {
        cmd.args(["--port", &port.to_string()]);
    };
    cmd.arg("--port-file");
    cmd.arg(&port_file.as_os_str());
    cmd.args(["--ttl", "2592000", "--log-levels", "error"]);

    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let last_start = std::time::Instant::now();
    eprintln!("Starting PocketIC...");
    eprintln!("PocketIC command: {:?}", cmd);
    cmd.spawn().expect("Could not start PocketIC.")
}

#[derive(Debug, Snafu)]
#[snafu(display("timeout waiting for port file"))]
pub struct WaitForPortTimeoutError;

pub async fn wait_for_port_file(path: &Path) -> Result<u16, WaitForPortTimeoutError> {
    let mut retries = 0;
    while retries < 3000 {
        if let Ok(contents) = read_to_string(path) {
            if contents.ends_with('\n') {
                if let Ok(port) = contents.trim().parse::<u16>() {
                    return Ok(port);
                }
            }
        }

        sleep(Duration::from_millis(100)).await;
        retries += 1;
    }
    Err(WaitForPortTimeoutError)
}

#[derive(Debug, Snafu)]
#[snafu(display("Child process exited early with status {status}"))]
pub struct ChildExitError {
    pub status: ExitStatus,
}

/// Yields immediately if the child exits.
pub async fn notice_child_exit(child: &mut Child) -> ChildExitError {
    loop {
        if let Some(status) = child.try_wait().expect("child status query failed") {
            return ChildExitError { status };
        }
        sleep(Duration::from_millis(100)).await;
    }
}

#[derive(Debug, Snafu)]
pub enum WaitForPortError {
    #[snafu(display("Interrupted"))]
    Interrupted,
    #[snafu(transparent)]
    PortFile { source: WaitForPortTimeoutError },
    #[snafu(transparent)]
    ChildExited { source: ChildExitError },
}

/// Waits for the child to populate a port number.
/// Exits early if the child exits or the user interrupts.
pub async fn wait_for_port(path: &Path, child: &mut Child) -> Result<u16, WaitForPortError> {
    tokio::select! {
        res = wait_for_port_file(path) => res.map_err(WaitForPortError::from),
        _ = ctrl_c() => Err(WaitForPortError::Interrupted),
        err = notice_child_exit(child) => Err(WaitForPortError::ChildExited { source: err }),
    }
}
