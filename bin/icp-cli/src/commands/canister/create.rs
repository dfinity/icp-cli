use candid::{CandidType, Decode, Encode, Nat};
use clap::Parser;
use ic_agent::{AgentError, export::Principal};
use ic_utils::interfaces::management_canister::LogVisibility;
use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::{
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError, RegisterError},
};

pub const DEFAULT_EFFECTIVE_ID: &str = "uqqxf-5h777-77774-qaaaa-cai";

#[derive(Debug, Parser)]
pub struct CanisterIDs {
    /// The effective canister ID to use when calling the management canister.
    #[clap(long, default_value = DEFAULT_EFFECTIVE_ID)]
    pub effective_id: Principal,

    /// The specific canister ID to assign if creating with a fixed principal.
    #[clap(long)]
    pub specific_id: Option<Principal>,
}

#[derive(Debug, Default, Parser)]
pub struct CanisterSettings {
    /// Optional compute allocation (0 to 100). Represents guaranteed compute capacity.
    #[clap(long)]
    pub compute_allocation: Option<u64>,

    /// Optional memory allocation in bytes. If unset, memory is allocated dynamically.
    #[clap(long)]
    pub memory_allocation: Option<u64>,

    /// Optional freezing threshold in seconds. Controls how long a canister can be inactive before being frozen.
    #[clap(long)]
    pub freezing_threshold: Option<u64>,

    /// Optional reserved cycles limit. If set, the canister cannot consume more than this many cycles.
    #[clap(long)]
    pub reserved_cycles_limit: Option<u64>,

    /// Optional Wasm memory limit in bytes. Sets an upper bound for Wasm heap growth.
    #[clap(long)]
    pub wasm_memory_limit: Option<u64>,

    /// Optional Wasm memory threshold in bytes. Triggers a callback when exceeded.
    #[clap(long)]
    pub wasm_memory_threshold: Option<u64>,
}

