use async_trait::async_trait;
use ic_agent::Agent;
use icp_deploy_canister::sync_exec::{PluginInvocation, ScriptInvocation};
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::package::PackageCache;

mod plugin;

#[derive(Debug, Snafu)]
pub enum SynchronizeError {
    #[snafu(transparent)]
    Script { source: super::script::ScriptError },

    #[snafu(transparent)]
    Plugin { source: plugin::PluginError },
}

/// Host execution of the two sync-step mechanisms that can't run inside a
/// canister: WASI plugins (wasmtime) and subprocess scripts.
///
/// Step dispatch and *all* input derivation (plugin dirs/files, the `ICP_CLI_*`
/// script environment) live in `icp-deploy-canister`; implementations here
/// receive a fully-resolved [`PluginInvocation`] / [`ScriptInvocation`] and
/// perform only the irreducible host action. This trait is the injection seam
/// the [`crate::context::Context`] carries so tests can stub it out.
#[async_trait]
pub trait Synchronize: Sync + Send {
    async fn run_plugin(
        &self,
        invocation: &PluginInvocation,
        agent: &Agent,
        stdio: Option<Sender<String>>,
        pkg_cache: &PackageCache,
    ) -> Result<Vec<String>, SynchronizeError>;

    async fn run_script(
        &self,
        invocation: &ScriptInvocation,
        stdio: Option<Sender<String>>,
    ) -> Result<Vec<String>, SynchronizeError>;
}

pub struct Syncer;

#[async_trait]
impl Synchronize for Syncer {
    async fn run_plugin(
        &self,
        invocation: &PluginInvocation,
        agent: &Agent,
        stdio: Option<Sender<String>>,
        pkg_cache: &PackageCache,
    ) -> Result<Vec<String>, SynchronizeError> {
        Ok(plugin::run(invocation, agent, stdio, pkg_cache).await?)
    }

    async fn run_script(
        &self,
        invocation: &ScriptInvocation,
        stdio: Option<Sender<String>>,
    ) -> Result<Vec<String>, SynchronizeError> {
        let env_refs: Vec<(&str, &str)> = invocation
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        super::script::execute_commands(&invocation.commands, &invocation.cwd, &env_refs, stdio)
            .await?;
        // Persistent stderr is a sync-plugin feature only; script steps don't
        // currently retain any output past the rolling step view.
        Ok(vec![])
    }
}

#[cfg(test)]
/// Unimplemented mock implementation of `Synchronize`.
/// All methods panic with `unimplemented!()` when called.
pub struct UnimplementedMockSyncer;

#[cfg(test)]
#[async_trait]
impl Synchronize for UnimplementedMockSyncer {
    async fn run_plugin(
        &self,
        _invocation: &PluginInvocation,
        _agent: &Agent,
        _stdio: Option<Sender<String>>,
        _pkg_cache: &PackageCache,
    ) -> Result<Vec<String>, SynchronizeError> {
        unimplemented!("UnimplementedMockSyncer::run_plugin")
    }

    async fn run_script(
        &self,
        _invocation: &ScriptInvocation,
        _stdio: Option<Sender<String>>,
    ) -> Result<Vec<String>, SynchronizeError> {
        unimplemented!("UnimplementedMockSyncer::run_script")
    }
}
