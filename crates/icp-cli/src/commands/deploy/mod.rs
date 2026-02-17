use anyhow::anyhow;
use candid::{CandidType, Principal};
use clap::Args;
use futures::{StreamExt, future::try_join_all, stream::FuturesOrdered};
use ic_agent::Agent;
use icp::{
    context::{CanisterSelection, Context, EnvironmentSelection},
    identity::IdentitySelection,
    network::Configuration as NetworkConfiguration,
};
use icp_canister_interfaces::candid_ui::MAINNET_CANDID_UI_CID;
use serde::Serialize;
use std::sync::Arc;

use crate::{
    commands::{canister::create, parsers::parse_cycles_amount},
    operations::{
        binding_env_vars::set_binding_env_vars_many, build::build_many_with_progress_bar,
        create::CreateOperation, install::install_many, settings::sync_settings_many,
        sync::sync_many,
    },
    options::{EnvironmentOpt, IdentityOpt},
    progress::{ProgressManager, ProgressManagerSettings},
};

/// Deploy a project to an environment
#[derive(Args, Debug)]
pub(crate) struct DeployArgs {
    /// Canister names
    pub(crate) names: Vec<String>,

    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub(crate) mode: String,

    /// The subnet to use for the canisters being deployed.
    #[clap(long)]
    pub(crate) subnet: Option<Principal>,

    /// One or more controllers for the canisters being deployed. Repeat `--controller` to specify multiple.
    #[arg(long)]
    pub(crate) controller: Vec<Principal>,

    /// Cycles to fund canister creation.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long, default_value_t = create::DEFAULT_CANISTER_CYCLES, value_parser = parse_cycles_amount)]
    pub(crate) cycles: u128,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
}

