use async_dropper::{AsyncDrop, AsyncDropper};
use bigdecimal::BigDecimal;
use candid::{Decode, Encode, Nat, Principal};
use futures::future::{join, join_all};
use ic_agent::{
    Agent, AgentError, Identity,
    agent::status::Status,
    identity::{AnonymousIdentity, Secp256k1Identity},
};
use ic_ledger_types::{AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs, TransferResult};
use icp_canister_interfaces::{
    cycles_ledger::{CYCLES_LEDGER_BLOCK_FEE, CYCLES_LEDGER_PRINCIPAL},
    cycles_minting_canister::{
        CYCLES_MINTING_CANISTER_PRINCIPAL, ConversionRateResponse, MEMO_MINT_CYCLES,
        NotifyMintArgs, NotifyMintResponse,
    },
    icp_ledger::{ICP_LEDGER_BLOCK_FEE_E8S, ICP_LEDGER_PRINCIPAL},
};
use icrc_ledger_types::icrc1::{
    account::Account,
    transfer::{TransferArg, TransferError},
};
use k256::SecretKey;
use rand::{RngCore, rng};
use snafu::prelude::*;
use std::{env::var, io::Write, process::ExitStatus, time::Duration};
use tokio::{process::Child, select, signal::ctrl_c, time::sleep};
use url::Url;
use uuid::Uuid;

use crate::{
    fs::{create_dir_all, lock::LockError, remove_dir_all},
    network::{
        Gateway, Managed, ManagedMode, NetworkDirectory, Port,
        config::{ChildLocator, NetworkDescriptorGatewayPort, NetworkDescriptorModel},
        directory::{ClaimPortError, SaveNetworkDescriptorError, save_network_descriptors},
        managed::{
            docker::{DockerDropGuard, spawn_docker_launcher},
            launcher::{ChildSignalOnDrop, CreateHttpGatewayError, spawn_network_launcher},
        },
    },
    prelude::*,
};

pub async fn run_network(
    config: &Managed,
    nd: NetworkDirectory,
    project_root: &Path,
    seed_accounts: impl Iterator<Item = Principal> + Clone,
    background: bool,
) -> Result<(), RunNetworkError> {
    nd.ensure_exists()?;

    let network_launcher_path = var("ICP_CLI_NETWORK_LAUNCHER_PATH").ok().map(PathBuf::from);

    run_network_launcher(
        network_launcher_path.as_deref(),
        config,
        &nd,
        project_root,
        seed_accounts,
        background,
    )
    .await?;
    Ok(())
}

