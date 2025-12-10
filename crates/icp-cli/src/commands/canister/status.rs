use clap::Args;
use ic_agent::export::Principal;
use ic_management_canister_types::{CanisterStatusResult, LogVisibility};
use icp::{
    context::{CanisterSelection, Context, EnvironmentSelection, NetworkSelection},
    identity::IdentitySelection,
};

use crate::{commands::args, options};

#[derive(Debug, Args)]
pub(crate) struct StatusArgs {
    /// An optional canister name or principal to target.
    /// When using a name, an enviroment must be specified
    pub(crate) canister: Option<args::Canister>,

    #[command(flatten)]
    pub(crate) network: options::NetworkOpt,

    #[command(flatten)]
    pub(crate) environment: options::EnvironmentOpt,

    #[command(flatten)]
    pub(crate) identity: options::IdentityOpt,

    /// Only print the canister ids
    #[arg(short, long)]
    pub id_only: bool,

    /// Format output in json
    #[arg(long = "json")]
    pub json_format: bool,
}

async fn get_principals(
    ctx: &Context,
    canister: Option<args::Canister>,
    environment: &EnvironmentSelection,
    network: &NetworkSelection,
) -> Result<Vec<Principal>, anyhow::Error> {
    let mut cids = Vec::<Principal>::new();
    match canister {
        Some(canister) => {
            let canister_selection: CanisterSelection = canister.clone().into();
            let cid = ctx
                .get_canister_id(&canister_selection, network, environment)
                .await?;
            cids.push(cid);
        }
        None => {
            let env = ctx.get_environment(environment).await?;
            for (_, c) in env.canisters.values() {
                let cid = ctx
                    .get_canister_id(
                        &CanisterSelection::Named(c.name.clone()),
                        network,
                        environment,
                    )
                    .await?;
                cids.push(cid);
            }
        }
    };
    Ok(cids)
}

pub(crate) async fn exec(ctx: &Context, args: &StatusArgs) -> Result<(), anyhow::Error> {
    struct Selection {
        environment: EnvironmentSelection,
        network: NetworkSelection,
        identity: IdentitySelection,
    }

    let selections = Selection {
        environment: args.environment.clone().into(),
        network: args.network.clone().into(),
        identity: args.identity.clone().into(),
    };

    let cids = get_principals(
        ctx,
        args.canister.clone(),
        &selections.environment,
        &selections.network,
    )
    .await?;

    if args.id_only {
        for cid in cids.iter() {
            let _ = ctx.term.write_line(&format!("{cid}"));
        }
        return Ok(());
    }

    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    for cid in cids.iter() {
        // Retrieve canister status from management canister
        let (result,) = mgmt.canister_status(cid).await?;
        // Status printout
        print_status(cid, &result);
    }

    Ok(())
}

pub(crate) fn print_status(canister_id: &Principal, result: &CanisterStatusResult) {
    eprintln!("Canister id: {canister_id}");
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