pub(crate) async fn exec(ctx: &Context, args: &DeployArgs) -> Result<(), anyhow::Error> {
    let environment_selection: EnvironmentSelection = args.environment.clone().into();
    let identity_selection: IdentitySelection = args.identity.clone().into();

    let env = ctx.get_environment(&environment_selection).await?;

    let cnames = match args.names.is_empty() {
        // No canisters specified
        true => env.canisters.keys().cloned().collect(),

        // Individual canisters specified
        false => args.names.clone(),
    };

    // Skip doing any work if no canisters are targeted
    if cnames.is_empty() {
        return Ok(());
    }

    let canisters_to_build = try_join_all(
        cnames
            .iter()
            .map(|name| ctx.get_canister_and_path_for_env(name, &environment_selection)),
    )
    .await?;

    // Build the selected canisters
    let _ = ctx.term.write_line("Building canisters:");

    build_many_with_progress_bar(
        canisters_to_build,
        ctx.builder.clone(),
        ctx.artifacts.clone(),
        &ctx.dirs.package_cache()?,
        Arc::new(ctx.term.clone()),
        ctx.debug,
    )
    .await?;

    // Create the selected canisters
    let _ = ctx.term.write_line("\n\nCreating canisters:");

    let env = ctx
        .get_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;
    let agent = ctx
        .get_agent_for_env(&identity_selection, &environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;
    let existing_canisters = ctx
        .ids_by_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;
    let canisters_to_create = cnames
        .iter()
        .filter(|name| !existing_canisters.contains_key(*name))
        .collect::<Vec<_>>();

    if canisters_to_create.is_empty() {
        let _ = ctx.term.write_line("All canisters already exist");
    } else {
        let create_operation = CreateOperation::new(
            agent.clone(),
            args.subnet,
            args.cycles,
            existing_canisters.into_values().collect(),
        );
        let mut futs = FuturesOrdered::new();
        let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });
        for name in canisters_to_create.iter() {
            let pb = progress_manager.create_progress_bar(name);
            pb.set_message("Creating...");
            let create_op = create_operation.clone();
            let (_, canister_info) = env.get_canister_info(name).map_err(|e| anyhow!(e))?;
            futs.push_back(async move {
                ProgressManager::execute_with_custom_progress(
                    &pb,
                    create_op.create(&canister_info.settings.into()),
                    || "Created successfully".to_string(),
                    |err: &_| err.to_string(),
                    |_| false,
                )
                .await
            });
        }

        // Cache errors until all futures are processed. Otherwise we risk dropping a canister id.
        let mut error: Option<anyhow::Error> = None;
        let mut idx = 0;
        while let Some(res) = futs.next().await {
            match res {
                Ok(id) => {
                    let canister_name = canisters_to_create
                        .get(idx)
                        .expect("should have tried to create every canister");
                    let _ = ctx
                        .term
                        .write_line(&format!("Created canister {canister_name} with ID {id}"));
                    ctx.set_canister_id_for_env(canister_name, id, &environment_selection)
                        .await
                        .map_err(|e| anyhow!(e))?;
                }
                Err(err) => {
                    error = Some(err.into());
                }
            }
            idx += 1;
        }
        if let Some(err) = error {
            return Err(err);
        }
    }

    let _ = ctx.term.write_line("\n\nSetting environment variables:");
    let env = ctx
        .get_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;

    let env_canisters = &env.canisters;
    let target_canisters = try_join_all(cnames.iter().map(|name| {
        let environment_selection = environment_selection.clone();
        async move {
            let cid = ctx
                .get_canister_id_for_env(
                    &CanisterSelection::Named(name.clone()),
                    &environment_selection,
                )
                .await
                .map_err(|e| anyhow!(e))?;
            let (_, info) = env_canisters
                .get(name)
                .ok_or_else(|| anyhow!("Canister id exists but no canister info"))?;
            Ok::<_, anyhow::Error>((cid, info.clone()))
        }
    }))
    .await?;

    let canister_list = ctx
        .ids_by_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;

    set_binding_env_vars_many(
        agent.clone(),
        &env.name,
        target_canisters.clone(),
        canister_list,
        Arc::new(ctx.term.clone()),
        ctx.debug,
    )
    .await
    .map_err(|e| anyhow!(e))?;

    sync_settings_many(
        agent.clone(),
        target_canisters,
        Arc::new(ctx.term.clone()),
        ctx.debug,
    )
    .await
    .map_err(|e| anyhow!(e))?;

    // Install the selected canisters
    let _ = ctx.term.write_line("\n\nInstalling canisters:");

    let canisters = try_join_all(cnames.iter().map(|name| {
        let environment_selection = environment_selection.clone();
        async move {
            let cid = ctx
                .get_canister_id_for_env(
                    &CanisterSelection::Named(name.clone()),
                    &environment_selection,
                )
                .await
                .map_err(|e| anyhow!(e))?;

            let env = ctx.get_environment(&environment_selection).await?;
            let (_canister_path, canister_info) =
                env.get_canister_info(name).map_err(|e| anyhow!(e))?;

            let init_args_bytes = canister_info
                .init_args
                .as_ref()
                .map(|ia| ia.to_bytes())
                .transpose()?;

            Ok::<_, anyhow::Error>((name.clone(), cid, init_args_bytes))
        }
    }))
    .await?;

    install_many(
        agent.clone(),
        canisters,
        &args.mode,
        ctx.artifacts.clone(),
        Arc::new(ctx.term.clone()),
        ctx.debug,
    )
    .await?;

    // Sync the selected canisters

    // Prepare list of canisters with their info for syncing
    let env = ctx
        .get_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;

    let env_canisters = &env.canisters;
    let sync_canisters = try_join_all(cnames.iter().map(|name| {
        let environment_selection = environment_selection.clone();
        async move {
            let cid = ctx
                .get_canister_id_for_env(
                    &CanisterSelection::Named(name.clone()),
                    &environment_selection,
                )
                .await
                .map_err(|e| anyhow!(e))?;
            let (canister_path, info) = env_canisters
                .get(name)
                .ok_or_else(|| anyhow!("Canister id exists but no canister info"))?;
            Ok::<_, anyhow::Error>((cid, canister_path.clone(), info.clone()))
        }
    }))
    .await?;

    // Filter out canisters with no sync steps
    let sync_canisters: Vec<_> = sync_canisters
        .into_iter()
        .filter(|(_, _, info)| !info.sync.steps.is_empty())
        .collect();

    if sync_canisters.is_empty() {
        let _ = ctx
            .term
            .write_line("\nNo canisters have sync steps configured");
    } else {
        let _ = ctx.term.write_line("\n\nSyncing canisters:");

        sync_many(
            ctx.syncer.clone(),
            agent.clone(),
            Arc::new(ctx.term.clone()),
            sync_canisters,
            ctx.debug,
        )
        .await?;
    }

    // Print URLs for deployed canisters
    print_canister_urls(ctx, &environment_selection, agent.clone(), &cnames).await?;

    Ok(())
}

