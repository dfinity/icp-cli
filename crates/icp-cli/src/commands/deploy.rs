use anyhow::anyhow;
use candid::{CandidType, Principal};
use clap::Args;
use futures::{StreamExt, future::try_join_all, stream::FuturesOrdered};
use ic_agent::Agent;
use icp::parsers::CyclesAmount;
use icp::{
    context::{CanisterSelection, Context, EnvironmentSelection},
    identity::IdentitySelection,
    network::Configuration as NetworkConfiguration,
};
use icp_canister_interfaces::candid_ui::MAINNET_CANDID_UI_CID;
use serde::Serialize;
use tracing::info;

use crate::{
    commands::canister::create,
    operations::{
        binding_env_vars::set_binding_env_vars_many,
        build::build_many_with_progress_bar,
        candid_compat::check_candid_compatibility_many,
        create::CreateOperation,
        install::{install_many, resolve_install_mode_and_status},
        settings::sync_settings_many,
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
    #[arg(long, default_value_t = CyclesAmount::from(create::DEFAULT_CANISTER_CYCLES))]
    pub(crate) cycles: CyclesAmount,

    /// Skip confirmation prompts, including the Candid interface compatibility check.
    #[arg(long, short)]
    pub(crate) yes: bool,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

    /// Output command results as JSON
    #[arg(long)]
    pub(crate) json: bool,
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
    info!("Building canisters:");

    build_many_with_progress_bar(
        canisters_to_build,
        ctx.builder.clone(),
        ctx.artifacts.clone(),
        &ctx.dirs.package_cache()?,
        ctx.debug,
    )
    .await?;

    // Create the selected canisters
    info!("Creating canisters:");

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
        info!("All canisters already exist");
    } else {
        let create_operation = CreateOperation::new(
            agent.clone(),
            args.subnet,
            args.cycles.get(),
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
                    if !args.json {
                        println!("Created canister {canister_name} with ID {id}");
                    }
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

    ctx.update_custom_domains(&environment_selection).await;

    info!("Setting environment variables:");
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
        ctx.debug,
    )
    .await
    .map_err(|e| anyhow!(e))?;

    sync_settings_many(agent.clone(), target_canisters, ctx.debug)
        .await
        .map_err(|e| anyhow!(e))?;

    // Install the selected canisters

    let canisters = try_join_all(cnames.iter().map(|name| {
        let environment_selection = environment_selection.clone();
        let agent = agent.clone();
        async move {
            let cid = ctx
                .get_canister_id_for_env(
                    &CanisterSelection::Named(name.clone()),
                    &environment_selection,
                )
                .await
                .map_err(|e| anyhow!(e))?;

            let (mode, status) =
                resolve_install_mode_and_status(&agent, name, &cid, &args.mode).await?;

            let env = ctx.get_environment(&environment_selection).await?;
            let (_canister_path, canister_info) =
                env.get_canister_info(name).map_err(|e| anyhow!(e))?;

            let init_args_bytes = canister_info
                .init_args
                .as_ref()
                .map(|ia| ia.to_bytes())
                .transpose()?;

            Ok::<_, anyhow::Error>((name.clone(), cid, mode, status, init_args_bytes))
        }
    }))
    .await?;

    if !args.yes {
        info!("Checking compatibility:");
        check_candid_compatibility_many(
            agent.clone(),
            canisters
                .iter()
                .map(|(name, cid, mode, _, _)| (&**name, *cid, *mode)),
            ctx.artifacts.clone(),
            ctx.debug,
        )
        .await
        .map_err(|e| anyhow!(e))?;
    }

    info!("Installing canisters:");

    install_many(agent.clone(), canisters, ctx.artifacts.clone(), ctx.debug).await?;

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
        info!("No canisters have sync steps configured");
    } else {
        info!("Syncing canisters:");

        sync_many(ctx.syncer.clone(), agent.clone(), sync_canisters, ctx.debug).await?;
    }

    // Print URLs for deployed canisters
    print_canister_urls(
        ctx,
        &environment_selection,
        agent.clone(),
        &cnames,
        args.json,
    )
    .await?;

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
    json: bool,
) -> Result<(), anyhow::Error> {
    use icp::network::custom_domains::{canister_gateway_url, gateway_domain};

    let env = ctx.get_environment(environment_selection).await?;

    // Get the network URL
    let (http_gateway_url, has_friendly) = match &env.network.configuration {
        NetworkConfiguration::Managed { managed: _ } => {
            let access = ctx.network.access(&env.network).await?;
            (access.http_gateway_url.clone(), access.use_friendly_domains)
        }
        NetworkConfiguration::Connected { connected } => {
            (connected.http_gateway_url.clone(), false)
        }
    };

    let mut json_canisters = Vec::new();

    if !json {
        println!("Deployed canisters:");
    }

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
            let has_http = has_http_request(&agent, canister_id).await;
            let friendly = if has_friendly {
                Some((name.as_str(), environment_selection.name()))
            } else {
                None
            };

            if has_http {
                let canister_url = canister_gateway_url(http_gateway_url, canister_id, friendly);
                if json {
                    json_canisters.push(JsonDeployedCanister {
                        name: name.clone(),
                        canister_id,
                        url: Some(canister_url.to_string()),
                    });
                } else {
                    println!("  {name}: {canister_url}");
                }
            } else {
                // For canisters without http_request, show the Candid UI URL
                let url = if let Some(ui_id) = get_candid_ui_id(ctx, environment_selection).await {
                    let domain = gateway_domain(http_gateway_url);
                    let mut candid_url = canister_gateway_url(http_gateway_url, ui_id, None);
                    if domain.is_some() {
                        candid_url.set_query(Some(&format!("id={canister_id}")));
                    } else {
                        candid_url.set_query(Some(&format!("canisterId={ui_id}&id={canister_id}")));
                    }
                    if !json {
                        println!("  {name} (Candid UI): {candid_url}");
                    }
                    Some(candid_url.to_string())
                } else {
                    if !json {
                        println!("  {name}: {canister_id} (Candid UI not available)");
                    }
                    None
                };
                if json {
                    json_canisters.push(JsonDeployedCanister {
                        name: name.clone(),
                        canister_id,
                        url,
                    });
                }
            }
        } else if json {
            json_canisters.push(JsonDeployedCanister {
                name: name.clone(),
                canister_id,
                url: None,
            });
        } else {
            println!("  {name}: {canister_id} (No gateway URL available)");
        }
    }

    if json {
        serde_json::to_writer(
            std::io::stdout(),
            &JsonDeploy {
                canisters: json_canisters,
            },
        )?;
    }

    Ok(())
}

#[derive(Serialize)]
struct JsonDeploy {
    canisters: Vec<JsonDeployedCanister>,
}

#[derive(Serialize)]
struct JsonDeployedCanister {
    name: String,
    canister_id: Principal,
    url: Option<String>,
}

/// Gets the Candid UI canister ID for the network
/// Returns None if the Candid UI ID cannot be determined
async fn get_candid_ui_id(
    ctx: &Context,
    environment_selection: &EnvironmentSelection,
) -> Option<Principal> {
    let env = ctx.get_environment(environment_selection).await.ok()?;

    match &env.network.configuration {
        NetworkConfiguration::Managed { managed: _ } => {
            // Try to get the candid UI ID from the network descriptor
            let nd = ctx.network.get_network_directory(&env.network).ok()?;
            if let Ok(Some(desc)) = nd.load_network_descriptor().await
                && let Some(candid_ui) = desc.candid_ui_canister_id
            {
                return Some(candid_ui);
            }
            // No Candid UI available for this managed network
            None
        }
        NetworkConfiguration::Connected { .. } => {
            // For connected networks, use the mainnet Candid UI
            Some(MAINNET_CANDID_UI_CID)
        }
    }
}
