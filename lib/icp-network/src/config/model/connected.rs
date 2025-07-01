use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum RouteField {
    /// Single url
    Url(String),

    /// More than one url (route round-robin)
    Urls(Vec<String>),
}

/// A "connected network" is a network that we connect to but don't manage.
/// Typical examples are mainnet or testnets.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ConnectedNetworkModel {
    /// The URL(s) this network can be reached at.
    #[serde(flatten)]
    pub route: RouteField,

    /// The root key of this network
    pub root_key: Option<String>,
}