/// Checks if a canister has an `http_request` function by querying it
async fn has_http_request(agent: &Agent, canister_id: Principal) -> bool {
    #[derive(CandidType, Serialize)]
    struct HttpRequest {
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    // Construct an HttpRequest for '/index.html'
    let request = HttpRequest {
        method: "GET".to_string(),
        url: "/index.html".to_string(),
        headers: vec![],
        body: vec![],
    };

    let args = candid::encode_one(&request).expect("failed to encode request");

    // Try to query the http_request endpoint
    let result = agent
        .query(&canister_id, "http_request")
        .with_arg(args)
        .call()
        .await;

    // If the query succeeds (regardless of the response), the canister has http_request
    result.is_ok()
}

/// Prints URLs for deployed canisters
async fn print_canister_urls(
    ctx: &Context,
    environment_selection: &EnvironmentSelection,
    agent: Agent,
    canister_names: &[String],
) -> Result<(), anyhow::Error> {
    let env = ctx.get_environment(environment_selection).await?;

    // Get the network URL
    let http_gateway_url = match &env.network.configuration {
        NetworkConfiguration::Managed { managed: _ } => {
            // For managed networks, construct localhost URL
            let access = ctx.network.access(&env.network).await?;
            access.http_gateway_url.clone()
        }
        NetworkConfiguration::Connected { connected } => {
            // For connected networks, use the configured URL
            connected.http_gateway_url.clone()
        }
    };

    let _ = ctx.term.write_line("\n\nDeployed canisters:");

    for name in canister_names {
        let canister_id = match ctx
            .get_canister_id_for_env(
                &CanisterSelection::Named(name.clone()),
                environment_selection,
            )
            .await
        {
            Ok(id) => id,
            Err(_) => continue,
        };

        if let Some(http_gateway_url) = &http_gateway_url {
            // Check if canister has http_request
            let has_http = has_http_request(&agent, canister_id).await;
            let domain = if let Some(domain) = http_gateway_url.domain() {
                Some(domain)
            } else if let Some(host) = http_gateway_url.host_str()
                && (host == "127.0.0.1" || host == "[::1]")
            {
                Some("localhost")
            } else {
                None
            };

            if has_http {
                let mut canister_url = http_gateway_url.clone();
                if let Some(domain) = domain {
                    canister_url
                        .set_host(Some(&format!("{canister_id}.{domain}")))
                        .unwrap();
                } else {
                    canister_url.set_query(Some(&format!("canisterId={canister_id}")));
                }
                let _ = ctx
                    .term
                    .write_line(&format!("  {}: {}", name, canister_url));
            } else {
                // For canisters without http_request, show the Candid UI URL
                if let Some(ref ui_id) = get_candid_ui_id(ctx, environment_selection).await {
                    let mut candid_url = http_gateway_url.clone();
                    if let Some(domain) = domain {
                        candid_url
                            .set_host(Some(&format!("{ui_id}.{domain}",)))
                            .unwrap();
                        candid_url.set_query(Some(&format!("id={canister_id}")));
                    } else {
                        candid_url.set_query(Some(&format!("canisterId={ui_id}&id={canister_id}")));
                    }
                    let _ = ctx
                        .term
                        .write_line(&format!("  {} (Candid UI): {}", name, candid_url));
                } else {
                    // No Candid UI available - just show the canister ID
                    let _ = ctx.term.write_line(&format!(
                        "  {}: {} (Candid UI not available)",
                        name, canister_id
                    ));
                }
            }
        } else {
            // No gateway subdomains available - just show the canister ID
            let _ = ctx.term.write_line(&format!(
                "  {}: {} (No gateway URL available)",
                name, canister_id
            ));
        }
    }

    Ok(())
}

/// Gets the Candid UI canister ID for the network
/// Returns None if the Candid UI ID cannot be determined
async fn get_candid_ui_id(
    ctx: &Context,
    environment_selection: &EnvironmentSelection,
) -> Option<String> {
    let env = ctx.get_environment(environment_selection).await.ok()?;

    match &env.network.configuration {
        NetworkConfiguration::Managed { managed: _ } => {
            // Try to get the candid UI ID from the network descriptor
            let nd = ctx.network.get_network_directory(&env.network).ok()?;
            if let Ok(Some(desc)) = nd.load_network_descriptor().await
                && let Some(candid_ui) = desc.candid_ui_canister_id
            {
                return Some(candid_ui.to_string());
            }
            // No Candid UI available for this managed network
            None
        }
        NetworkConfiguration::Connected { .. } => {
            // For connected networks, use the mainnet Candid UI
            Some(MAINNET_CANDID_UI_CID.to_string())
        }
    }
}
