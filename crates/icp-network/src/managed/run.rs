use std::env::var;

use candid::Principal;
use icp::{
    fs::{create_dir_all, remove_dir_all, remove_file},
    prelude::*,
};
use snafu::prelude::*;
use tokio::{process::Child, select, signal::ctrl_c};
use uuid::Uuid;

use crate::{
    BindPort, ManagedNetworkModel, NetworkDirectory,
    RunNetworkError::NoPocketIcPath,
    config::{NetworkDescriptorGatewayPort, NetworkDescriptorModel},
    directory::SaveNetworkDescriptorError,
    lock::OpenFileForWriteLockError,
    managed::{
        descriptor::{AnotherProjectRunningOnSamePortError, ProjectNetworkAlreadyRunningError},
        pocketic::{
            self, CreateHttpGatewayError, CreateInstanceError, PocketIcInstance, WaitForPortError,
            default_instance_config, notice_child_exit, spawn_pocketic, wait_for_port,
        },
    },
    status,
};

pub async fn run_network(
    config: &ManagedNetworkModel,
    nd: NetworkDirectory,
    project_root: &Path,
    seed_accounts: impl Iterator<Item = Principal> + Clone,
) -> Result<(), RunNetworkError> {
    let pocketic_path = PathBuf::from(var("ICP_POCKET_IC_PATH").ok().ok_or(NoPocketIcPath)?);

    nd.ensure_exists()?;

    let mut network_lock = nd.open_network_lock_file()?;
    let _network_claim = network_lock.try_acquire()?;

    let mut port_lock;
    let _port_claim;

    if let BindPort::Fixed(port) = &config.gateway.port {
        port_lock = Some(nd.open_port_lock_file(*port)?);
        _port_claim = Some(port_lock.as_mut().unwrap().try_acquire()?);
    }

    run_pocketic(&pocketic_path, config, &nd, project_root, seed_accounts).await?;
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum RunNetworkError {
    #[snafu(transparent)]
    ProjectNetworkAlreadyRunning {
        source: ProjectNetworkAlreadyRunningError,
    },

    #[snafu(transparent)]
    AnotherProjectRunningOnSamePort {
        source: AnotherProjectRunningOnSamePortError,
    },

    #[snafu(transparent)]
    CreateDirFailed { source: icp::fs::Error },

    #[snafu(display("ICP_POCKET_IC_PATH environment variable is not set"))]
    NoPocketIcPath,

    #[snafu(transparent)]
    OpenFileForWriteLock { source: OpenFileForWriteLockError },

    #[snafu(transparent)]
    RunPocketIc { source: RunPocketIcError },
}

async fn run_pocketic(
    pocketic_path: &Path,
    config: &ManagedNetworkModel,
    nd: &NetworkDirectory,
    project_root: &Path,
    seed_accounts: impl Iterator<Item = Principal> + Clone,
) -> Result<(), RunPocketIcError> {
    let nds = nd.structure();
    eprintln!("PocketIC path: {pocketic_path}");

    create_dir_all(&nds.pocketic_dir()).context(CreateDirAllSnafu)?;
    let port_file = nds.pocketic_port_file();
    if port_file.exists() {
        remove_file(&port_file).context(RemoveDirAllSnafu)?;
    }
    eprintln!("Port file: {port_file}");
    if nds.state_dir().exists() {
        remove_dir_all(&nds.state_dir()).context(RemoveDirAllSnafu)?;
    }
    create_dir_all(&nds.state_dir()).context(CreateDirAllSnafu)?;
    let mut child = spawn_pocketic(pocketic_path, &port_file);

    let result = async {
        let pocketic_port = wait_for_port(&port_file, &mut child).await?;
        eprintln!("PocketIC started on port {pocketic_port}");
        let instance = initialize_pocketic(
            pocketic_port,
            &config.gateway.port,
            &nds.state_dir(),
            seed_accounts,
        )
        .await?;

        let gateway = NetworkDescriptorGatewayPort {
            port: instance.gateway_port,
            fixed: matches!(config.gateway.port, BindPort::Fixed(_)),
        };
        let default_effective_canister_id = instance.effective_canister_id;
        let descriptor = NetworkDescriptorModel {
            id: Uuid::new_v4(),
            project_dir: project_root.to_path_buf(),
            network: nd.network_name.to_owned(),
            network_dir: nd.structure().network_root.to_path_buf(),
            gateway,
            default_effective_canister_id,
            pocketic_url: instance.admin.base_url.to_string(),
            pocketic_instance_id: instance.instance_id,
            pid: Some(child.id().unwrap()),
            root_key: instance.root_key,
        };

        let _cleaner = nd.save_network_descriptors(&descriptor)?;
        eprintln!("Press Ctrl-C to exit.");
        let _ = wait_for_shutdown(&mut child).await;
        Ok(())
    }
    .await;

    let _ = child.kill().await;
    let _ = child.wait().await;

    result
}

#[derive(Debug, Snafu)]
pub enum RunPocketIcError {
    #[snafu(display("failed to create dir"))]
    CreateDirAll { source: icp::fs::Error },

    #[snafu(display("failed to remove dir"))]
    RemoveDirAll { source: icp::fs::Error },

    #[snafu(display("failed to remove file"))]
    RemoveFile { source: icp::fs::Error },

    #[snafu(transparent)]
    SaveNetworkDescriptor { source: SaveNetworkDescriptorError },

    #[snafu(transparent)]
    InitPocketIc { source: InitializePocketicError },

    #[snafu(transparent)]
    WaitForPort { source: WaitForPortError },
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

async fn initialize_pocketic(
    pocketic_port: u16,
    gateway_bind_port: &BindPort,
    state_dir: &Path,
    seed_accounts: impl Iterator<Item = Principal> + Clone,
) -> Result<PocketIcInstance, InitializePocketicError> {
    let instance_config = default_instance_config(state_dir);
    let gateway_port = match gateway_bind_port {
        BindPort::Fixed(port) => Some(*port),
        BindPort::Random => None,
    };

    pocketic::initialize_instance(pocketic_port, instance_config, gateway_port, seed_accounts).await
}

#[derive(Debug, Snafu)]
pub enum InitializePocketicError {
    #[snafu(transparent)]
    CreateInstance { source: CreateInstanceError },

    #[snafu(transparent)]
    CreateHttpGateway { source: CreateHttpGatewayError },

    #[snafu(display("no root key reported in status"))]
    NoRootKey,

    #[snafu(transparent)]
    PingAndWait { source: status::PingAndWaitError },

    #[snafu(transparent)]
    Reqwest { source: reqwest::Error },

    #[snafu(display("Failed to seed initial balances: {error}"))]
    SeedTokens { error: String },
}
