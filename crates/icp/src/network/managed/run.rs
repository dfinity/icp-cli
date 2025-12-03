use candid::Principal;
use futures::future::{join, join_all};
use ic_agent::{Agent, AgentError, agent::status::Status};
use ic_ledger_types::{AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs, TransferResult};
use icp_canister_interfaces::{
    cycles_ledger::CYCLES_LEDGER_BLOCK_FEE,
    cycles_minting_canister::{
        CYCLES_MINTING_CANISTER_PRINCIPAL, ConversionRateResponse, MEMO_MINT_CYCLES,
        NotifyMintArgs, NotifyMintResponse,
    },
    governance::GOVERNANCE_PRINCIPAL,
    icp_ledger::{ICP_LEDGER_BLOCK_FEE_E8S, ICP_LEDGER_PRINCIPAL},
};
use pocket_ic::{
    common::rest::{HttpGatewayBackend, InstanceConfig, RawEffectivePrincipal},
    nonblocking::{PocketIc, call_candid, call_candid_as},
};
use reqwest::Url;
use snafu::prelude::*;
use std::{env::var, fs::read_to_string, io::Write, process::ExitStatus, time::Duration};
use sysinfo::{Pid, ProcessesToUpdate, Signal, System};
use tokio::{process::Child, select, signal::ctrl_c, time::sleep};
use uuid::Uuid;

