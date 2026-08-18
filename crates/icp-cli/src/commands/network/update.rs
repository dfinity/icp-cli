use std::sync::{Arc, OnceLock};

use clap::Parser;
use icp::{context::Context, network::managed::cache::download_launcher_version};

use icp_events::TaskKind;

use crate::events::indicatif_reporter;

/// Update icp-cli-network-launcher to the latest version.
#[derive(Parser, Debug)]
pub struct UpdateArgs {}

pub async fn exec(ctx: &Context, _args: &UpdateArgs) -> Result<(), anyhow::Error> {
    let task = indicatif_reporter(ctx.debug).unlabelled_task(TaskKind::Spinner);
    task.message("Downloading latest icp-cli-network-launcher...");

    let pkg = ctx.dirs.package_cache()?;
    let version_slot: Arc<OnceLock<String>> = Arc::new(OnceLock::new());
    let version_capture = version_slot.clone();

    task.run(
        async move {
            pkg.with_write(async move |pkg| {
                let (ver, _path) =
                    download_launcher_version(pkg, "latest", &reqwest::Client::new()).await?;
                let _ = version_capture.set(ver);
                anyhow::Ok(())
            })
            .await?
        },
        move || {
            let ver = version_slot.get().map(String::as_str).unwrap();
            format!("Updated icp-cli-network-launcher to {ver}")
        },
        |err| format!("Failed to update icp-cli-network-launcher: {err}"),
    )
    .await
}
