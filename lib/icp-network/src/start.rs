use crate::config::model::managed::{BindPort, ManagedNetworkModel};
use crate::config::model::network_descriptor::NetworkDescriptorModel;
use crate::structure::NetworkDirectoryStructure;
use icp_support::fs::{RemoveFileError, remove_file};
use icp_support::json::LoadJsonFileError;
use icp_support::process::process_running;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StartLocalNetworkError {
    #[error(transparent)]
    LoadJsonFile(#[from] LoadJsonFileError),

    #[error("already running (this project)")]
    AlreadyRunningThisProject,

    #[error("already running (other project)")]
    AlreadyRunningOtherProject,

    #[error(transparent)]
    RemoveFile(#[from] RemoveFileError),
}

pub fn start_local_network(
    config: ManagedNetworkModel,
    nds: NetworkDirectoryStructure,
) -> Result<(), StartLocalNetworkError> {
    let project_descriptor_path = nds.project_descriptor_path();
    // first check the connected network
    if project_descriptor_path.exists() {
        let project_network_descriptor = NetworkDescriptorModel::load(&project_descriptor_path)?;
        if let Some(pid) = project_network_descriptor.pid {
            if process_running(pid) {
                return Err(StartLocalNetworkError::AlreadyRunningThisProject);
            }
        }
        if let Some(port) = project_network_descriptor.port {
            let port_descriptor_path = NetworkDirectoryStructure::port_descriptor_path(port);
            if port_descriptor_path.exists() {
                let port_network_descriptor = NetworkDescriptorModel::load(&port_descriptor_path)?;
                if let Some(pid) = port_network_descriptor.pid {
                    if process_running(pid) {
                        return Err(StartLocalNetworkError::AlreadyRunningOtherProject);
                    }
                }
                remove_file(&port_descriptor_path)?;
            }
        }
        remove_file(&nds.project_descriptor_path())?;
    }

    // get port from network configuration
    if let BindPort::Fixed(port) = config.bind.port {
        let port_descriptor_path = NetworkDirectoryStructure::port_descriptor_path(port);
        if port_descriptor_path.exists() {
            let port_network_descriptor = NetworkDescriptorModel::load(&port_descriptor_path)?;
            if let Some(pid) = port_network_descriptor.pid {
                if process_running(pid) {
                    return Err(StartLocalNetworkError::AlreadyRunningOtherProject);
                }
            }
            remove_file(&port_descriptor_path)?;
        }
    }

    // get my own pid

    // we're going to have to start the process, then get the port (if dynamic),
    // and the root key.

    let network_descriptor = NetworkDescriptorModel {
        id: uuid::Uuid::new_v4(),
        pid: Some(std::process::id()),
        port: None,
        path: nds.network_root().to_path_buf(),
        root_key: "".to_string(),
    };

    // write the project network descriptor

    // write the port network descriptor (fixed port only)

    todo!()
}