#[derive(Debug, Parser)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: Option<String>,

    #[clap(flatten)]
    pub identity: IdentityOpt,

    #[clap(flatten)]
    pub environment: EnvironmentOpt,

    // Canister ID configuration, including the effective and optionally specific ID.
    #[clap(flatten)]
    pub ids: CanisterIDs,

    /// One or more controllers for the canister. Repeat `--controller` to specify multiple.
    #[clap(long)]
    pub controller: Vec<Principal>,

    // Resource-related settings and thresholds for the new canister.
    #[clap(flatten)]
    pub settings: CanisterSettings,

    /// How many cycles the canister should be created with. Also needs to pay for canister creation cost.
    #[clap(long)]
    pub cycles: Option<u128>,

    /// Suppress human-readable output; print only canister IDs, one per line, to stdout.
    #[clap(long, short = 'q')]
    pub quiet: bool,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load project
    let pm = ctx.project()?;

    // Load target environment
    let env = pm
        .environments
        .iter()
        .find(|&v| v.name == cmd.environment.name())
        .ok_or(CommandError::EnvironmentNotFound {
            name: cmd.environment.name().to_owned(),
        })?;

    // Choose canisters to create
    let cs = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.name {
            Some(name) => name == &c.name,
            None => true,
        })
        .collect::<Vec<_>>();

    // Check if selected canister exists
    if let Some(name) = &cmd.name {
        if cs.is_empty() {
            return Err(CommandError::CanisterNotFound {
                name: name.to_owned(),
            });
        }
    }

    // Collect environment canisters
    let ecs = env.canisters.clone().unwrap_or(
        pm.canisters
            .iter()
            .map(|(_, c)| c.name.to_owned())
            .collect(),
    );

    // Filter for environment canisters
    let cs = cs
        .iter()
        .filter(|(_, c)| ecs.contains(&c.name))
        .collect::<Vec<_>>();

    // Ensure canister is included in the environment
    if let Some(name) = &cmd.name {
        if !ecs.contains(name) {
            return Err(CommandError::EnvironmentCanister {
                environment: env.name.to_owned(),
                canister: name.to_owned(),
            });
        }
    }

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

    // Skip created canisters
    let cs = cs
        .into_iter()
        .filter(|&(_, c)| {
            let cid = ctx.id_store.lookup(&Key {
                network: network.to_owned(),
                environment: env.name.to_owned(),
                canister: c.name.to_owned(),
            });

            match cid {
                // Exists (skip)
                Ok(_) => false,

                // Doesn't exist (include)
                Err(LookupError::IdNotFound { .. }) => true,

                // Lookup failed
                Err(err) => panic!("{err}"),
            }
        })
        .collect::<Vec<_>>();

    // Verify at least one canister is available to create
    if cs.is_empty() {
        return Err(CommandError::NoCanisters);
    }

    // Load identity
    ctx.require_identity(cmd.identity.name());

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    ctx.require_network(
        env.network
            .as_ref()
            .expect("no network specified in environment"),
    );

    // Prepare agent
    let agent = ctx.agent()?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(agent);

    for (_, c) in cs {
        // TODO: support detecting (non)mainnet
        if network == "ic" {
            #[derive(CandidType, Serialize, Clone, Debug, Default)]
            pub struct CanisterSettings {
                pub freezing_threshold: Option<Nat>,
                pub controllers: Option<Vec<Principal>>,
                pub reserved_cycles_limit: Option<Nat>,
                pub memory_allocation: Option<Nat>,
                pub compute_allocation: Option<Nat>,
            }

            #[derive(CandidType, Serialize, Clone, Debug, Default)]
            pub struct SubnetFilter {
                pub subnet_type: Option<String>,
            }

            #[derive(CandidType, Serialize, Clone, Debug)]
            pub enum SubnetSelection {
                Filter(SubnetFilter),
                Subnet { subnet: Principal },
            }

            #[derive(CandidType, Serialize, Clone, Debug, Default)]
            pub struct CmcCreateCanisterArgs {
                pub subnet_selection: Option<SubnetSelection>,
                pub settings: Option<CanisterSettings>,
            }

            #[derive(CandidType, Serialize, Clone, Debug, Default)]
            pub struct CreateCanisterArgs {
                pub from_subaccount: Option<Vec<u8>>,
                pub created_at_time: Option<u64>,
                pub amount: Nat,
                pub creation_args: Option<CmcCreateCanisterArgs>,
            }

            let create_canister_arg = CreateCanisterArgs {
                from_subaccount: None,
                created_at_time: None,
                amount: cmd.cycles.unwrap_or(1_500_000_000_000).into(),
                creation_args: Some(CmcCreateCanisterArgs {
                    subnet_selection: None,
                    settings: Some(CanisterSettings {
                        freezing_threshold: cmd.settings.freezing_threshold.map(|v| v.into()),
                        controllers: Some(cmd.controller.clone()),
                        reserved_cycles_limit: cmd.settings.reserved_cycles_limit.map(|v| v.into()),
                        memory_allocation: cmd.settings.memory_allocation.map(|v| v.into()),
                        compute_allocation: cmd.settings.compute_allocation.map(|v| v.into()),
                    }),
                }),
            };

            let result_bytes = agent
                .update(
                    &Principal::from_text("um5iw-rqaaa-aaaaq-qaaba-cai").unwrap(),
                    "create_canister",
                )
                .with_arg(Encode!(&create_canister_arg).unwrap())
                .call_and_wait()
                .await?;

            // Types for create_canister result
            pub type BlockIndex = Nat;

            #[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
            pub struct CreateCanisterSuccess {
                pub block_id: BlockIndex,
                pub canister_id: Principal,
            }

            #[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
            pub struct CreateCanisterErrorGeneric {
                pub message: String,
                pub error_code: Nat,
            }

            #[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
            pub struct CreateCanisterErrorDuplicate {
                pub duplicate_of: Nat,
                pub canister_id: Option<Principal>,
            }

            #[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
            pub struct CreateCanisterErrorCreatedInFuture {
                pub ledger_time: u64,
            }

            #[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
            pub struct CreateCanisterErrorFailedToCreate {
                pub error: String,
                pub refund_block: Option<BlockIndex>,
                pub fee_block: Option<BlockIndex>,
            }

            #[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
            pub struct CreateCanisterErrorInsufficientFunds {
                pub balance: Nat,
            }

            #[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
            pub enum CreateCanisterError {
                GenericError(CreateCanisterErrorGeneric),
                TemporarilyUnavailable,
                Duplicate(CreateCanisterErrorDuplicate),
                CreatedInFuture(CreateCanisterErrorCreatedInFuture),
                FailedToCreate(CreateCanisterErrorFailedToCreate),
                TooOld,
                InsufficientFunds(CreateCanisterErrorInsufficientFunds),
            }

            #[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
            pub enum CreateCanisterResult {
                Ok(CreateCanisterSuccess),
                Err(CreateCanisterError),
            }

            let result = Decode!(&result_bytes, CreateCanisterResult)
                .expect("The cycles ledger's create_canister method returned an invalid result");

            match result {
                CreateCanisterResult::Ok(success) => {
                    if cmd.quiet {
                        println!("{}", success.canister_id);
                    } else {
                        eprintln!(
                            "Created canister '{}' with ID: '{}'",
                            c.name, success.canister_id
                        );
                    }
                }
                CreateCanisterResult::Err(error) => {
                    println!("Error creating canister: {:?}", error);
                }
            }
        } else {
            // Create canister
            let mut builder = mgmt.create_canister();

            // Cycles amount
            builder = builder.as_provisional_create_with_amount(cmd.cycles);

            // Canister ID (effective)
            builder = builder.with_effective_canister_id(cmd.ids.effective_id);

            // Canister ID (specific)
            if let Some(id) = cmd.ids.specific_id {
                builder = builder.as_provisional_create_with_specified_id(id);
            }

            // Controllers
            for c in &cmd.controller {
                builder = builder.with_controller(c.to_owned());
            }

            // Compute
            builder = builder.with_optional_compute_allocation(
                cmd.settings
                    .compute_allocation
                    .or(c.settings.compute_allocation),
            );

            // Memory
            builder = builder.with_optional_memory_allocation(
                cmd.settings
                    .memory_allocation
                    .or(c.settings.memory_allocation),
            );

            // Freezing Threshold
            builder = builder.with_optional_freezing_threshold(
                cmd.settings
                    .freezing_threshold
                    .or(c.settings.freezing_threshold),
            );

            // Reserved Cycles (limit)
            builder = builder.with_optional_reserved_cycles_limit(
                cmd.settings
                    .reserved_cycles_limit
                    .or(c.settings.reserved_cycles_limit),
            );

            // Wasm (memory limit)
            builder = builder.with_optional_wasm_memory_limit(
                cmd.settings
                    .wasm_memory_limit
                    .or(c.settings.wasm_memory_limit),
            );

            // Wasm (memory threshold)
            builder = builder.with_optional_wasm_memory_threshold(
                cmd.settings
                    .wasm_memory_threshold
                    .or(c.settings.wasm_memory_threshold),
            );

            // Logs
            builder = builder.with_optional_log_visibility(
                Some(LogVisibility::Public), //
            );

            // Create the canister
            let (cid,) = builder.await?;

            // Create canister-network association-key
            let k = Key {
                network: network.to_owned(),
                environment: env.name.to_owned(),
                canister: c.name.to_owned(),
            };

            // Register the canister ID
            ctx.id_store.register(&k, &cid)?;

            if cmd.quiet {
                println!("{}", cid);
            } else {
                eprintln!("Created canister '{}' with ID: '{}'", c.name, cid);
            }
        }
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(display("no canisters available to create"))]
    NoCanisters,

    #[snafu(transparent)]
    CreateCanister { source: AgentError },

    #[snafu(transparent)]
    RegisterCanister { source: RegisterError },
}
