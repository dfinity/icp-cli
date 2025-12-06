use ic_agent::Agent;
use ic_utils::canister::CanisterBuilderError;
use snafu::prelude::*;

use crate::manifest::adapter::assets::Adapter;

use super::Params;

#[derive(Debug, Snafu)]
pub enum AssetsError {
    #[snafu(display("failed to build canister client"))]
    Client { source: CanisterBuilderError },

    #[snafu(display("failed to synchronize assets canister"))]
    Sync { source: ic_asset::error::SyncError },
}

pub(super) async fn sync(
    adapter: &Adapter,
    params: &Params,
    agent: &Agent,
) -> Result<(), AssetsError> {
    // Prepare canister client
    let canister = ic_utils::Canister::builder()
        .with_canister_id(params.cid)
        .with_agent(agent)
        .build()
        .context(ClientSnafu)?;

    // Normalize `dir` field based on whether it's a single dir or multiple.
    let dirs = adapter.dir.as_vec();

    #[allow(clippy::disallowed_types)]
    let dirs = dirs
        .iter()
        // Paths are specified relative to the canister path
        .map(|p| params.path.join(p))
        // Convert to PathBuf
        .map(std::path::PathBuf::from)
        .collect::<Vec<std::path::PathBuf>>();

    #[allow(clippy::disallowed_types)]
    let dirs: Vec<&std::path::Path> = dirs.iter().map(|p| p.as_path()).collect();

    // ic-asset requires a logger, so provide it a nop logger
    let logger = slog::Logger::root(slog::Discard, slog::o!());

    // Synchronize assets to canister
    ic_asset::sync(
        &canister, // canister
        &dirs,     // dirs
        false,     // no_delete
        &logger,   // logger
        None,      // progress
    )
    .await
    .context(SyncSnafu)?;

    Ok(())
}
