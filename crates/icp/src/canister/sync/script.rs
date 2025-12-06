use tokio::sync::mpsc::Sender;

use crate::manifest::adapter::script::Adapter;

use super::Params;

use super::super::script::{ScriptError, execute};

pub(super) async fn sync(
    adapter: &Adapter,
    params: &Params,
    stdio: Option<Sender<String>>,
) -> Result<(), ScriptError> {
    execute(adapter, params.path.as_ref(), &[], stdio).await
}
