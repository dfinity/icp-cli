use std::{env::var, fs::read_to_string, process::ExitStatus, time::Duration};

use candid::{CandidType, Nat, Principal};
use futures::future::{join, join_all};
use ic_ledger_types::{AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs, TransferResult};
use icp::{
    fs::{create_dir_all, remove_dir_all, remove_file},
    prelude::*,
};
use pocket_ic::{
    common::rest::{HttpGatewayBackend, RawEffectivePrincipal},
    nonblocking::{PocketIc, call_candid, call_candid_as},
};
use reqwest::Url;
use serde::Deserialize;
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
            spawn_pocketic,
        },
        run::InitializePocketicError::NoRootKey,
    },
    status,
};

pub const MEMO_MINT_CYCLES: u64 = 0x544e494d; // == 'MINT'

/// 0.0001 ICP, a.k.a. 10k e8s
const ICP_TRANSFER_FEE_E8S: u64 = 10_000;

/// 100m cycles
const CYCLES_LEDGER_BLOCK_FEE: u128 = 100_000_000;

/// ICP ledger on mainnet
pub const ICP_LEDGER_CID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";

/// Governance on mainnet
pub const GOVERNANCE_CID: &str = "rrkah-fqaaa-aaaaa-aaaaq-cai";

/// Cycles minting canister on mainnet
pub const CYCLES_MINTING_CANISTER_CID: &str = "rkp4c-7iaaa-aaaaa-aaaca-cai";

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

/// Waits for the child to populate a port number.
/// Exits early if the child exits or the user interrupts.
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

    eprintln!("Seeding ICP and TCYCLES account balances");
    let pocket_ic_client = PocketIc::new_from_existing_instance(pic_url, instance_id, None);
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

    #[snafu(display("Failed to seed initial balances: {error}"))]
    SeedTokens { error: String },
}

async fn mint_cycles_to_account(
    pic: &PocketIc,
    account: Principal,
    amount: u128,
    icp_xdr_conversion_rate: u64,
) -> Result<(), InitializePocketicError> {
    let icp_to_convert =
        (amount + CYCLES_LEDGER_BLOCK_FEE).div_ceil(icp_xdr_conversion_rate as u128) as u64;
    mint_icp_to_account(pic, account, icp_to_convert + ICP_TRANSFER_FEE_E8S).await?;
    let (transfer_result,): (TransferResult,) = call_candid_as(
        pic,
        Principal::from_text(ICP_LEDGER_CID).unwrap(),
        RawEffectivePrincipal::None,
        account,
        "transfer",
        (TransferArgs {
            memo: Memo(MEMO_MINT_CYCLES),
            amount: Tokens::from_e8s(icp_to_convert),
            fee: Tokens::from_e8s(ICP_TRANSFER_FEE_E8S),
            from_subaccount: None,
            to: AccountIdentifier::new(
                &Principal::from_text(CYCLES_MINTING_CANISTER_CID).unwrap(),
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
        Principal::from_text(CYCLES_MINTING_CANISTER_CID).unwrap(),
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
    pic: &PocketIc,
    account: Principal,
    amount: u64,
) -> Result<(), InitializePocketicError> {
    let response: (TransferResult,) = call_candid_as(
        pic,
        Principal::from_text(ICP_LEDGER_CID).unwrap(),
        RawEffectivePrincipal::None,
        Principal::from_text(GOVERNANCE_CID).unwrap(),
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

async fn get_icp_xdr_conversion_rate(pic: &PocketIc) -> Result<u64, InitializePocketicError> {
    let response: (ConversionRateResponse,) = call_candid(
        pic,
        Principal::from_text(CYCLES_MINTING_CANISTER_CID).unwrap(),
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

/// Response from get_icp_xdr_conversion_rate on the cycles minting canister
#[derive(Debug, Deserialize, CandidType)]
struct ConversionRateResponse {
    pub data: ConversionRateData,
}

#[derive(Debug, Deserialize, CandidType)]
struct ConversionRateData {
    pub xdr_permyriad_per_icp: u64,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintArgs {
    pub block_index: u64,
    pub deposit_memo: Option<Vec<u8>>,
    pub to_subaccount: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintOk {
    pub balance: Nat,
    pub block_index: Nat,
    pub minted: Nat,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintRefunded {
    pub block_index: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintOther {
    pub error_message: String,
    pub error_code: u64,
}

#[derive(Debug, Deserialize, CandidType)]
pub enum NotifyMintErr {
    Refunded(NotifyMintRefunded),
    InvalidTransaction(String),
    Other(NotifyMintOther),
    Processing,
    TransactionTooOld(u64),
}

#[derive(Debug, Deserialize, CandidType)]
pub enum NotifyMintResponse {
    Ok(NotifyMintOk),
    Err(NotifyMintErr),
}
