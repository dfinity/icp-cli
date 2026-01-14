use clap::Parser;
use icp::{context::Context, network::managed::cache::download_launcher_version};

/// Update icp-cli-network-launcher to the latest version.
#[derive(Parser, Debug)]
pub struct UpdateArgs {}

pub async fn exec(ctx: &Context, _args: &UpdateArgs) -> Result<(), anyhow::Error> {
    let pkg = ctx.dirs.package_cache()?;
    let ver = pkg
        .with_write(async |pkg| {
            let (ver, _path) =
                download_launcher_version(pkg, "latest", &reqwest::Client::new()).await?;
            anyhow::Ok(ver)
        })
        .await??;
    ctx.term.write_line(&format!(
        "icp-cli-network-launcher has been updated to the latest version {ver}."
    ))?;
    Ok(())
}
