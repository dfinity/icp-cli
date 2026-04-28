use std::sync::{Arc, OnceLock};

use clap::Parser;
use icp::{context::Context, network::managed::cache::download_launcher_version};

use crate::progress::{ProgressManager, ProgressManagerSettings};

/// Update icp-cli-network-launcher to the latest version.
#[derive(Parser, Debug)]
pub struct UpdateArgs {}

pub async fn exec(ctx: &Context, _args: &UpdateArgs) -> Result<(), anyhow::Error> {
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });
    let pb = progress_manager.create_independent_progress_bar();
    pb.set_message("Downloading latest icp-cli-network-launcher...".to_string());

    let pkg = ctx.dirs.package_cache()?;
    let version_slot: Arc<OnceLock<String>> = Arc::new(OnceLock::new());
    let version_capture = version_slot.clone();

    ProgressManager::execute_with_progress(
        &pb,
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
