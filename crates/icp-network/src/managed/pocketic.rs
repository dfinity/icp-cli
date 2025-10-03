use candid::Principal;
use futures::future::{join, join_all};
use icp::prelude::*;
use pocket_ic::{
    common::rest::{
        AutoProgressConfig, CreateHttpGatewayResponse, CreateInstanceResponse, HttpGatewayBackend,
        HttpGatewayConfig, HttpGatewayInfo, IcpConfig, IcpConfigFlag, IcpFeatures,
        IcpFeaturesConfig, InstanceConfig, InstanceId, RawTime, SubnetConfigSet, Topology,
    },
    nonblocking::PocketIc,
};
use reqwest::Url;
use snafu::prelude::*;
use time::OffsetDateTime;

use crate::managed::run::InitializePocketicError;

/// Creates a default InstanceConfig for production use with 1 application subnet
pub fn default_instance_config(state_dir: &Path) -> InstanceConfig {
    custom_instance_config(state_dir, 1)
}

/// Creates a custom InstanceConfig with specified number of application subnets
pub fn custom_instance_config(state_dir: &Path, application_subnets: usize) -> InstanceConfig {
    InstanceConfig {
        // State directory
        state_dir: Some(state_dir.to_path_buf().into()),

        // Replica logging level
        log_level: Some("ERROR".to_string()),

        // Special features
        icp_features: Some(IcpFeatures {
            // Enable with default feature configuration
            icp_token: Some(IcpFeaturesConfig::DefaultConfig),

            // Same as above
            cycles_token: Some(IcpFeaturesConfig::DefaultConfig),

            // Same as above
            cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),

            // Same as above
            registry: Some(IcpFeaturesConfig::DefaultConfig),

            // The rest of the features are disabled by default
            ..Default::default()
        }),

        subnet_config_set: (SubnetConfigSet {
            application: application_subnets,

            // The rest of the subnets are disabled by default
            ..Default::default()
        })
        .into(),

        icp_config: Some(IcpConfig {
            // Required to enable environment variables
            beta_features: Some(IcpConfigFlag::Enabled),
            ..Default::default()
        }),

        ..Default::default()
    }
}

/// Result of initializing a PocketIC instance
pub struct PocketIcInstance {
    pub admin: PocketIcAdminInterface,
    pub gateway_port: u16,
    pub instance_id: InstanceId,
    pub effective_canister_id: Principal,
    pub root_key: String,
}

pub fn spawn_pocketic(pocketic_path: &Path, port_file: &Path) -> tokio::process::Child {
    let mut cmd = tokio::process::Command::new(pocketic_path);
    cmd.arg("--port-file");
    cmd.arg(port_file.as_os_str());
    cmd.args(["--ttl", "2592000", "--log-levels", "error"]);

    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());
    #[cfg(unix)]
    {
        //use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    eprintln!("Starting PocketIC...");
    cmd.spawn().expect("Could not start PocketIC.")
}

pub struct PocketIcAdminInterface {
    client: reqwest::Client,
    pub base_url: Url,
}

impl PocketIcAdminInterface {
    pub fn new(base_url: Url) -> Self {
        let client = reqwest::Client::new();
        Self { client, base_url }
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.post(self.base_url.join(path).unwrap())
    }

    pub async fn create_instance(
        &self,
        state_dir: &Path,
    ) -> Result<(InstanceId, Topology), CreateInstanceError> {
        let inst_cfg = default_instance_config(state_dir);
        self.create_instance_with_config(inst_cfg).await
    }

    pub async fn create_instance_with_config(
        &self,
        inst_cfg: InstanceConfig,
    ) -> Result<(InstanceId, Topology), CreateInstanceError> {
        // Perform request
        let resp = self.post("/instances").json(&inst_cfg).send().await?;

        // Check for replica errors
        let resp = resp
            .error_for_status()?
            .json::<CreateInstanceResponse>()
            .await?;

        match resp {
            CreateInstanceResponse::Created {
                instance_id,
                topology,
                ..
            } => Ok((instance_id, topology)),

            CreateInstanceResponse::Error { message } => {
                Err(CreateInstanceError::Create { message })
            }
        }
    }

    async fn set_time(&self, instance_id: InstanceId) -> Result<(), reqwest::Error> {
        self.post(&format!("/instances/{instance_id}/update/set_time"))
            .json(&RawTime {
                nanos_since_epoch: OffsetDateTime::now_utc()
                    .unix_timestamp_nanos()
                    .try_into()
                    .unwrap(),
            })
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn auto_progress(
        &self,
        instance_id: InstanceId,
        artificial_delay: i32,
    ) -> Result<(), reqwest::Error> {
        self.post(&format!("/instances/{instance_id}/auto_progress"))
            .json(&AutoProgressConfig {
                artificial_delay_ms: Some(artificial_delay as u64),
            })
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn create_http_gateway(
        &self,
        forward_to: HttpGatewayBackend,
        port: Option<u16>,
    ) -> Result<HttpGatewayInfo, CreateHttpGatewayError> {
        let resp = self
            .post("/http_gateway")
            .json(&HttpGatewayConfig {
                ip_addr: None,
                port,
                forward_to,
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
                Err(CreateHttpGatewayError::Create { message })
            }
            CreateHttpGatewayResponse::Created(info) => Ok(info),
        }
    }
}

#[derive(Debug, Snafu)]
pub enum CreateInstanceError {
    #[snafu(
        display("failed to create PocketIC instance: {message}"),
        context(suffix(InstanceSnafu))
    )]
    Create { message: String },

    #[snafu(transparent)]
    Reqwest { source: reqwest::Error },
}

#[derive(Debug, Snafu)]
pub enum CreateHttpGatewayError {
    #[snafu(
        display("failed to create HTTP gateway: {message}"),
        context(suffix(GatewaySnafu))
    )]
    Create { message: String },

    #[snafu(transparent, context(false))]
    Reqwest { source: reqwest::Error },
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
