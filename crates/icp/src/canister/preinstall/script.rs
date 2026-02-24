use tokio::sync::mpsc::Sender;

use super::super::script::{ScriptError, execute};
use super::Params;
use crate::manifest::adapter::script::Adapter;

pub(super) async fn preinstall(
    adapter: &Adapter,
    params: &Params,
    stdio: Option<Sender<String>>,
) -> Result<(), ScriptError> {
    execute(
        adapter,
        params.path.as_ref(),
        &[
            ("ICP_CANISTER_PATH", params.path.as_str()),
            ("ICP_WASM_PATH", params.wasm_path.as_str()),
            ("ICP_CANISTER_ID", &params.cid.to_string()),
            ("ICP_ENVIRONMENT", &params.environment),
        ],
        stdio,
    )
    .await
}
