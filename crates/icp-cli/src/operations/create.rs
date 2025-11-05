use anyhow::anyhow;
use candid::{Decode, Encode, Nat, Principal};
use futures::{StreamExt, future::join_all, stream::FuturesOrdered};
use ic_agent::Agent;
use icp::{
    context::{Context, EnvironmentSelection, GetCanisterIdForEnvError},
    identity::IdentitySelection,
};
use icp_canister_interfaces::{
    cycles_ledger::{
        CYCLES_LEDGER_PRINCIPAL, CanisterSettingsArg, CreateCanisterArgs, CreateCanisterResponse,
        CreationArgs, SubnetSelectionArg,
    },
    cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL,
    registry::{GetSubnetForCanisterRequest, GetSubnetForCanisterResult, REGISTRY_PRINCIPAL},
};
use rand::seq::IndexedRandom;

use crate::{
    commands::canister::create::CanisterSettings,
    progress::{ProgressManager, ProgressManagerSettings},
};

pub(crate) async fn create_canisters(
    canister_names: Vec<&str>,
    ctx: &Context,
    environment_selection: &EnvironmentSelection,
    identity_selection: &IdentitySelection,
    subnet: Option<Principal>,
    controllers: Vec<Principal>,
    settings_override: CanisterSettings,
    cycles: u128,
) -> Result<Vec<Principal>, anyhow::Error> {
    let project = ctx.project.load().await?;

    // Validate canister names exist in project before loading environment/agent
    for name in &canister_names {
        project.ensure_canister_declared(name)?;
    }

    let env = ctx.get_environment(environment_selection).await?;

    // Validate canister names exist in environment before loading agent
    for name in &canister_names {
        env.ensure_canister_declared(name)?;
    }

    let agent = ctx
        .get_agent_for_env(identity_selection, environment_selection)
        .await?;

    let existing_canisters = join_all(
        env.canisters
            .values()
            .map(|(_, c)| ctx.get_canister_id_for_env(&c.name, environment_selection)),
    )
    .await
    .into_iter()
    .filter_map(Result::ok)
    .collect::<Vec<_>>();

    // If no names specified, nothing to create
    if canister_names.is_empty() {
        return Ok(vec![]);
    }

    // Determine which canisters to create (only those that don't have IDs yet)
    let mut canisters_to_create = Vec::new();
    for name in &canister_names {
        if matches!(
            ctx.get_canister_id_for_env(name, environment_selection)
                .await,
            Err(GetCanisterIdForEnvError::NotFound)
        ) {
            if let Some((path, canister)) = env.canisters.get(*name) {
                canisters_to_create.push((path, canister));
            }
        }
    }

    if canisters_to_create.is_empty() {
        return Ok(vec![]);
    }

    // Select which subnet to deploy the canisters to
    //
    // If we don't specify a subnet, then the CMC will choose a random subnet
    // for each canister. Ideally, a project's canister should all live on the same subnet.
    let subnet = match subnet {
        // Target specified subnet
        Some(v) => v,

        // No subnet specified, and no canisters exist
        // Target a random subnet
        None if existing_canisters.is_empty() => {
            let vs = get_available_subnets(&agent).await?;

            // Choose a random subnet
            vs.choose(&mut rand::rng())
                .expect("missing subnet id")
                .to_owned()
        }

        // No subnet specified, and some canisters exist
        // Target the same subnet as the first canister
        None => {
            get_subnet_of_canister(
                &agent,                                                   // agent
                existing_canisters.first().expect("missing canister id"), // id
            )
            .await?
        }
    };

    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });
    let controllers_ref = &controllers;

    for (_path, info) in canisters_to_create.iter() {
        let pb = progress_manager.create_progress_bar(&info.name);
        let create_fn = {
            let agent = agent.clone();
            let pb = pb.clone();
            async move {
                pb.set_message("Creating...");

                let settings = CanisterSettingsArg {
                    freezing_threshold: settings_override
                        .freezing_threshold
                        .or(info.settings.freezing_threshold)
                        .map(Nat::from),
                    controllers: if controllers_ref.is_empty() {
                        None
                    } else {
                        Some(controllers_ref.clone())
                    },
                    reserved_cycles_limit: settings_override
                        .reserved_cycles_limit
                        .or(info.settings.reserved_cycles_limit)
                        .map(Nat::from),
                    memory_allocation: settings_override
                        .memory_allocation
                        .or(info.settings.memory_allocation)
                        .map(Nat::from),
                    compute_allocation: settings_override
                        .compute_allocation
                        .or(info.settings.compute_allocation)
                        .map(Nat::from),
                };

                let creation_args = CreationArgs {
                    subnet_selection: Some(SubnetSelectionArg::Subnet { subnet }),
                    settings: Some(settings),
                };

                let arg = CreateCanisterArgs {
                    from_subaccount: None,
                    created_at_time: None,
                    amount: Nat::from(cycles),
                    creation_args: Some(creation_args),
                };

                // Call cycles ledger create_canister
                let resp = agent
                    .update(&CYCLES_LEDGER_PRINCIPAL, "create_canister")
                    .with_arg(Encode!(&arg)?)
                    .call_and_wait()
                    .await?;

                let resp: CreateCanisterResponse = Decode!(&resp, CreateCanisterResponse)?;

                let cid = match resp {
                    CreateCanisterResponse::Ok { canister_id, .. } => canister_id,
                    CreateCanisterResponse::Err(err) => {
                        return Err(anyhow!("failed to create canister: {err}"));
                    }
                };

                ctx.set_canister_id_for_env(&info.name, environment_selection, &cid)?;
                Ok::<_, anyhow::Error>(cid)
            }
        };

        futs.push_back(async move {
            ProgressManager::execute_with_custom_progress(
                &pb,
                create_fn,
                || "Created successfully".to_string(),
                |err| format!("failed to create canister: {err}"),
                |_| false,
            )
            .await
        });
    }

    // Consume the set of futures and abort if an error occurs
    let mut created_canisters = Vec::new();
    while let Some(res) = futs.next().await {
        created_canisters.push(res?);
    }

    Ok(created_canisters)
}

async fn get_subnet_of_canister(agent: &Agent, id: &Principal) -> Result<Principal, anyhow::Error> {
    let args = &GetSubnetForCanisterRequest {
        principal: Some(*id),
    };

    let bs = agent
        .query(&REGISTRY_PRINCIPAL, "get_subnet_for_canister")
        .with_arg(Encode!(args)?)
        .call()
        .await
        .map_err(|err| anyhow!("failed to fetch subnet for canister: {err}"))?;

    let resp = Decode!(&bs, GetSubnetForCanisterResult)
        .map_err(|err| anyhow!("failed to decode subnet for canister: {err}"))?;

    let out = resp
        .map_err(|err| anyhow!("failed to get subnet for canister: {err}"))?
        .subnet_id
        .ok_or(anyhow!("canister is not assigned to any subnet"))?;

    Ok(out)
}

async fn get_available_subnets(agent: &Agent) -> Result<Vec<Principal>, anyhow::Error> {
    let bs = agent
        .query(&CYCLES_MINTING_CANISTER_PRINCIPAL, "get_default_subnets")
        .with_arg(Encode!(&())?)
        .call()
        .await
        .map_err(|err| anyhow!("failed to get default subnets: {err}"))?;

    let resp = Decode!(&bs, Vec<Principal>)?;

    // Check if any subnets are available
    if resp.is_empty() {
        return Err(anyhow!("no available subnets found").into());
    }

    Ok(resp)
}
