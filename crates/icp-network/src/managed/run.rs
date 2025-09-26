use std::{env::var, fs::read_to_string, process::ExitStatus, time::Duration};

use camino::{Utf8Path, Utf8PathBuf};
use candid::{Encode, Nat, Principal};
use futures::future::join_all;
use icp_fs::{
    fs::{
        CreateDirAllError, RemoveDirAllError, RemoveFileError, create_dir_all, remove_dir_all,
        remove_file,
    },
    lock::OpenFileForWriteLockError,
};
use icrc_ledger_types::icrc1::{account::Account, transfer::TransferArg};
use pocket_ic::{common::rest::HttpGatewayBackend, nonblocking::PocketIc};
use reqwest::Url;
use snafu::prelude::*;
use tokio::{process::Child, select, signal::ctrl_c, time::sleep};
use uuid::Uuid;

use crate::{
    BindPort, ManagedNetworkModel, NetworkDirectory,
    RunNetworkError::NoPocketIcPath,
    config::{NetworkDescriptorGatewayPort, NetworkDescriptorModel},
    directory::SaveNetworkDescriptorError,
    managed::{
        descriptor::{AnotherProjectRunningOnSamePortError, ProjectNetworkAlreadyRunningError},
        pocketic::{
            CreateHttpGatewayError, CreateInstanceError, PocketIcAdminInterface, PocketIcInstance,
            spawn_pocketic,
        },
        run::InitializePocketicError::NoRootKey,
    },
    status,
};

/// ICP ledger on mainnet
pub const ICP_LEDGER_CID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";

pub async fn run_network(
    config: &ManagedNetworkModel,
    nd: NetworkDirectory,
    project_root: &Utf8Path,
    seed_accounts: impl Iterator<Item = Principal>,
) -> Result<(), RunNetworkError> {
    let pocketic_path = Utf8PathBuf::from(var("ICP_POCKET_IC_PATH").ok().ok_or(NoPocketIcPath)?);

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
    CreateDirFailed { source: CreateDirAllError },

    #[snafu(display("ICP_POCKET_IC_PATH environment variable is not set"))]
    NoPocketIcPath,

    #[snafu(transparent)]
    OpenFileForWriteLock { source: OpenFileForWriteLockError },

    #[snafu(transparent)]
    RunPocketIc { source: RunPocketIcError },
}

async fn run_pocketic(
    pocketic_path: &Utf8Path,
    config: &ManagedNetworkModel,
    nd: &NetworkDirectory,
    project_root: &Utf8Path,
    seed_accounts: impl Iterator<Item = Principal>,
) -> Result<(), RunPocketIcError> {
    let nds = nd.structure();
    eprintln!("PocketIC path: {pocketic_path}");

    create_dir_all(nds.pocketic_dir())?;
    let port_file = nds.pocketic_port_file();
    if port_file.exists() {
        remove_file(&port_file)?;
    }
    eprintln!("Port file: {port_file}");
    if nds.state_dir().exists() {
        remove_dir_all(nds.state_dir())?;
    }
    create_dir_all(nds.state_dir())?;
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
    #[snafu(transparent)]
    CreateDirAll { source: CreateDirAllError },

    #[snafu(transparent)]
    RemoveDirAll { source: RemoveDirAllError },

    #[snafu(transparent)]
    RemoveFile { source: RemoveFileError },

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

pub async fn wait_for_port_file(path: &Utf8Path) -> Result<u16, WaitForPortTimeoutError> {
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
#[snafu(display("timeout waiting for port file"))]
pub struct WaitForPortTimeoutError;

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
#[snafu(display("Child process exited early with status {status}"))]
pub struct ChildExitError {
    pub status: ExitStatus,
}

/// Waits for the child to populate a port number.
/// Exits early if the child exits or the user interrupts.
pub async fn wait_for_port(path: &Utf8Path, child: &mut Child) -> Result<u16, WaitForPortError> {
    tokio::select! {
        res = wait_for_port_file(path) => res.map_err(WaitForPortError::from),
        _ = ctrl_c() => Err(WaitForPortError::Interrupted),
        err = notice_child_exit(child) => Err(WaitForPortError::ChildExited { source: err }),
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

async fn initialize_pocketic(
    pocketic_port: u16,
    gateway_bind_port: &BindPort,
    state_dir: &Utf8Path,
    seed_accounts: impl Iterator<Item = Principal>,
) -> Result<PocketIcInstance, InitializePocketicError> {
    let pic_url = format!("http://localhost:{pocketic_port}")
        .parse::<Url>()
        .unwrap();
    let pic = PocketIcAdminInterface::new(pic_url.clone());

    eprintln!("Initializing PocketIC instance");

    eprintln!("Creating instance");
    let (instance_id, topology) = pic.create_instance(state_dir.as_ref()).await?;
    let default_effective_canister_id = topology.default_effective_canister_id;
    eprintln!("Created instance with id {}", instance_id);

    eprintln!("Setting time");
    pic.set_time(instance_id).await?;

    eprintln!("Set auto-progress");
    let artificial_delay = 600;
    pic.auto_progress(instance_id, artificial_delay).await?;

    eprintln!("Seeding ICP account balances...");
    let pocket_ic_client = PocketIc::new_from_existing_instance(pic_url, instance_id, None);
    join_all(seed_accounts.map(|account| {
        pocket_ic_client.update_call(
            Principal::from_text(ICP_LEDGER_CID).unwrap(),
            Principal::anonymous(),
            "icrc1_transfer",
            Encode!(&TransferArg {
                to: Account {
                    owner: account,
                    subaccount: None,
                },
                from_subaccount: None,
                fee: None,
                created_at_time: None,
                memo: None,
                amount: Nat::from(100_000_000_000_000u64),
            })
            .expect("Failed to encode transfer arg"),
        )
    }))
    .await;

    let gateway_port = match gateway_bind_port {
        BindPort::Fixed(port) => Some(*port),
        BindPort::Random => None,
    };
    let gateway_info = pic
        .create_http_gateway(
            HttpGatewayBackend::PocketIcInstance(instance_id),
            gateway_port,
        )
        .await?;
    eprintln!(
        "Created HTTP gateway instance={} port={}",
        gateway_info.instance_id, gateway_info.port
    );

    let agent_url = format!("http://localhost:{}", gateway_info.port);

    eprintln!("Agent url is {}", agent_url);
    let status = status::ping_and_wait(&agent_url).await?;

    let root_key = status.root_key.ok_or(NoRootKey)?;
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
}
