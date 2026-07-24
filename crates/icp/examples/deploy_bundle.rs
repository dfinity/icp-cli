//! Deploy a marketplace app bundle programmatically, using only the **public**
//! API of the `icp` library crate — no `icp` subprocess.
//!
//! This example exists to prove the public surface is sufficient for a backend
//! service (e.g. dfinity/control-panel) to deploy prebuilt app bundles in-process.
//! It is compiled as an external consumer of `icp`, so it can only reach `pub`
//! items — if it compiles, the three capabilities below are reachable from
//! outside the crate.
//!
//! The three capabilities, and what each maps to:
//!
//! 1. Read, parse and validate the manifest: [`icp::project::load_project`]
//!    (returns a validated [`icp::Project`]).
//! 2. Deploy the wasms (create canisters + install code):
//!    [`icp::deploy::create_canister_on_subnet`],
//!    [`icp::deploy::resolve_install_mode`], [`icp::deploy::install_wasm`].
//! 3. Sync assets via the wasm plugin: [`icp::canister::sync::Syncer`] driving
//!    [`icp::canister::sync::Synchronize`].
//!
//! Run it with real arguments to perform live calls; with none it prints usage.
//! CI only needs it to *compile* — that is the point of the example.

use std::collections::BTreeMap;
use std::sync::Arc;

use candid::{Encode, Principal};
use ic_agent::{Agent, Identity, identity::AnonymousIdentity};
use ic_management_canister_types::{CanisterId, CanisterSettings, UpdateSettingsArgs};

use icp::agent::{Create, Creator};
use icp::canister::sync::{Params, Syncer, Synchronize};
use icp::canister::wasm;
use icp::deploy;
use icp::manifest::BuildStep;
use icp::package::PackageCache;
use icp::prelude::PathBuf;
use icp::project::load_project;

/// Deploy every canister of `environment` in the project at `project_dir` in
/// three explicit phases — create ALL, install ALL, sync ALL — mirroring the
/// CLI's ordering in `commands/deploy.rs`. Doing every `create_canister` up
/// front means the full name -> id map exists before we resolve controllers,
/// install code, or sync, so named-canister references always resolve and each
/// sync step receives the COMPLETE `Params::canister_ids`.
#[allow(clippy::too_many_arguments)]
async fn deploy_bundle(
    project_dir: &PathBuf,
    network_url: &str,
    environment: &str,
    subnet: Principal,
    identity: Arc<dyn Identity>,
    cache_root: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Read, parse and validate the manifest.
    let project = load_project(project_dir).await?;
    let env = project
        .environments
        .get(environment)
        .ok_or_else(|| format!("environment '{environment}' not found in project"))?;

    let agent = Creator.create(identity, network_url).await?;
    let deployer = agent
        .get_principal()
        .map_err(Box::<dyn std::error::Error>::from)?;
    let pkg_cache = PackageCache::new(cache_root)?;
    let syncer = Syncer;

    // Ids of everything we deploy, so controller refs and sync steps can wire
    // canister-name references to concrete principals.
    let mut canister_ids: BTreeMap<String, Principal> = BTreeMap::new();

    // Phase (a): create ALL canisters, building the full name -> id map.
    //
    // NOTE: `From<Settings> for CanisterSettings` hard-codes `controllers: None`,
    // silently dropping the manifest's declared controllers. We therefore create
    // with allocations only (the deployer becomes the sole controller by default)
    // and re-apply the configured controllers in a second pass below, once every
    // canister id is known — a named-canister controller can only be resolved
    // after the canister it names has been created.
    for (name, (_canister_dir, canister)) in &env.canisters {
        let settings: CanisterSettings = canister.settings.clone().into();
        let cid = deploy::create_canister_on_subnet(&agent, subnet, settings).await?;
        canister_ids.insert(name.clone(), cid);
    }

    // Phase (a, controllers): with the full map in hand, resolve each canister's
    // configured controllers (principals + named-canister refs) and append the
    // deployer without duplicates, then apply them via `update_settings`.
    for (name, (_canister_dir, canister)) in &env.canisters {
        let cid = canister_ids[name];
        let controllers = resolve_controllers(canister, &canister_ids, deployer)?;
        set_controllers(&agent, cid, controllers).await?;
    }

    // Phase (b): install ALL wasms.
    for (name, (canister_dir, canister)) in &env.canisters {
        let cid = canister_ids[name];
        let wasm_bytes = load_prebuilt_wasm(canister, canister_dir, &pkg_cache).await?;
        let mode = deploy::resolve_install_mode(&agent, cid).await?;
        let init_args = canister
            .init_args
            .as_ref()
            .map(|ia| ia.to_bytes())
            .transpose()?;
        deploy::install_wasm(&agent, cid, &wasm_bytes, mode, init_args.as_deref()).await?;
    }

    // Phase (c): sync ALL asset canisters. An asset canister declares a `plugin`
    // sync step pointing at the directory to upload; the syncer resolves the
    // plugin wasm and runs it against the live canister. Each step gets the
    // COMPLETE `canister_ids` map so cross-canister references resolve.
    for (name, (canister_dir, canister)) in &env.canisters {
        let cid = canister_ids[name];
        for step in &canister.sync.steps {
            let params = Params {
                path: canister_dir.clone(),
                cid,
                environment: env.name.clone(),
                network: env.network.name.clone(),
                canister_ids: canister_ids.clone(),
                proxy: None,
            };
            syncer.sync(step, &params, &agent, None, &pkg_cache).await?;
        }
        println!("deployed {name} -> {cid}");
    }

    Ok(())
}

