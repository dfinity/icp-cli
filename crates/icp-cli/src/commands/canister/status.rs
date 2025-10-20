use clap::Args;
use ic_agent::{AgentError, export::Principal};
use ic_management_canister_types::{CanisterStatusResult, LogVisibility};
use icp::{agent, identity, network};

use crate::{
    commands::{Context, Mode, args},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Args)]
pub(crate) struct StatusArgs {
    pub(crate) canister: args::Canister,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error("an invalid argument was provided")]
    Args,

    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error(transparent)]
    Lookup(#[from] LookupIdError),

    #[error(transparent)]
    Status(#[from] AgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &StatusArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            let args::Canister::Principal(_) = &args.canister else {
                return Err(CommandError::Args);
            };

            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(pdir) => {
            let args::Canister::Name(name) = &args.canister else {
                return Err(CommandError::Args);
            };

            // Load project
            let p = ctx.project.load(pdir).await?;

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Load target environment
            let env = p.environments.get(args.environment.name()).ok_or(
                CommandError::EnvironmentNotFound {
                    name: args.environment.name().to_owned(),
                },
            )?;

            // Access network
            let access = ctx.network.access(&env.network).await?;

            // Agent
            let agent = ctx.agent.create(id, &access.url).await?;

            if let Some(k) = access.root_key {
                agent.set_root_key(k);
            }

            // Ensure canister is included in the environment
            if !env.canisters.contains_key(name) {
                return Err(CommandError::EnvironmentCanister {
                    environment: env.name.to_owned(),
                    canister: name.to_owned(),
                });
            }

            // Lookup the canister id
            let cid = ctx.ids.lookup(&Key {
                network: env.network.name.to_owned(),
                environment: env.name.to_owned(),
                canister: name.to_owned(),
            })?;

            // Management Interface
            let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

            // Retrieve canister status from management canister
            let (result,) = mgmt.canister_status(&cid).await?;

            // Status printout
            print_status(&result);
        }
    }

    Ok(())
}

// TODO(or.ricon): Convert this to indoc?
fn print_status(result: &CanisterStatusResult) {
    eprintln!("Canister Status Report:");
    eprintln!("  Status: {:?}", result.status);

    let settings = &result.settings;
    let controllers: Vec<String> = settings.controllers.iter().map(|p| p.to_string()).collect();
    eprintln!("  Controllers: {}", controllers.join(", "));
    eprintln!("  Compute allocation: {}", settings.compute_allocation);
    eprintln!("  Memory allocation: {}", settings.memory_allocation);
    eprintln!("  Freezing threshold: {}", settings.freezing_threshold);

    eprintln!(
        "  Reserved cycles limit: {}",
        settings.reserved_cycles_limit
    );
    eprintln!("  Wasm memory limit: {}", settings.wasm_memory_limit);
    eprintln!(
        "  Wasm memory threshold: {}",
        settings.wasm_memory_threshold
    );

    let log_visibility = match &settings.log_visibility {
        LogVisibility::Controllers => "Controllers".to_string(),
        LogVisibility::Public => "Public".to_string(),
        LogVisibility::AllowedViewers(viewers) => {
            if viewers.is_empty() {
                "Allowed viewers list is empty".to_string()
            } else {
                let mut viewers: Vec<_> = viewers.iter().map(Principal::to_text).collect();
                viewers.sort();
                format!("Allowed viewers: {}", viewers.join(", "))
            }
        }
    };
    eprintln!("  Log visibility: {log_visibility}");

    // Display environment variables configured for this canister
    // Environment variables are key-value pairs that can be accessed within the canister
    if settings.environment_variables.is_empty() {
        eprintln!("  Environment Variables: N/A",);
    } else {
        eprintln!("  Environment Variables:");
        for v in &settings.environment_variables {
            eprintln!("    Name: {}, Value: {}", v.name, v.value);
        }
    }

    match &result.module_hash {
        Some(hash) => {
            let hex_string: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            eprintln!("  Module hash: 0x{hex_string}");
        }
        None => eprintln!("  Module hash: <none>"),
    }

    eprintln!("  Memory size: {}", result.memory_size);
    eprintln!("  Cycles: {}", result.cycles);
    eprintln!("  Reserved cycles: {}", result.reserved_cycles);
    eprintln!(
        "  Idle cycles burned per day: {}",
        result.idle_cycles_burned_per_day
    );

    let stats = &result.query_stats;
    eprintln!("  Query stats:");
    eprintln!("    Calls: {}", stats.num_calls_total);
    eprintln!("    Instructions: {}", stats.num_instructions_total);
    eprintln!(
        "    Req payload bytes: {}",
        stats.request_payload_bytes_total
    );
    eprintln!(
        "    Res payload bytes: {}",
        stats.response_payload_bytes_total
    );
}
