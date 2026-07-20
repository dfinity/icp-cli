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

use candid::Principal;
use ic_agent::{Identity, identity::AnonymousIdentity};
use ic_management_canister_types::CanisterSettings;

use icp::agent::{Create, Creator};
use icp::canister::sync::{Params, Syncer, Synchronize};
use icp::canister::wasm;
use icp::deploy;
use icp::manifest::BuildStep;
use icp::package::PackageCache;
use icp::prelude::PathBuf;
use icp::project::load_project;

/// Deploy every canister of `environment` in the project at `project_dir`:
/// load + validate the manifest, then for each canister create it, install its
/// wasm, and run its sync steps.
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
    let controller = agent
        .get_principal()
        .map_err(Box::<dyn std::error::Error>::from)?;
    let pkg_cache = PackageCache::new(cache_root)?;
    let syncer = Syncer;

    // Ids of everything we deploy, so sync steps can wire canister references.
    let mut canister_ids: BTreeMap<String, Principal> = BTreeMap::new();

    for (name, (canister_dir, canister)) in &env.canisters {
        let wasm_bytes = load_prebuilt_wasm(canister, canister_dir, &pkg_cache).await?;

        // 2. Deploy the wasm: create the canister, then install code.
        //    Controllers/allocations come from the manifest; we add ourselves as
        //    a controller so later upgrades and asset syncs are permitted.
        let mut settings: CanisterSettings = canister.settings.clone().into();
        settings.controllers = Some(vec![controller]);
        let cid = deploy::create_canister_on_subnet(&agent, subnet, settings).await?;
        canister_ids.insert(name.clone(), cid);

        let mode = deploy::resolve_install_mode(&agent, cid).await?;
        let init_args = canister
            .init_args
            .as_ref()
            .map(|ia| ia.to_bytes())
            .transpose()?;
        deploy::install_wasm(&agent, cid, &wasm_bytes, mode, init_args.as_deref()).await?;

        // 3. Sync assets via the wasm plugin. An asset canister declares a
        //    `plugin` sync step pointing at the directory to upload; the syncer
        //    resolves the plugin wasm and runs it against the live canister.
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
