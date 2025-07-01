// A network run by icp-cli for a test. These fields are read from the network descriptor
// after starting the network.
pub struct TestNetwork {
    pub gateway_port: u16,
    pub root_key: String,
}

// A network run by icp-cli, but set up in ~/.config/dfx/networks.json for dfx to connect to.
pub struct TestNetworkForDfx {
    pub dfx_network_name: String,
    pub gateway_port: u16,
}
