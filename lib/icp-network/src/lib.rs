pub mod access;
pub mod config;
mod directory;
mod managed;
pub mod status;
pub mod structure;

pub use config::{BindPort, ManagedNetworkModel, NetworkConfig};
pub use directory::NetworkDirectory;
pub use managed::run::{RunNetworkError, run_network};

pub const NETWORK_LOCAL: &str = "local";
pub const NETWORK_IC: &str = "ic";