pub async fn stop_network(locator: &ChildLocator) -> Result<(), StopNetworkError> {
    match locator {
        ChildLocator::Pid { pid } => {
            super::launcher::stop_launcher((*pid as usize).into()).await;
        }
        ChildLocator::Container {
            id,
            socket,
            rm_on_exit,
        } => {
            super::docker::stop_docker_launcher(socket, id, *rm_on_exit).await?;
        }
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum RunNetworkError {
    #[snafu(transparent)]
    CreateDirFailed { source: crate::fs::IoError },

    #[snafu(transparent)]
    LockFileError { source: LockError },

    #[snafu(transparent)]
    RunNetworkLauncher { source: RunNetworkLauncherError },
}

#[derive(Debug, Snafu)]
pub enum StopNetworkError {
    #[snafu(transparent)]
    DockerLauncher {
        source: super::docker::StopContainerError,
    },
}

async fn run_network_launcher(
    network_launcher_path: Option<&Path>,
    config: &Managed,
    nd: &NetworkDirectory,
    project_root: &Path,
    seed_accounts: impl Iterator<Item = Principal> + Clone,
    background: bool,
) -> Result<(), RunNetworkLauncherError> {
    let network_root = nd.root()?;
    // hold port_claim until the end of this function
    let (mut guard, port, _port_claim) = network_root
        .with_write(async |root| -> Result<_, RunNetworkLauncherError> {
            let port_lock = if let ManagedMode::Launcher {
                gateway:
                    Gateway {
                        port: Port::Fixed(port),
                        ..
                    },
            } = &config.mode
            {
                Some(nd.port(*port)?.into_write().await?)
            } else {
                None
            };
            let port_claim = port_lock
                .as_ref()
                .map(|lock| lock.claim_port())
                .transpose()?;

            create_dir_all(&root.launcher_dir()).context(CreateDirAllSnafu)?;

            if root.state_dir().exists() {
                remove_dir_all(&root.state_dir()).context(RemoveDirAllSnafu)?;
            }
            create_dir_all(&root.state_dir()).context(CreateDirAllSnafu)?;

            let (guard, instance, gateway, locator) = match &config.mode {
                ManagedMode::Image(image_config) => {
                    let (guard, instance, locator) = spawn_docker_launcher(image_config).await?;
                    let gateway = NetworkDescriptorGatewayPort {
                        port: instance.gateway_port,
                        fixed: false,
                    };
                    (ShutdownGuard::Container(guard), instance, gateway, locator)
                }
                ManagedMode::Launcher { gateway } => {
                    if root.state_dir().exists() {
                        remove_dir_all(&root.state_dir()).context(RemoveDirAllSnafu)?;
                    }
                    create_dir_all(&root.state_dir()).context(CreateDirAllSnafu)?;
                    let network_launcher_path =
                        network_launcher_path.context(NoNetworkLauncherPathSnafu)?;
                    eprintln!("Network launcher path: {network_launcher_path}");
                    let (child, instance, locator) = spawn_network_launcher(
                        network_launcher_path,
                        &root.network_stdout_file(),
                        &root.network_stderr_file(),
                        background,
                        &gateway.port,
                        &root.state_dir(),
                    )
                    .await?;
                    if background {
                        // background means we're using stdio files - otherwise the launcher already prints this
                        eprintln!("Network started on port {}", instance.gateway_port);
                    }
                    let gateway = NetworkDescriptorGatewayPort {
                        port: instance.gateway_port,
                        fixed: matches!(gateway.port, Port::Fixed(_)),
                    };
                    (ShutdownGuard::Process(child), instance, gateway, locator)
                }
            };
            seed_instance(
                &format!("http://localhost:{}", instance.gateway_port)
                    .parse()
                    .unwrap(),
                &instance.root_key,
                seed_accounts,
            )
            .await?;
            let descriptor = NetworkDescriptorModel {
                v: "1".to_string(),
                id: Uuid::new_v4(),
                project_dir: project_root.to_path_buf(),
                network: nd.network_name.to_owned(),
                network_dir: root.root_dir().to_path_buf(),
                gateway,
                child_locator: locator.clone(),
                root_key: instance.root_key,
                pocketic_config_port: instance.pocketic_config_port,
                pocketic_instance_id: instance.pocketic_instance_id,
            };

            save_network_descriptors(
                root,
                port_lock.as_ref().map(|lock| lock.as_ref()),
                &descriptor,
            )
            .await?;
            Ok((guard, instance.gateway_port, port_claim))
        })
        .await??;
    if background {
        eprintln!("To stop the network, run `icp network stop`");
        guard.defuse();
    } else {
        eprintln!("Press Ctrl-C to exit.");

        let _ = wait_for_shutdown(&mut guard).await;
        guard.async_drop().await;

        let _ = nd.cleanup_project_network_descriptor().await;
        let _ = nd.cleanup_port_descriptor(Some(port)).await;
    }
    Ok(())
}

enum ShutdownGuard {
    Container(AsyncDropper<DockerDropGuard>),
    Process(AsyncDropper<ChildSignalOnDrop>),
}

impl ShutdownGuard {
    async fn async_drop(self) {
        match self {
            ShutdownGuard::Container(mut guard) => guard.async_drop().await,
            ShutdownGuard::Process(mut guard) => guard.async_drop().await,
        }
    }
    fn defuse(self) {
        match self {
            ShutdownGuard::Container(mut guard) => guard.defuse(),
            ShutdownGuard::Process(mut guard) => guard.defuse(),
        }
    }
}

#[derive(Debug, Snafu)]
pub enum RunNetworkLauncherError {
    #[snafu(display("ICP_CLI_NETWORK_LAUNCHER_PATH environment variable is not set"))]
    NoNetworkLauncherPath,

    #[snafu(display("failed to create dir"))]
    CreateDirAll { source: crate::fs::IoError },

    #[snafu(display("failed to remove dir"))]
    RemoveDirAll { source: crate::fs::IoError },

    #[snafu(display("failed to remove file"))]
    RemoveFile { source: crate::fs::IoError },

    #[snafu(transparent)]
    SaveNetworkDescriptor { source: SaveNetworkDescriptorError },

    #[snafu(transparent)]
    InitNetwork { source: InitializeNetworkError },

    #[snafu(transparent)]
    WaitForPort { source: WaitForPortError },

    #[snafu(transparent)]
    LockFile { source: LockError },

    #[snafu(transparent)]
    ClaimPort { source: ClaimPortError },

    #[snafu(transparent)]
    SavePid {
        source: crate::network::directory::SavePidError,
    },

    #[snafu(transparent)]
    SpawnLauncher {
        source: crate::network::managed::launcher::SpawnNetworkLauncherError,
    },

    #[snafu(transparent)]
    SpawnDockerLauncher {
        source: crate::network::managed::docker::DockerLauncherError,
    },
}

#[derive(Debug)]
pub enum ShutdownReason {
    CtrlC,
    ChildExited,
}

/// Write to stderr, ignoring any errors. This is safe to use even when stderr is closed
/// (e.g., in a background process after the parent exits), unlike eprintln! which panics.
fn safe_eprintln(msg: &str) {
    let _ = std::io::stderr().write_all(msg.as_bytes());
    let _ = std::io::stderr().write_all(b"\n");
}

async fn wait_for_shutdown(guard: &mut ShutdownGuard) -> ShutdownReason {
    match guard {
        ShutdownGuard::Container(_) => {
            _ = ctrl_c().await;
            safe_eprintln("Received Ctrl-C, shutting down PocketIC...");
            ShutdownReason::CtrlC
        }
        ShutdownGuard::Process(child) => {
            select!(
                _ = ctrl_c() => {
                    safe_eprintln("Received Ctrl-C, shutting down PocketIC...");
                    ShutdownReason::CtrlC
                }
                res = notice_child_exit(child.child.as_mut().unwrap()) => {
                    safe_eprintln(&format!("PocketIC exited with status: {:?}", res.status));
                    ShutdownReason::ChildExited
                }
            )
        }
    }
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
#[snafu(display("Child process exited early with status {status}"))]
pub struct ChildExitError {
    pub status: ExitStatus,
}

#[derive(Debug, Snafu)]
pub enum WaitForPortError {
    #[snafu(display("Interrupted"))]
    Interrupted,
    #[snafu(transparent)]
    ChildExited { source: ChildExitError },
}

pub async fn seed_instance(
    gateway_url: &Url,
    root_key: &[u8],
    seed_accounts: impl IntoIterator<Item = Principal> + Clone,
) -> Result<(), InitializeNetworkError> {
    eprintln!("Seeding ICP and TCYCLES account balances");
    let agent = Agent::builder()
        .with_url(gateway_url.as_str())
        .with_identity(AnonymousIdentity)
        .build()
        .context(BuildAgentSnafu {
            url: gateway_url.as_str(),
        })?;
    agent.set_root_key(root_key.to_vec());
    let icp_xdr_conversion_rate = get_icp_xdr_conversion_rate(&agent).await?;
    let seed_icp = join_all(
        seed_accounts
            .clone()
            .into_iter()
            .filter(|account| *account != Principal::anonymous()) // Anon gets seeded by pocket-ic (or whatever the launcher is doing)
            .map(|account| acquire_icp_to_account(&agent, account, 100_000_000_000_000u64)),
    );
    let seed_cycles = join_all(seed_accounts.into_iter().map(|account| {
        mint_cycles_to_account(
            &agent,
            account,
            1_000_000_000_000_000u128, // 1k TCYCLES
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
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum InitializeNetworkError {
    #[snafu(transparent)]
    CreateHttpGateway { source: CreateHttpGatewayError },

    #[snafu(display("no root key reported in status"))]
    NoRootKey,

    #[snafu(transparent)]
    PingAndWait { source: PingAndWaitError },

    #[snafu(transparent)]
    Reqwest { source: reqwest::Error },

    #[snafu(display("Failed to seed initial balances: {error}"))]
    SeedTokens { error: String },
}

async fn mint_cycles_to_account(
    agent: &Agent,
    account: Principal,
    amount: u128,
    icp_xdr_conversion_rate: u64,
) -> Result<(), InitializeNetworkError> {
    // First withdraw to a different account because notify_mint_cycles will fail if the depositing transaction is a mint TX
    let mut tmp_key = [0_u8; 32];
    rng().fill_bytes(&mut tmp_key);
    let tmp_identity =
        Secp256k1Identity::from_private_key(SecretKey::from_bytes(&tmp_key.into()).unwrap());
    // one ICP ledger fee to acquire, one to transfer to CMC,
    // one cycles ledger fee to mint, one to transfer back
    let icp_to_convert =
        (amount + CYCLES_LEDGER_BLOCK_FEE * 2).div_ceil(icp_xdr_conversion_rate as u128) as u64;
    acquire_icp_to_account(
        agent,
        tmp_identity.sender().unwrap(),
        icp_to_convert + ICP_LEDGER_BLOCK_FEE_E8S * 2,
    )
    .await?;
    // Then transfer to the CMC account
    let mut tmp_agent = agent.clone();
    tmp_agent.set_identity(tmp_identity.clone());
    let transfer_result = tmp_agent
        .update(&ICP_LEDGER_PRINCIPAL, "transfer")
        .with_arg(
            Encode!(&TransferArgs {
                memo: Memo(MEMO_MINT_CYCLES),
                amount: Tokens::from_e8s(icp_to_convert),
                fee: Tokens::from_e8s(ICP_LEDGER_BLOCK_FEE_E8S),
                from_subaccount: None,
                to: AccountIdentifier::new(
                    &CYCLES_MINTING_CANISTER_PRINCIPAL,
                    &Subaccount::from(tmp_identity.sender().unwrap()),
                ),
                created_at_time: None,
            })
            .unwrap(),
        )
        .await
        .map_err(|err| InitializeNetworkError::SeedTokens {
            error: format!("Failed to send transfer ICP to CMC request: {err}"),
        })?;
    let transfer_result = Decode!(&transfer_result, TransferResult).map_err(|err| {
        InitializeNetworkError::SeedTokens {
            error: format!("Failed to decode transfer ICP to CMC response: {err}"),
        }
    })?;
    let block_index = transfer_result.map_err(|err| InitializeNetworkError::SeedTokens {
        error: format!("Failed to transfer ICP to CMC: {err}"),
    })?;

    let mint_result = tmp_agent
        .update(&CYCLES_MINTING_CANISTER_PRINCIPAL, "notify_mint_cycles")
        .with_arg(
            Encode!(&NotifyMintArgs {
                block_index,
                deposit_memo: None,
                to_subaccount: None,
            })
            .unwrap(),
        )
        .await
        .map_err(|err| InitializeNetworkError::SeedTokens {
            error: format!("Failed to send notify mint cycles request: {err}"),
        })?;
    let mint_result = Decode!(&mint_result, NotifyMintResponse).map_err(|err| {
        InitializeNetworkError::SeedTokens {
            error: format!("Failed to decode notify mint cycles response: {err}"),
        }
    })?;
    if let NotifyMintResponse::Err(err) = mint_result {
        return SeedTokensSnafu {
            error: format!("Failed to notify mint cycles: {err:?}"),
        }
        .fail();
    }
    let response = tmp_agent
        .update(&CYCLES_LEDGER_PRINCIPAL, "icrc1_transfer")
        .with_arg(
            Encode!(&TransferArg {
                to: Account {
                    owner: account,
                    subaccount: None
                },
                amount: amount.into(),
                memo: None,
                fee: Some(CYCLES_LEDGER_BLOCK_FEE.into()),
                from_subaccount: None,
                created_at_time: None,
            })
            .unwrap(),
        )
        .await
        .map_err(|err| InitializeNetworkError::SeedTokens {
            error: format!("Failed to send cycles ledger transfer request: {err}"),
        })?;

    let response = Decode!(&response, Result<Nat, TransferError>).map_err(|err| {
        InitializeNetworkError::SeedTokens {
            error: format!("Failed to decode cycles ledger transfer response: {err}"),
        }
    })?;
    response.map_err(|err| InitializeNetworkError::SeedTokens {
        error: format!("Failed to transfer cycles: {err}"),
    })?;
    Ok(())
}

async fn acquire_icp_to_account(
    agent: &Agent,
    account: Principal,
    amount: u64,
) -> Result<(), InitializeNetworkError> {
    let response = agent
        .update(&ICP_LEDGER_PRINCIPAL, "transfer")
        .with_arg(
            Encode!(&TransferArgs {
                memo: Memo(0),
                amount: Tokens::from_e8s(amount),
                fee: Tokens::from_e8s(ICP_LEDGER_BLOCK_FEE_E8S),
                from_subaccount: None,
                to: AccountIdentifier::new(&account, &Subaccount([0u8; 32])),
                created_at_time: None,
            })
            .unwrap(),
        )
        .await
        .map_err(|err| InitializeNetworkError::SeedTokens {
            error: format!("Failed to send ICP transfer request: {err}"),
        })?;
    let response =
        Decode!(&response, TransferResult).map_err(|err| InitializeNetworkError::SeedTokens {
            error: format!("Failed to decode ICP transfer response: {err}"),
        })?;
    response.map_err(|err| InitializeNetworkError::SeedTokens {
        error: format!("Failed to transfer ICP: {err}"),
    })?;
    let display_amount = BigDecimal::new(amount.into(), 8).normalized();
    eprintln!("Minted {display_amount} ICP to account {account}");
    Ok(())
}

async fn get_icp_xdr_conversion_rate(agent: &Agent) -> Result<u64, InitializeNetworkError> {
    let response = agent
        .update(
            &CYCLES_MINTING_CANISTER_PRINCIPAL,
            "get_icp_xdr_conversion_rate",
        )
        .with_arg(Encode!().unwrap())
        .await
        .map_err(|e| InitializeNetworkError::SeedTokens {
            error: format!("Failed to get ICP XDR conversion rate: {e}"),
        })?;
    let response = Decode!(&response, ConversionRateResponse).map_err(|e| {
        InitializeNetworkError::SeedTokens {
            error: format!("Failed to decode ICP XDR conversion rate response: {e}"),
        }
    })?;
    Ok(response.data.xdr_permyriad_per_icp)
}

pub async fn ping_and_wait(url: &str) -> Result<Status, PingAndWaitError> {
    let agent = Agent::builder()
        .with_url(url)
        .build()
        .context(BuildAgentSnafu { url })?;

    let mut retries = 0;

    loop {
        let status = agent.status().await;
        match status {
            Ok(status) => {
                if matches!(&status.replica_health_status, Some(status) if status == "healthy") {
                    break Ok(status);
                }
            }

            Err(e) => {
                if retries >= 60 {
                    break Err(PingAndWaitError::Timeout { source: e });
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
                retries += 1;
            }
        }
    }
}

#[derive(Debug, Snafu)]
pub enum PingAndWaitError {
    #[snafu(display("failed to build agent for url {}", url))]
    BuildAgent {
        source: AgentError,
        url: String,
    },

    Timeout {
        source: AgentError,
    },
}
