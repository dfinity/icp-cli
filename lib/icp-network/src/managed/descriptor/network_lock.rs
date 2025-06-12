use crate::managed::descriptor::claim::LockFileClaim;
use icp_fs::lock::RwFileLock;
use snafu::Snafu;

pub struct NetworkLock {
    file_lock: RwFileLock,
    network_name: String,
}

impl NetworkLock {
    pub fn new(file_lock: RwFileLock, network_name: &str) -> Self {
        Self {
            file_lock,
            network_name: network_name.to_string(),
        }
    }

    pub fn try_acquire(&mut self) -> Result<LockFileClaim<'_>, ProjectNetworkAlreadyRunningError> {
        let path = self.file_lock.path().to_owned();
        let guard = self.file_lock.rwlock_mut().try_write().map_err(|_| {
            ProjectNetworkAlreadyRunningError {
                network: self.network_name.clone(),
            }
        })?;
        Ok(LockFileClaim::new(path, guard))
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("the {network} network for this project is already running"))]
pub struct ProjectNetworkAlreadyRunningError {
    pub network: String,
}