use crate::{
    fs::{create_dir_all, lock::LockError, remove_dir_all},
    network::{
        Managed, NetworkDirectory, Port,
        RunNetworkError::NoPocketIcPath,
        config::{NetworkDescriptorGatewayPort, NetworkDescriptorModel},
        directory::{ClaimPortError, SaveNetworkDescriptorError, save_network_descriptors},
        managed::pocketic::{
            CreateHttpGatewayError, CreateInstanceError, PocketIcAdminInterface, PocketIcInstance,
            spawn_pocketic_launcher,
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
    let pocketic_launcher_path = PathBuf::from(
        var("ICP_POCKET_IC_LAUNCHER_PATH")
            .ok()
            .ok_or(NoPocketIcPath)?,
    );

    nd.ensure_exists()?;

    run_pocketic_launcher(
        &pocketic_launcher_path,
        config,
        &nd,
        project_root,
        seed_accounts,
        background,
    )
    .await?;
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum RunNetworkError {
    #[snafu(transparent)]
    CreateDirFailed { source: crate::fs::IoError },

    #[snafu(display("ICP_POCKET_IC_LAUNCHER_PATH environment variable is not set"))]
    NoPocketIcPath,

    #[snafu(transparent)]
    LockFileError { source: LockError },

    #[snafu(transparent)]
    RunPocketIc { source: RunPocketIcError },
}

async fn run_pocketic_launcher(
    pocketic_launcher_path: &Path,
    config: &Managed,
    nd: &NetworkDirectory,
    project_root: &Path,
    seed_accounts: impl Iterator<Item = Principal> + Clone,
    background: bool,
) -> Result<(), RunPocketIcError> {
    let network_root = nd.root()?;
    // hold port_claim until the end of this function
    let (mut child, port, _port_claim) = network_root
        .with_write(async |root| -> Result<_, RunPocketIcError> {
            let port_lock = if let Port::Fixed(port) = &config.gateway.port {
                Some(nd.port(*port)?.into_write().await?)
            } else {
                None
            };
            let port_claim = port_lock
                .as_ref()
                .map(|lock| lock.claim_port())
                .transpose()?;
            eprintln!("PocketIC launcher path: {pocketic_launcher_path}");

            create_dir_all(&root.pocketic_dir()).context(CreateDirAllSnafu)?;

            // let port_file = root.pocketic_port_file();
            // if port_file.exists() {
            //     remove_file(&port_file).context(RemoveDirAllSnafu)?;
            // }
            // eprintln!("Port file: {port_file}");

            if root.state_dir().exists() {
                remove_dir_all(&root.state_dir()).context(RemoveDirAllSnafu)?;
            }
            create_dir_all(&root.state_dir()).context(CreateDirAllSnafu)?;
            // let mut child = spawn_pocketic(
            //     pocketic_path,
            //     &port_file,
            //     &root.pocketic_stdout_file(),
            //     &root.pocketic_stderr_file(),
            //     background,
            // );
            let (child, instance) = spawn_pocketic_launcher(
                pocketic_launcher_path,
                &root.pocketic_stdout_file(),
                &root.pocketic_stderr_file(),
                background,
                &config.gateway.port,
                &root.state_dir(),
            )
            .await;
            // let pocketic_port = wait_for_port(&port_file, &mut child).await?;
            eprintln!("PocketIC started on port {}", instance.gateway_port);
            // let instance = initialize_pocketic(
            //     pocketic_port,
            //     &config.gateway.port,
            //     &root.state_dir(),
            //     seed_accounts,
            // )
            // .await?;
            // let port = instance.gateway_port;
            seed_instance(&instance, seed_accounts).await?;
            let gateway = NetworkDescriptorGatewayPort {
                port: instance.gateway_port,
                fixed: matches!(config.gateway.port, Port::Fixed(_)),
            };
            let default_effective_canister_id = instance.effective_canister_id;
            let descriptor = NetworkDescriptorModel {
                id: Uuid::new_v4(),
                project_dir: project_root.to_path_buf(),
                network: nd.network_name.to_owned(),
                network_dir: root.root_dir().to_path_buf(),
                gateway,
                default_effective_canister_id,
                pocketic_url: instance.admin.base_url.to_string(),
                pocketic_instance_id: instance.instance_id,
                pid: Some(child.id().unwrap()),
                root_key: instance.root_key,
            };

            save_network_descriptors(
                root,
                port_lock.as_ref().map(|lock| lock.as_ref()),
                &descriptor,
            )
            .await?;
            Ok((child, instance.gateway_port, port_claim))
        })
        .await??;
    if background {
        // Save the PID of the main `pocket-ic` process
        // This is used by the `icp network stop` command to find and kill the process.
        nd.save_background_network_runner_pid(Pid::from(child.id().unwrap() as usize))
            .await?;
        eprintln!("To stop the network, run `icp network stop`");
    } else {
        eprintln!("Press Ctrl-C to exit.");

        let _ = wait_for_shutdown(&mut child).await;
        let pid = child.id().unwrap() as usize;
        send_sigint(pid.into());
        let _ = child.wait().await;

        let _ = nd.cleanup_project_network_descriptor().await;
        let _ = nd.cleanup_port_descriptor(Some(port)).await;
    }
    Ok(())
}

fn send_sigint(pid: Pid) {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    if let Some(process) = system.process(pid) {
        process.kill_with(Signal::Interrupt);
    }
}

#[derive(Debug, Snafu)]
pub enum RunPocketIcError {
    #[snafu(display("failed to create dir"))]
    CreateDirAll { source: crate::fs::IoError },

    #[snafu(display("failed to remove dir"))]
    RemoveDirAll { source: crate::fs::IoError },

    #[snafu(display("failed to remove file"))]
    RemoveFile { source: crate::fs::IoError },

    #[snafu(transparent)]
    SaveNetworkDescriptor { source: SaveNetworkDescriptorError },

    #[snafu(transparent)]
    InitPocketIc { source: InitializePocketicError },

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

async fn wait_for_shutdown(child: &mut Child) -> ShutdownReason {
    select!(
        _ = ctrl_c() => {
            safe_eprintln("Received Ctrl-C, shutting down PocketIC...");
            ShutdownReason::CtrlC
        }
        res = notice_child_exit(child) => {
            safe_eprintln(&format!("PocketIC exited with status: {:?}", res.status));
            ShutdownReason::ChildExited
        }
    )
}

pub async fn wait_for_port_file(path: &Path) -> Result<u16, WaitForPortTimeoutError> {
    let start_time = std::time::Instant::now();

    loop {
        if let Ok(contents) = read_to_string(path)
            && contents.ends_with('\n')
            && let Ok(port) = contents.trim().parse::<u16>()
        {
            return Ok(port);
        }

        if start_time.elapsed().as_secs() > 30 {
            return WaitForPortTimeoutSnafu.fail();
        }
        sleep(Duration::from_millis(100)).await;
    }
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

// async fn initialize_pocketic(
//     pocketic_port: u16,
//     gateway_bind_port: &Port,
//     state_dir: &Path,
//     seed_accounts: impl Iterator<Item = Principal> + Clone,
// ) -> Result<PocketIcInstance, InitializePocketicError> {
//     let instance_config = default_instance_config(state_dir);
//     let gateway_port = match gateway_bind_port {
//         Port::Fixed(port) => Some(*port),
//         Port::Random => None,
//     };

//     initialize_instance(pocketic_port, instance_config, gateway_port, seed_accounts).await
// }

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

    eprintln!("Initializing PocketIC instance");
    let (instance_id, topology) = pic.create_instance_with_config(instance_config).await?;
    let default_effective_canister_id = topology.default_effective_canister_id;
    eprintln!("Created instance with id {instance_id}");

    eprintln!("Setting time");
    pic.set_time(instance_id).await?;

    eprintln!("Setting auto-progress");
    let artificial_delay = 600;
    pic.auto_progress(instance_id, artificial_delay).await?;

    eprintln!("Seeding ICP and TCYCLES account balances");
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
    eprintln!("Agent url is {agent_url}");
    let status = ping_and_wait(&agent_url).await?;

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

pub async fn seed_instance(
    instance: &PocketIcInstance,
    seed_accounts: impl Iterator<Item = Principal> + Clone,
) -> Result<(), InitializePocketicError> {
    eprintln!("Seeding ICP and TCYCLES account balances");
    let pocket_ic_client = PocketIc::new_from_existing_instance(
        instance.admin.base_url.clone(),
        instance.instance_id,
        None,
    );
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
pub enum InitializePocketicError {
    #[snafu(transparent)]
    CreateInstance { source: CreateInstanceError },

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
    pic: &PocketIc,
    account: Principal,
    amount: u128,
    icp_xdr_conversion_rate: u64,
) -> Result<(), InitializePocketicError> {
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
        return SeedTokensSnafu {
            error: format!("Failed to notify mint cycles: {err:?}"),
        }
        .fail();
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
    eprintln!("Minted {amount} ICP to account {account}");
    Ok(())
}

async fn get_icp_xdr_conversion_rate(pic: &PocketIc) -> Result<u64, InitializePocketicError> {
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
