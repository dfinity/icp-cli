use crate::StartLocalNetworkError::NoPocketIcPath;
use crate::config::model::managed::BindPort::Fixed;
use crate::config::model::managed::{BindPort, ManagedNetworkModel};
use crate::config::model::network_descriptor::NetworkDescriptorModel;
use crate::managed::pocketic::admin::{
    CreateHttpGatewayError, CreateInstanceError, PocketIcAdminInterface,
};
use crate::managed::pocketic::instance::PocketIcInstance;
use crate::managed::pocketic::native::spawn_pocketic;
use crate::status;
use crate::structure::NetworkDirectoryStructure;
use candid::Principal;
use fd_lock::RwLock;
use icp_fs::fs::{CreateDirAllError, RemoveFileError, WriteFileError, create_dir_all, remove_dir_all, remove_file, write, RemoveDirAllError};
use icp_fs::json::{LoadJsonFileError, SaveJsonFileError, save_json_file};
use pocket_ic::common::rest::HttpGatewayBackend;
use reqwest::Url;
use snafu::prelude::*;
use std::env::var_os;
use std::fs::{OpenOptions, read_to_string};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;
use tokio::process::Child;
use tokio::select;
use tokio::signal::ctrl_c;
use tokio::time::sleep;
use uuid::Uuid;
use crate::managed::run::InitializePocketicError::NoRootKey;

#[derive(Debug, Snafu)]
pub enum StartLocalNetworkError {
    #[snafu(transparent)]
    LoadJsonFile { source: LoadJsonFileError },

    #[snafu(display("already running (this project)"))]
    AlreadyRunningThisProject,

    #[snafu(display("already running (other project)"))]
    AlreadyRunningOtherProject,

    #[snafu(transparent)]
    CreateDirFailed { source: CreateDirAllError },

    #[snafu(display("ICP_POCKET_IC_PATH environment variable is not set"))]
    NoPocketIcPath,

    #[snafu(display("failed to open lock file"))]
    OpenLockFile { source: std::io::Error },

    #[snafu(transparent)]
    RemoveFile { source: RemoveFileError },

    #[snafu(transparent)]
    SaveJsonFile { source: SaveJsonFileError },

    #[snafu(transparent)]
    WriteFile { source: WriteFileError },
}

