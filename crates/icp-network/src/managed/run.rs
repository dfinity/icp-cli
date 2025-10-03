use std::{env::var, fs::read_to_string, process::ExitStatus, time::Duration};

use candid::Principal;
use futures::future::{join, join_all};
use icp::{
    fs::{create_dir_all, remove_dir_all, remove_file},
    prelude::*,
};
use pocket_ic::{
    common::rest::{HttpGatewayBackend, InstanceConfig},
    nonblocking::PocketIc,
};
use reqwest::Url;
use snafu::prelude::*;
use tokio::{process::Child, select, signal::ctrl_c, time::sleep};
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
            CreateHttpGatewayError, CreateInstanceError, PocketIcAdminInterface, PocketIcInstance,
            default_instance_config, spawn_pocketic,
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

/// Waits for a port file to be written and contain a valid port number.
/// Returns the port number or an error if timeout occurs.
///
/// This function polls the file every 100ms for up to 5 minutes (3000 retries).
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

/// Waits for a child process to populate a port number in a file.
/// Exits early if the child exits or the user interrupts (Ctrl-C).
///
/// # Arguments
/// * `path` - Path to the port file
/// * `child` - The child process to monitor
///
/// # Returns
/// The port number on success, or an error if interrupted, timeout, or child exits
pub async fn wait_for_port(path: &Path, child: &mut Child) -> Result<u16, WaitForPortError> {
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
    state_dir: &Path,
    seed_accounts: impl Iterator<Item = Principal> + Clone,
) -> Result<PocketIcInstance, InitializePocketicError> {
    let instance_config = default_instance_config(state_dir);
    let gateway_port = match gateway_bind_port {
        BindPort::Fixed(port) => Some(*port),
        BindPort::Random => None,
    };

    initialize_instance(pocketic_port, instance_config, gateway_port, seed_accounts).await
}

/// Initializes a PocketIC instance with the given configuration.
///
/// # Arguments
/// * `pocketic_port` - Port where PocketIC server is running
/// * `instance_config` - Configuration for the PocketIC instance (topology, features, etc.)
/// * `gateway_port` - Optional fixed port for HTTP gateway (None for random)
/// * `seed_accounts` - Accounts to seed with ICP and cycles
pub async fn initialize_instance(
    pocketic_port: u16,
    instance_config: InstanceConfig,
    gateway_port: Option<u16>,
    seed_accounts: impl Iterator<Item = Principal> + Clone,
) -> Result<PocketIcInstance, InitializePocketicError> {
    let pic_url = format!("http://localhost:{pocketic_port}")
        .parse::<Url>()
        .unwrap();
    let pic = PocketIcAdminInterface::new(pic_url.clone());

    eprintln!("Creating PocketIC instance");
    let (instance_id, topology) = pic.create_instance_with_config(instance_config).await?;
    let default_effective_canister_id = topology.default_effective_canister_id;
    eprintln!("Created instance with id {}", instance_id);

    eprintln!("Setting time");
    pic.set_time(instance_id).await?;

    eprintln!("Setting auto-progress");
    let artificial_delay = 600;
    pic.auto_progress(instance_id, artificial_delay).await?;

    eprintln!("Seeding ICP and cycles account balances");
    let pocket_ic_client = PocketIc::new_from_existing_instance(pic_url.clone(), instance_id, None);
    let icp_xdr_conversion_rate = get_icp_xdr_conversion_rate(&pocket_ic_client).await?;
    let seed_icp = join_all(
        seed_accounts
            .clone()
            .filter(|account| *account != Principal::anonymous()) // Anon gets seeded by pocket-ic
            .map(|account| mint_icp_to_account(&pocket_ic_client, account, 100_000_000_000_000u64)),
    );
    let seed_cycles = join_all(seed_accounts.map(|account| {
        mint_cycles_to_account(
            &pocket_ic_client,
            account,
            1_000_000_000_000_000u128, // 1k Trillion cycles
            icp_xdr_conversion_rate,
        )
    }));
    let (seed_icp_results, seed_cycles_results) = join(seed_icp, seed_cycles).await;
    seed_icp_results
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    seed_cycles_results
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

    eprintln!("Creating HTTP gateway");
    let gateway_info = pic
        .create_http_gateway(
            HttpGatewayBackend::PocketIcInstance(instance_id),
            gateway_port,
        )
        .await?;
    eprintln!("Created HTTP gateway on port {}", gateway_info.port);

    let agent_url = format!("http://localhost:{}", gateway_info.port);
    eprintln!("Pinging network at {}", agent_url);
    let status = crate::status::ping_and_wait(&agent_url).await?;

    let root_key = status.root_key.ok_or(InitializePocketicError::NoRootKey)?;
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

    #[snafu(display("Failed to seed initial balances: {error}"))]
    SeedTokens { error: String },
}

