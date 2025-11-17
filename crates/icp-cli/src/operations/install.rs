use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, AgentError, export::Principal};
use ic_utils::interfaces::{
    ManagementCanister, management_canister::builders::CanisterInstallMode,
};
use snafu::Snafu;
use std::sync::Arc;
use tracing::debug;

use crate::progress::{ProgressManager, ProgressManagerSettings};

#[derive(Debug, Snafu)]
pub enum InstallOperationError {
    #[snafu(display("Could not find build artifact for canister '{canister_name}'"))]
    ArtifactNotFound { canister_name: String },

    #[snafu(display("agent error: {source}"))]
    Agent { source: AgentError },
}

pub(crate) async fn install_canister(
    mgmt: &ManagementCanister<'_>,
    canister_id: &Principal,
    canister_name: &str,
    wasm: &[u8],
    mode: &str,
) -> Result<(), InstallOperationError> {
    let install_mode = match mode {
        "auto" => {
            let (status,) = mgmt
                .canister_status(canister_id)
                .await
                .map_err(|source| InstallOperationError::Agent { source })?;

            match status.module_hash {
                // Canister has had code installed to it.
                Some(_) => CanisterInstallMode::Upgrade(None),

                // Canister has not had code installed to it.
                None => CanisterInstallMode::Install,
            }
        }
        "install" => CanisterInstallMode::Install,
        "reinstall" => CanisterInstallMode::Reinstall,
        "upgrade" => CanisterInstallMode::Upgrade(None),
        _ => panic!("invalid install mode"),
    };

    // Install code to canister
    debug!("Install new canister code for {}", canister_name);
    mgmt.install_code(canister_id, wasm)
        .with_mode(install_mode)
        .await
        .map_err(|source| InstallOperationError::Agent { source })?;

    Ok(())
}

/// Installs code to multiple canisters and displays progress bars
pub(crate) async fn install_many(
    agent: Agent,
    canisters: Vec<(String, Principal)>,
    mode: &str,
    artifacts: Arc<dyn icp::store_artifact::Access>,
    debug: bool,
) -> Result<(), InstallOperationError> {
    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (name, cid) in canisters {
        let pb = progress_manager.create_progress_bar(&name);

        let install_fn = {
            let mgmt = ManagementCanister::create(&agent);
            let pb = pb.clone();
            let mode = mode.to_string();
            let artifacts = artifacts.clone();
            let name = name.clone();

            async move {
                pb.set_message("Installing...");

                // Lookup the canister build artifact
                let wasm = artifacts.lookup(&name).await.map_err(|_| {
                    InstallOperationError::ArtifactNotFound {
                        canister_name: name.clone(),
                    }
                })?;

                install_canister(&mgmt, &cid, &name, &wasm, &mode).await
            }
        };

        futs.push_back(async move {
            ProgressManager::execute_with_progress(
                &pb,
                install_fn,
                || "Installed successfully".to_string(),
                |err| format!("Failed to install canister: {err}"),
            )
            .await
        });
    }

    while let Some(res) = futs.next().await {
        res?;
    }

    Ok(())
}
