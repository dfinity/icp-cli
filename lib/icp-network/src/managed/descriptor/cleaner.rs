use crate::NetworkDirectory;

pub struct NetworkDescriptorCleaner<'a> {
    network_directory: &'a NetworkDirectory,
    gateway_port: Option<u16>,
}

impl<'a> NetworkDescriptorCleaner<'a> {
    pub fn new(network_directory: &'a NetworkDirectory, gateway_port: Option<u16>) -> Self {
        Self {
            network_directory,
            gateway_port,
        }
    }
}

impl Drop for NetworkDescriptorCleaner<'_> {
    fn drop(&mut self) {
        let _ = self.network_directory.cleanup_project_network_descriptor();
        let _ = self
            .network_directory
            .cleanup_port_descriptor(self.gateway_port);
    }
}
