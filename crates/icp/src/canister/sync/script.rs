use tokio::sync::mpsc::Sender;

use crate::manifest::adapter::script::Adapter;

use super::Params;

use super::super::script::{ScriptError, execute};

pub(super) async fn sync(
    adapter: &Adapter,
    params: &Params,
    stdio: Option<Sender<String>>,
) -> Result<Vec<String>, ScriptError> {
    let mut envs: Vec<(String, String)> = vec![
        ("ICP_CLI_ENVIRONMENT".to_owned(), params.environment.clone()),
        ("ICP_CLI_NETWORK".to_owned(), params.network.clone()),
        ("ICP_CLI_CID".to_owned(), params.cid.to_text()),
    ];
    for (name, id) in &params.canister_ids {
        let key = format!(
            "ICP_CLI_CID_{}",
            name.to_uppercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
        );
        envs.push((key, id.to_text()));
    }
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    execute(adapter, params.path.as_ref(), &env_refs, stdio).await?;
    Ok(vec![])
}
