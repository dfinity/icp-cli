use crate::config::model::network_descriptor::NetworkDescriptorModel;
use crate::structure::NetworkDirectoryStructure;
use icp_support::fs::{remove_file, RemoveFileError};
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

pub fn start_local_network(nds: NetworkDirectoryStructure) -> Result<(), StartLocalNetworkError> {
    if nds.project_descriptor_path().exists() {
        let project_network_descriptor =
            NetworkDescriptorModel::load(&nds.project_descriptor_path())?;
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
    // if fixed port, try to load port network descriptor
    // if found and pid still running, report already running (other project)
    // otherwise delete port network descriptor

    todo!()
}
