use std::collections::HashSet;

use candid::{CandidType, Decode, Deserialize, Encode, Nat};
use clap::Parser;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{AgentError, export::Principal};
use snafu::Snafu;

use crate::{
    context::{Context, ContextAgentError, ContextProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    progress::ProgressManager,
    store_id::{Key, LookupError, RegisterError},
};
use icp_canister_interfaces::cycles_ledger::CYCLES_LEDGER_CID;

pub const DEFAULT_CANISTER_CYCLES: u128 = 2_000_000_000_000;

#[derive(Clone, Debug, Default, Parser)]
pub struct CanisterSettings {
    /// Optional compute allocation (0 to 100). Represents guaranteed compute capacity.
    #[arg(long)]
    pub compute_allocation: Option<u64>,

    /// Optional memory allocation in bytes. If unset, memory is allocated dynamically.
    #[arg(long)]
    pub memory_allocation: Option<u64>,

    /// Optional freezing threshold in seconds. Controls how long a canister can be inactive before being frozen.
    #[arg(long)]
    pub freezing_threshold: Option<u64>,

    /// Optional reserved cycles limit. If set, the canister cannot consume more than this many cycles.
    #[arg(long)]
    pub reserved_cycles_limit: Option<u64>,
}

#[derive(Clone, Debug, Parser)]
pub struct Cmd {
    /// The names of the canister within the current project
    pub names: Vec<String>,

    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,

    /// One or more controllers for the canister. Repeat `--controller` to specify multiple.
    #[arg(long)]
    pub controller: Vec<Principal>,

    // Resource-related settings and thresholds for the new canister.
    #[command(flatten)]
    pub settings: CanisterSettings,

    /// Suppress human-readable output; print only canister IDs, one per line, to stdout.
    #[arg(long, short = 'q')]
    pub quiet: bool,

    /// Cycles to fund canister creation (in raw cycles).
    #[arg(long, default_value_t = DEFAULT_CANISTER_CYCLES)]
    pub cycles: u128,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterSettingsArg {
    freezing_threshold: Option<Nat>,
    controllers: Option<Vec<Principal>>,
    reserved_cycles_limit: Option<Nat>,
    memory_allocation: Option<Nat>,
    compute_allocation: Option<Nat>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum SubnetSelectionArg {
    Filter { subnet_type: Option<String> },
    Subnet { subnet: Principal },
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CreationArgs {
    subnet_selection: Option<SubnetSelectionArg>,
    settings: Option<CanisterSettingsArg>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CreateCanisterArgs {
    from_subaccount: Option<Vec<u8>>,
    created_at_time: Option<u64>,
    amount: Nat,
    creation_args: Option<CreationArgs>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum CreateCanisterResponse {
    Ok {
        block_id: Nat,
        canister_id: Principal,
    },
    Err(CreateCanisterError),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum CreateCanisterError {
    GenericError {
        message: String,
        error_code: Nat,
    },
    TemporarilyUnavailable,
    Duplicate {
        duplicate_of: Nat,
        canister_id: Option<Principal>,
    },
    CreatedInFuture {
        ledger_time: u64,
    },
    FailedToCreate {
        error: String,
        refund_block: Option<Nat>,
        fee_block: Option<Nat>,
    },
    TooOld,
    InsufficientFunds {
        balance: Nat,
    },
}

fn format_create_canister_error(err: &CreateCanisterError, requested_cycles: u128) -> String {
    match err {
        CreateCanisterError::GenericError {
            message,
            error_code,
        } => {
            format!("Cycles ledger error (code {}): {}", error_code, message)
        }
        CreateCanisterError::TemporarilyUnavailable => {
            "Cycles ledger temporarily unavailable. Please retry in a moment.".to_string()
        }
        CreateCanisterError::Duplicate { .. } => {
            unreachable!("no created_at_time is set therefore the request is not deduplicated")
        }
        CreateCanisterError::CreatedInFuture { .. } => {
            unreachable!("no created_at_time is set therefore the request is not in the future")
        }
        CreateCanisterError::FailedToCreate {
            error,
            refund_block,
            fee_block,
        } => {
            let mut msg = format!("Failed to create canister: {}", error);
            if let Some(b) = refund_block {
                msg.push_str(&format!(". Refund block: {}", b));
            }
            if let Some(b) = fee_block {
                msg.push_str(&format!(". Fee block: {}", b));
            }
            msg
        }
        CreateCanisterError::TooOld => {
            unreachable!("no created_at_time is set therefore the request is not too old")
        }
        CreateCanisterError::InsufficientFunds { balance } => {
            format!(
                "Insufficient cycles. Requested: {} cycles, available balance: {} cycles. 
                use `icp cycles mint` to get more cycles or use `--cycles` to specify a different amount.",
                requested_cycles, balance
            )
        }
    }
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
        .filter(|(_, c)| match &cmd.names.is_empty() {
            // If no names specified, create all canisters
            true => true,

            // If names specified, only create matching canisters
            false => cmd.names.contains(&c.name),
        })
        .collect::<Vec<_>>();

    // Check if selected canisters exists
    if !cmd.names.is_empty() {
        let names = cs.iter().map(|(_, c)| &c.name).collect::<HashSet<_>>();

        for name in &cmd.names {
            if !names.contains(name) {
                return Err(CommandError::CanisterNotFound {
                    name: name.to_owned(),
                });
            }
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
    if !cmd.names.is_empty() {
        for name in &cmd.names {
            if !ecs.contains(name) {
                return Err(CommandError::EnvironmentCanister {
                    environment: env.name.to_owned(),
                    canister: name.to_owned(),
                });
            }
        }
    }

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

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

    // Prepare a futures set for concurrent operations
    let mut futs = FuturesOrdered::new();

    let progress_manager = ProgressManager::new();

    for (_, c) in cs {
        // Create progress bar with standard configuration
        let pb = progress_manager.create_progress_bar(&c.name);

        // Create an async closure that handles the operation for this specific canister
        let create_fn = {
            let cmd = cmd.clone();
            let pb = pb.clone();

            async move {
                // Indicate to user that the canister is created
                pb.set_message("Creating...");

                // Create canister-network association-key
                let k = Key {
                    network: network.to_owned(),
                    environment: env.name.to_owned(),
                    canister: c.name.to_owned(),
                };

                match ctx.id_store.lookup(&k) {
                    // Exists (skip)
                    Ok(principal) => {
                        return Err(CommandError::CanisterExists { principal });
                    }

                    // Doesn't exist (include)
                    Err(LookupError::IdNotFound { .. }) => {}

                    // Lookup failed
                    Err(err) => panic!("{err}"),
                };

                // Build cycles ledger create_canister args
                let settings = CanisterSettingsArg {
                    freezing_threshold: cmd
                        .settings
                        .freezing_threshold
                        .or(c.settings.freezing_threshold)
                        .map(Nat::from),
                    controllers: if cmd.controller.is_empty() {
                        None
                    } else {
                        Some(cmd.controller.clone())
                    },
                    reserved_cycles_limit: cmd
                        .settings
                        .reserved_cycles_limit
                        .or(c.settings.reserved_cycles_limit)
                        .map(Nat::from),
                    memory_allocation: cmd
                        .settings
                        .memory_allocation
                        .or(c.settings.memory_allocation)
                        .map(Nat::from),
                    compute_allocation: cmd
                        .settings
                        .compute_allocation
                        .or(c.settings.compute_allocation)
                        .map(Nat::from),
                };

                let creation_args = CreationArgs {
                    subnet_selection: None,
                    settings: Some(settings),
                };

                let arg = CreateCanisterArgs {
                    from_subaccount: None,
                    created_at_time: None,
                    amount: Nat::from(cmd.cycles),
                    creation_args: Some(creation_args),
                };

                // Call cycles ledger create_canister
                let resp = agent
                    .update(
                        &Principal::from_text(CYCLES_LEDGER_CID).unwrap(),
                        "create_canister",
                    )
                    .with_arg(Encode!(&arg).map_err(|source| CommandError::Candid { source })?)
                    .call_and_wait()
                    .await
                    .map_err(|source| CommandError::CreateCanister { source })?;

                let resp: CreateCanisterResponse = Decode!(&resp, CreateCanisterResponse)
                    .map_err(|source| CommandError::Candid { source })?;

                let cid = match resp {
                    CreateCanisterResponse::Ok { canister_id, .. } => canister_id,
                    CreateCanisterResponse::Err(err) => {
                        return Err(CommandError::LedgerCreate {
                            err: format_create_canister_error(&err, cmd.cycles),
                        });
                    }
                };

                // Register the canister ID
                ctx.id_store.register(&k, &cid)?;

                Ok::<_, CommandError>(())
            }
        };

        futs.push_back(async move {
            // Execute the create function with custom progress tracking
            let mut result = ProgressManager::execute_with_custom_progress(
                pb,
                create_fn,
                || "Created successfully".to_string(),
                |err| match err {
                    CommandError::CanisterExists { principal } => {
                        format!("Canister already created: {principal}")
                    }
                    _ => format!("Failed to create canister: {err}"),
                },
                |err| matches!(err, CommandError::CanisterExists { .. }),
            )
            .await;

            // If canister already exists, it is not considered an error
            if let Err(CommandError::CanisterExists { .. }) = result {
                result = Ok(());
            }

            result
        });
    }

    // Consume the set of futures and abort if an error occurs
    while let Some(res) = futs.next().await {
        // TODO(or.ricon): Handle canister creation failures
        res?;
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextAgentError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(display("no canisters available to create"))]
    NoCanisters,

    #[snafu(display("canister exists already: {principal}"))]
    CanisterExists { principal: Principal },

    #[snafu(transparent)]
    CreateCanister { source: AgentError },

    #[snafu(transparent)]
    RegisterCanister { source: RegisterError },

    #[snafu(transparent)]
    Candid { source: candid::Error },

    #[snafu(display("{err}"))]
    LedgerCreate { err: String },
}
