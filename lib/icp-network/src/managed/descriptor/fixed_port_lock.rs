use crate::config::model::network_descriptor::NetworkDescriptorModel;
use crate::managed::descriptor::claim::LockFileClaim;
use camino::{Utf8Path, Utf8PathBuf};
use icp_fs::lock::RwFileLock;
use icp_fs::lockedjson::load_json_with_lock;
use snafu::Snafu;

pub struct FixedPortLock {
    file_lock: RwFileLock,
    port_descriptor_path: Utf8PathBuf,
    port: u16,
}

impl FixedPortLock {
    pub fn new(file_lock: RwFileLock, port_descriptor_path: &Utf8Path, port: u16) -> Self {
        Self {
            file_lock,
            port_descriptor_path: port_descriptor_path.to_path_buf(),
            port,
        }
    }

    pub fn try_acquire(
        &mut self,
    ) -> Result<LockFileClaim<'_>, AnotherProjectRunningOnSamePortError> {
        let lock_path = self.file_lock.path().to_path_buf();

        let guard = self.file_lock.rwlock_mut().try_write().map_err(|_| {
            let network_descriptor = load_json_with_lock(&self.port_descriptor_path)
                .ok()
                .flatten()
                .map(Box::new);
            AnotherProjectRunningOnSamePortError {
                network_descriptor,
                port: self.port,
            }
        })?;

        Ok(LockFileClaim::new(lock_path, guard))
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("port {port} is in use by the {} network of the project at '{}'",
  network_descriptor.as_ref().map(|nd| nd.network.clone()).unwrap_or_else(|| "<unknown>".to_string()),
  network_descriptor.as_ref().map(|nd| nd.project_dir.to_string()).unwrap_or_else(|| "<unknown>".to_string())))]
pub struct AnotherProjectRunningOnSamePortError {
    pub network_descriptor: Option<Box<NetworkDescriptorModel>>,
    pub port: u16,
}
