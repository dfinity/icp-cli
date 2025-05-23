use crate::config::model::managed::BindPort::Fixed;
use crate::config::model::managed::{BindPort, ManagedNetworkModel};
use crate::config::model::network_descriptor::NetworkDescriptorModel;
use crate::status;
use crate::structure::NetworkDirectoryStructure;
use candid::Principal;
use fd_lock::RwLock;
use icp_support::fs::{CreateDirAllError, RemoveFileError, WriteFileError, create_dir_all, remove_file, write, remove_dir_all};
use icp_support::json::{LoadJsonFileError, SaveJsonFileError, save_json_file};
use icp_support::process::process_running;
use pocket_ic::common::rest::{
    CreateHttpGatewayResponse, HttpGatewayBackend, HttpGatewayConfig, HttpGatewayInfo,
};
use snafu::prelude::*;
use std::fs::{OpenOptions, read_to_string};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;
use tokio::process::Child;
use tokio::select;
use tokio::signal::ctrl_c;
use tokio::time::sleep;
use crate::pocketic::native::spawn_pocketic;

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

    let pocketic_path =
        PathBuf::from("/Users/ericswanson/.cache/dfinity/versions/0.26.1/pocket-ic");

    run_pocketic(&pocketic_path, config, nds).await;
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

#[derive(Debug, Snafu)]
pub enum RunPocketIcError {
    #[snafu(transparent)]
    CreateDirAll { source: CreateDirAllError },

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

    create_dir_all(&nds.pocketic_dir()).unwrap();
    let port_file = nds.pocketic_port_file();
    if port_file.exists() {
        remove_file(&port_file).unwrap();
    }
    eprintln!("Port file: {}", port_file.display());
    remove_dir_all(&nds.state_dir()).unwrap();
    create_dir_all(&nds.state_dir())?;
    let mut child = spawn_pocketic(&pocketic_path, &config.bind.port, &port_file);

    let result = async {
        let port = wait_for_port(&port_file, &mut child).await?;
        eprintln!("PocketIC started on port {port}");
        let _props = initialize_pocketic(port, &nds.state_dir()).await?;
        // TODO: write network descriptor
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
    #[snafu(display("Failed to create PocketIC instance: {message}"))]
    CreateInstance { message: String },

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
    // bitcoind_addr: &Option<Vec<SocketAddr>>,
    // bitcoin_integration_config: &Option<BitcoinIntegrationConfig>,
    // replica_config: &ReplicaConfig,
    // logger: Logger,
) -> Result<PocketIcInstanceProperties, InitializePocketicError> {
    eprintln!("Initializing PocketIC instance");
    let artificial_delay = 600;
    //use dfx_core::config::model::dfinity::ReplicaSubnetType;
    use pocket_ic::common::rest::{
        AutoProgressConfig, CreateInstanceResponse, ExtendedSubnetConfigSet, InstanceConfig,
        RawTime, SubnetSpec,
    };
    use reqwest::Client;
    use time::OffsetDateTime;
    let init_client = Client::new();
    // debug!(logger, "Configuring PocketIC server");
    let mut subnet_config_set = ExtendedSubnetConfigSet {
        nns: Some(SubnetSpec::default()),
        sns: Some(SubnetSpec::default()),
        ii: Some(SubnetSpec::default()),
        fiduciary: Some(SubnetSpec::default()),
        bitcoin: Some(SubnetSpec::default()),
        system: vec![],
        verified_application: vec![],
        application: vec![<_>::default()],
    };
    // match replica_config.subnet_type {
    //     ReplicaSubnetType::Application => subnet_config_set.application.push(<_>::default()),
    //     ReplicaSubnetType::System => subnet_config_set.system.push(<_>::default()),
    //     ReplicaSubnetType::VerifiedApplication => {
    //         subnet_config_set.verified_application.push(<_>::default())
    //     }
    // }
    eprintln!("Creating instance");
    let resp = init_client
        .post(format!("http://localhost:{port}/instances"))
        .json(&InstanceConfig {
            subnet_config_set,
            state_dir: Some(state_dir.to_path_buf()),
            nonmainnet_features: true,
            log_level: Some("ERROR".to_string()),
            bitcoind_addr: None, // bitcoind_addr.clone(),
        })
        .send()
        .await?
        .error_for_status()?
        .json::<CreateInstanceResponse>()
        .await?;
    let (instance_id, default_effective_canister_id) = match resp {
        CreateInstanceResponse::Error { message } => {
            return Err(InitializePocketicError::CreateInstance { message });
        }
        CreateInstanceResponse::Created {
            instance_id,
            topology,
        } => {
            let default_effective_canister_id: Principal =
                topology.default_effective_canister_id.into();

            (instance_id, default_effective_canister_id)
        }
    };
    eprintln!("Created instance with id {}", instance_id);
    eprintln!("Setting time");
    init_client
        .post(format!(
            "http://localhost:{port}/instances/{instance_id}/update/set_time"
        ))
        .json(&RawTime {
            nanos_since_epoch: OffsetDateTime::now_utc()
                .unix_timestamp_nanos()
                .try_into()
                .unwrap(),
        })
        .send()
        .await?
        .error_for_status()?;
    eprintln!("Set auto-progress");
    init_client
        .post(format!(
            "http://localhost:{port}/instances/{instance_id}/auto_progress"
        ))
        .json(&AutoProgressConfig {
            artificial_delay_ms: Some(artificial_delay as u64),
        })
        .send()
        .await?
        .error_for_status()?;
    let resp = init_client
        .post(format!("http://localhost:{port}/http_gateway"))
        .json(&HttpGatewayConfig {
            ip_addr: None,
            port: Some(8000),
            forward_to: HttpGatewayBackend::PocketIcInstance(instance_id),
            domains: None,
            https_config: None,
        })
        .send()
        .await?
        .error_for_status()?
        .json::<CreateHttpGatewayResponse>()
        .await?;

    match resp {
        CreateHttpGatewayResponse::Error { message } => {
            return Err(InitializePocketicError::CreateInstance { message });
        }
        CreateHttpGatewayResponse::Created(HttpGatewayInfo { instance_id, port }) => {
            eprintln!("Created HTTP gateway instance={instance_id} port={port}");
        }
    }

    let agent_url = format!("http://localhost:{port}/instances/{instance_id}/");

    eprintln!("Agent url is {}", agent_url);

    // debug!(logger, "Waiting for replica to report healthy status");
    status::ping_and_wait(&agent_url).await?;

    // todo
    // if let Some(bitcoin_integration_config) = bitcoin_integration_config {
    //     let agent = create_integrations_agent(&agent_url, &logger).await?;
    //     initialize_bitcoin_canister(&agent, &logger, bitcoin_integration_config.clone()).await?;
    // }

    // debug!(logger, "Initialized PocketIC.");
    let props = PocketIcInstanceProperties {
        instance_id,
        effective_canister_id: default_effective_canister_id,
        root_key: "".to_string(),
    };
    Ok(props)
}
