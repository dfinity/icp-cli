//! Host implementations of the `icp-deploy-canister` IO traits, backing the
//! library's install/sync/deploy core with the CLI's `ic-agent` transport and
//! on-disk stores.

use std::sync::Arc;

use async_trait::async_trait;
use candid::Principal;
use icp::prelude::*;
use icp::store_artifact;
use icp_deploy_canister::files::{FileAccess, FileAccessError};
use icp_deploy_canister::icp_access::{IcpAccess, IcpAccessError};

use super::proxy::update_or_proxy_raw;

/// [`IcpAccess`] over an `ic-agent` `Agent`. Proxy routing is baked in (the
/// impl is constructed with the proxy principal); the library never threads a
/// proxy per call. The caller's principal is captured once at construction.
pub struct AgentIcpAccess {
    agent: ic_agent::Agent,
    proxy: Option<Principal>,
    caller: Principal,
}

impl AgentIcpAccess {
    pub fn new(agent: ic_agent::Agent, proxy: Option<Principal>) -> Self {
        let caller = agent
            .get_principal()
            .unwrap_or_else(|_| Principal::anonymous());
        Self {
            agent,
            proxy,
            caller,
        }
    }
}

#[async_trait]
impl IcpAccess for AgentIcpAccess {
    async fn canister_update(
        &self,
        canister: Principal,
        method: &str,
        arg: Vec<u8>,
        effective_canister_id: Principal,
        cycles: u128,
    ) -> Result<Vec<u8>, IcpAccessError> {
        update_or_proxy_raw(
            &self.agent,
            canister,
            method,
            arg,
            self.proxy,
            Some(effective_canister_id),
            cycles,
        )
        .await
        .map_err(|e| IcpAccessError::Update {
            canister,
            method: method.to_owned(),
            message: e.to_string(),
        })
    }

    async fn read_canister_metadata(
        &self,
        canister: Principal,
        path: &str,
    ) -> Result<Option<Vec<u8>>, IcpAccessError> {
        // A read failure is treated as "metadata absent" (matching the previous
        // EOP-detection behavior), so a missing custom section never aborts an
        // install.
        Ok(self
            .agent
            .read_state_canister_metadata(canister, path)
            .await
            .ok())
    }

    fn caller_principal(&self) -> Principal {
        self.caller
    }
}

/// [`FileAccess`] backed by the canister build-artifact store. The library reads
/// a canister's built wasm via `read_file(artifact_path)`; here the "path" is the
/// canister's store key, resolved through the (locked) artifact store. Only
/// `read_file` is used by the install path; the other methods have benign
/// defaults.
pub struct ArtifactFileAccess(pub Arc<dyn store_artifact::Access>);

#[async_trait]
impl FileAccess for ArtifactFileAccess {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, FileAccessError> {
        self.0
            .lookup(path.as_str())
            .await
            .map_err(|e| FileAccessError::Read {
                path: path.to_owned(),
                message: e.to_string(),
            })
    }

    async fn read_to_string(&self, path: &Path) -> Result<String, FileAccessError> {
        let bytes = self.read_file(path).await?;
        String::from_utf8(bytes).map_err(|e| FileAccessError::Read {
            path: path.to_owned(),
            message: e.to_string(),
        })
    }

    async fn exists(&self, path: &Path) -> bool {
        self.0.lookup(path.as_str()).await.is_ok()
    }

    async fn is_file(&self, path: &Path) -> bool {
        self.exists(path).await
    }

    async fn is_dir(&self, _path: &Path) -> bool {
        false
    }

    async fn read_dir(&self, _path: &Path) -> Result<Vec<PathBuf>, FileAccessError> {
        Ok(Vec::new())
    }

    async fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        Some(path.to_owned())
    }
}