pub async fn run_network(
    config: ManagedNetworkModel,
    nds: NetworkDirectoryStructure,
) -> Result<(), StartLocalNetworkError> {
    let pocketic_path = PathBuf::from(var_os("ICP_POCKET_IC_PATH").ok_or(NoPocketIcPath)?);

    create_dir_all(nds.network_root())?;

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
    eprintln!("Holding lock on {}", nds.lock_path().display());

    run_pocketic(&pocketic_path, config, nds).await;
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum RunPocketIcError {
    #[snafu(transparent)]
    CreateDirAll { source: CreateDirAllError },

    #[snafu(transparent)]
    RemoveDirAll { source: RemoveDirAllError },

    #[snafu(transparent)]
    RemoveFile { source: RemoveFileError },

    #[snafu(transparent)]
    SaveJsonFile { source: SaveJsonFileError },

    #[snafu(display("Failed to start PocketIC: {source}"))]
    StartPocketIc { source: std::io::Error },

    #[snafu(transparent)]
    InitPocketIc { source: InitializePocketicError },

    #[snafu(transparent)]
    WaitForPort { source: WaitForPortError },
}
async fn run_pocketic(
    pocketic_path: &Path,
    config: ManagedNetworkModel,
    nds: NetworkDirectoryStructure,
) -> Result<(), RunPocketIcError> {
    eprintln!("PocketIC path: {}", pocketic_path.display());

    create_dir_all(&nds.pocketic_dir())?;
    let port_file = nds.pocketic_port_file();
    if port_file.exists() {
        remove_file(&port_file)?;
    }
    eprintln!("Port file: {}", port_file.display());
    if nds.state_dir().exists() {
        remove_dir_all(&nds.state_dir())?;
    }
    create_dir_all(&nds.state_dir())?;
    let mut child = spawn_pocketic(&pocketic_path, &port_file);

    let result = async {
        let port = wait_for_port(&port_file, &mut child).await?;
        eprintln!("PocketIC started on port {port}");
        let instance = initialize_pocketic(port, &nds.state_dir()).await?;

        let nd = NetworkDescriptorModel {
            id: Uuid::new_v4(),
            path: nds.network_root().to_path_buf(),
            gateway_port: Some(instance.gateway_port),
            pid: Some(child.id().unwrap().into()),
            root_key: instance.root_key,
        };
        save_json_file(&nds.project_descriptor_path(), &nd)?;
        eprintln!("Press Ctrl-C to exit.");
        let _ = wait_for_shutdown(&mut child).await;
        Ok(())
    }
    .await;

    let _ = child.kill().await;
    let _ = child.wait().await;

    result
}

pub enum ShutdownReason {
    CtrlC,
    ChildExited,
}
async fn wait_for_shutdown(child: &mut Child) -> ShutdownReason {
    select!(
        _ = ctrl_c() => {
            eprintln!("Received Ctrl-C, shutting down PocketIC...");
            ShutdownReason::CtrlC
        }
        res = notice_child_exit(child) => {
            eprintln!("PocketIC exited with status: {:?}", res.status);
            ShutdownReason::ChildExited
        }
    )
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

struct PocketIcInstanceProperties {
    instance_id: usize,
    effective_canister_id: Principal,
    root_key: String,
}

#[derive(Debug, Snafu)]
pub enum InitializePocketicError {
    #[snafu(transparent)]
    CreateInstance { source: CreateInstanceError },

    #[snafu(transparent)]
    CreateHttpGateway { source: CreateHttpGatewayError },

    #[snafu(display("no root key reported in status"))]
    NoRootKey,

    #[snafu(display("Failed to save effective config: {source}"))]
    SaveEffectiveConfig { source: SaveJsonFileError },

    #[snafu(transparent)]
    PingAndWait { source: status::PingAndWaitError },

    #[snafu(transparent)]
    Reqwest { source: reqwest::Error },
}

async fn initialize_pocketic(
    port: u16,
    state_dir: &Path,
) -> Result<PocketIcInstance, InitializePocketicError> {
    let pic =
        PocketIcAdminInterface::new(format!("http://localhost:{port}").parse::<Url>().unwrap());

    eprintln!("Initializing PocketIC instance");

    eprintln!("Creating instance");
    let (instance_id, topology) = pic.create_instance(state_dir).await?;
    let default_effective_canister_id = topology.default_effective_canister_id;
    eprintln!("Created instance with id {}", instance_id);

    eprintln!("Setting time");
    pic.set_time(instance_id).await?;

    eprintln!("Set auto-progress");
    let artificial_delay = 600;
    pic.auto_progress(instance_id, artificial_delay).await?;

    let gateway_info = pic
        .create_http_gateway(
            HttpGatewayBackend::PocketIcInstance(instance_id),
            Some(8000),
        )
        .await?;
    eprintln!(
        "Created HTTP gateway instance={} port={}",
        gateway_info.instance_id, gateway_info.port
    );

    let agent_url = format!("http://localhost:{}", gateway_info.port);

    eprintln!("Agent url is {}", agent_url);
    let status = status::ping_and_wait(&agent_url).await?;

    let root_key = status.root_key.ok_or(NoRootKey);
    let root_key = hex::encode(root_key);
    eprintln!("Root key: {root_key}");

    let props = PocketIcInstance {
        admin: pic,
        gateway_port: gateway_info.port,
        instance_id,
        effective_canister_id: default_effective_canister_id.into(),
        root_key,
    };
    Ok(props)
}
