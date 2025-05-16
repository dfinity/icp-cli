use serde::Deserialize;

#[derive(Deserialize, Default)]
pub struct ManagedNetworkModel {
    bind: BindModel,
}

#[derive(Deserialize)]
pub struct BindModel {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: Option<u16>,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> Option<u16> {
    Some(8000)
}

impl Default for BindModel {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}
