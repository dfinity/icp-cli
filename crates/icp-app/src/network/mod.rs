//! Running, describing and reaching networks on this machine.
//!
//! The *configuration* of a network is part of a project, and lives in
//! [`crate::network`]. Everything here is about the machine: launching a managed
//! network, the descriptors it writes, the friendly-domain file its gateway
//! serves, and resolving any network to endpoints and a root key.

pub mod accessor;
pub mod config;
pub mod custom_domains;
pub mod directory;
pub mod managed;
pub mod resolve;

#[cfg(any(test, feature = "test-util"))]
pub use accessor::UnimplementedMockDirectories;
pub use accessor::{Accessor, Directories, LocateNetworkDirectoryError};
pub use directory::{LoadPidError, NetworkDirectory, SavePidError};
pub use managed::run::{RunNetworkError, run_network};
pub use resolve::GetNetworkAccessError;
