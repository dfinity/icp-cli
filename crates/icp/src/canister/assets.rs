use anyhow::Context;
use async_trait::async_trait;
use ic_agent::Agent;
use tokio::sync::mpsc::Sender;

use crate::canister::sync::{Params, Step, Synchronize, SynchronizeError};

pub struct Assets;

#[async_trait]
impl Synchronize for Assets {
    async fn sync(
        &self,
        step: &Step,
        params: &Params,
        agent: &Agent,
        _: Option<Sender<String>>,
    ) -> Result<(), SynchronizeError> {
        // Adapter
        let adapter = match step {
            Step::Assets(v) => v,
            _ => panic!("expected assets adapter"),
        };

        // Prepare canister client
        let canister = ic_utils::Canister::builder()
            .with_canister_id(params.cid)
            .with_agent(agent)
            .build()
            .context("failed to build canister client")?;

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
        .context("failed to synchronize assets canister")?;

        Ok(())
    }
}
