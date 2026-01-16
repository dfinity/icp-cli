pub use camino::{FromPathBufError, Utf8Path as Path, Utf8PathBuf as PathBuf};

pub const TRILLION: u128 = 1_000_000_000_000;

pub const SECOND: u64 = 1;
pub const MINUTE: u64 = 60 * SECOND;

pub const IC_MAINNET_NETWORK_URL: &str = "https://icp-api.io";
/// Name of the implicit IC mainnet network and its implicit environment
pub const IC: &str = "ic";
/// Name of the implicit local managed network and its implicit environment
pub const LOCAL: &str = "local";
