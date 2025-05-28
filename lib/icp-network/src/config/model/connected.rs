use serde::Deserialize;

/// A "connected network" is a network that we connect to but don't manage.
/// Typical examples are mainnet or testnets.
#[derive(Deserialize)]
pub struct ConnectedNetworkModel {
    // /// The URL(s) this network can be reached at.
    // providers: Vec<String>,
}