/// Resolve a canister's manifest-declared controllers (principals and
/// named-canister references) against the full `canister_ids` map, then append
/// `deployer` without duplicates so later upgrades and asset syncs are permitted.
///
/// This exists because `From<Settings> for CanisterSettings` drops controllers,
/// so retaining the manifest's configured controllers is the caller's job.
fn resolve_controllers(
    canister: &icp::Canister,
    canister_ids: &BTreeMap<String, Principal>,
    deployer: Principal,
) -> Result<Vec<Principal>, Box<dyn std::error::Error>> {
    let refs = canister.settings.controllers.as_deref().unwrap_or_default();
    let (mut resolved, unresolved) = icp::canister::resolve_controllers(refs, canister_ids);
    if !unresolved.is_empty() {
        return Err(format!(
            "canister '{}' declares controller(s) not created in this deployment: {unresolved:?}",
            canister.name
        )
        .into());
    }
    if !resolved.contains(&deployer) {
        resolved.push(deployer);
    }
    Ok(resolved)
}

/// Apply `controllers` to `cid` via the management canister's `update_settings`.
/// Required because the create call can't carry controllers (see
/// [`resolve_controllers`]): `From<Settings> for CanisterSettings` drops them.
async fn set_controllers(
    agent: &Agent,
    cid: Principal,
    controllers: Vec<Principal>,
) -> Result<(), Box<dyn std::error::Error>> {
    let args = UpdateSettingsArgs {
        canister_id: CanisterId::from(cid),
        settings: CanisterSettings {
            controllers: Some(controllers),
            ..Default::default()
        },
        sender_canister_version: None,
    };
    agent
        .update(&Principal::management_canister(), "update_settings")
        .with_arg(Encode!(&args)?)
        .with_effective_canister_id(cid)
        .call_and_wait()
        .await?;
    Ok(())
}

/// Resolve a canister's prebuilt wasm to bytes. Marketplace bundles ship
/// prebuilt modules, so we look for a `pre-built` build step and read the file
/// (local path or cached download) it points at.
async fn load_prebuilt_wasm(
    canister: &icp::Canister,
    canister_dir: &PathBuf,
    pkg_cache: &PackageCache,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    for step in &canister.build.steps {
        if let BuildStep::Prebuilt(adapter) = step {
            let wasm_path = wasm::resolve(
                &adapter.source,
                canister_dir,
                adapter.sha256.as_deref(),
                None,
                pkg_cache,
            )
            .await?;
            return Ok(std::fs::read(&wasm_path)?);
        }
    }
    Err(format!(
        "canister '{}' has no prebuilt wasm build step",
        canister.name
    )
    .into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(project_dir), Some(network_url), Some(subnet)) =
        (args.next(), args.next(), args.next())
    else {
        eprintln!(
            "usage: deploy_bundle <project-dir> <network-url> <subnet-principal> [environment]\n\n\
             Wires icp's public API end-to-end: load+validate manifest -> create+install \
             each canister's wasm -> sync asset canisters via the plugin.\n\
             With valid arguments it performs live network calls; with none it prints this help."
        );
        return Ok(());
    };
    let environment = args.next().unwrap_or_else(|| "local".to_string());

    // A real backend passes its controlling identity here; anonymous cannot
    // create canisters, but is enough to exercise the wiring.
    let identity: Arc<dyn Identity> = Arc::new(AnonymousIdentity);

    deploy_bundle(
        &PathBuf::from(project_dir),
        &network_url,
        &environment,
        Principal::from_text(&subnet)?,
        identity,
        PathBuf::from("./.icp-deploy-example-cache"),
    )
    .await
}