async fn mint_cycles_to_account(
    pic: &pocket_ic::nonblocking::PocketIc,
    account: Principal,
    amount: u128,
    icp_xdr_conversion_rate: u64,
) -> Result<(), InitializePocketicError> {
    use ic_ledger_types::{
        AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs, TransferResult,
    };
    use icp_canister_interfaces::{
        cycles_ledger::CYCLES_LEDGER_BLOCK_FEE,
        cycles_minting_canister::{
            CYCLES_MINTING_CANISTER_PRINCIPAL, MEMO_MINT_CYCLES, NotifyMintArgs, NotifyMintResponse,
        },
        icp_ledger::{ICP_LEDGER_BLOCK_FEE_E8S, ICP_LEDGER_PRINCIPAL},
    };
    use pocket_ic::common::rest::RawEffectivePrincipal;
    use pocket_ic::nonblocking::call_candid_as;

    let icp_to_convert =
        (amount + CYCLES_LEDGER_BLOCK_FEE).div_ceil(icp_xdr_conversion_rate as u128) as u64;
    // First mint to the non-CMC account because notify_mint_cycles will fail if the depositing transaction is a mint TX
    mint_icp_to_account(pic, account, icp_to_convert + ICP_LEDGER_BLOCK_FEE_E8S).await?;
    // Then transfer to the CMC account
    let (transfer_result,): (TransferResult,) = call_candid_as(
        pic,
        ICP_LEDGER_PRINCIPAL,
        RawEffectivePrincipal::None,
        account,
        "transfer",
        (TransferArgs {
            memo: Memo(MEMO_MINT_CYCLES),
            amount: Tokens::from_e8s(icp_to_convert),
            fee: Tokens::from_e8s(ICP_LEDGER_BLOCK_FEE_E8S),
            from_subaccount: None,
            to: AccountIdentifier::new(
                &CYCLES_MINTING_CANISTER_PRINCIPAL,
                &Subaccount::from(account),
            ),
            created_at_time: None,
        },),
    )
    .await
    .map_err(|err| InitializePocketicError::SeedTokens {
        error: format!("Failed to decode transfer ICP to CMC response: {err}"),
    })?;
    let block_index = transfer_result.map_err(|err| InitializePocketicError::SeedTokens {
        error: format!("Failed to transfer ICP to CMC: {err}"),
    })?;

    let mint_result: (NotifyMintResponse,) = call_candid_as(
        pic,
        CYCLES_MINTING_CANISTER_PRINCIPAL,
        RawEffectivePrincipal::None,
        account,
        "notify_mint_cycles",
        (NotifyMintArgs {
            block_index,
            deposit_memo: None,
            to_subaccount: None,
        },),
    )
    .await
    .map_err(|err| InitializePocketicError::SeedTokens {
        error: format!("Failed to decode notify mint cycles response: {err}"),
    })?;
    if let NotifyMintResponse::Err(err) = mint_result.0 {
        eprintln!("Failed to notify mint cycles: {err:?}");
        return Err(InitializePocketicError::SeedTokens {
            error: format!("Failed to notify mint cycles: {err:?}"),
        });
    }

    if let NotifyMintResponse::Ok(ok) = mint_result.0 {
        eprintln!("Minted {} cycles to account {}", ok.minted, account);
    }

    Ok(())
}

async fn mint_icp_to_account(
    pic: &pocket_ic::nonblocking::PocketIc,
    account: Principal,
    amount: u64,
) -> Result<(), InitializePocketicError> {
    use ic_ledger_types::{
        AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs, TransferResult,
    };
    use icp_canister_interfaces::{
        governance::GOVERNANCE_PRINCIPAL, icp_ledger::ICP_LEDGER_PRINCIPAL,
    };
    use pocket_ic::common::rest::RawEffectivePrincipal;
    use pocket_ic::nonblocking::call_candid_as;

    let response: (TransferResult,) = call_candid_as(
        pic,
        ICP_LEDGER_PRINCIPAL,
        RawEffectivePrincipal::None,
        GOVERNANCE_PRINCIPAL, // Governance with no subaccount is configured as the minter on the ICP ledger
        "transfer",
        (TransferArgs {
            memo: Memo(0),
            amount: Tokens::from_e8s(amount),
            fee: Tokens::from_e8s(0), // mints are free
            from_subaccount: None,
            to: AccountIdentifier::new(&account, &Subaccount([0u8; 32])),
            created_at_time: None,
        },),
    )
    .await
    .map_err(|err| InitializePocketicError::SeedTokens {
        error: format!("Failed to decode ICP mint response: {err}"),
    })?;
    response
        .0
        .map_err(|err| InitializePocketicError::SeedTokens {
            error: format!("Failed to mint ICP: {err}"),
        })?;
    eprintln!("Minted {} ICP to account {}", amount, account);
    Ok(())
}

async fn get_icp_xdr_conversion_rate(
    pic: &pocket_ic::nonblocking::PocketIc,
) -> Result<u64, InitializePocketicError> {
    use icp_canister_interfaces::cycles_minting_canister::{
        CYCLES_MINTING_CANISTER_PRINCIPAL, ConversionRateResponse,
    };
    use pocket_ic::common::rest::RawEffectivePrincipal;
    use pocket_ic::nonblocking::call_candid;

    let response: (ConversionRateResponse,) = call_candid(
        pic,
        CYCLES_MINTING_CANISTER_PRINCIPAL,
        RawEffectivePrincipal::None,
        "get_icp_xdr_conversion_rate",
        ((),),
    )
    .await
    .map_err(|e| InitializePocketicError::SeedTokens {
        error: format!("Failed to get ICP XDR conversion rate: {e}"),
    })?;
    Ok(response.0.data.xdr_permyriad_per_icp)
}
