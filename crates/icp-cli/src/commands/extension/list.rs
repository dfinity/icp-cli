use clap::Args;
use icp_app::context::Context;

/// List installed extensions
#[derive(Args, Debug)]
pub(crate) struct ListArgs {}

pub(crate) async fn exec(ctx: &Context, _args: &ListArgs) -> Result<(), anyhow::Error> {
    let installed = icp_app::extension::list(&ctx.dirs.extensions());
    if installed.is_empty() {
        eprintln!("No extensions installed. Install one with `icp extension install <URL>`.");
    }
    for extension in installed {
        let info = &extension.info;
        println!(
            "{} {}\t{}\t{}",
            info.name, info.version, info.summary, extension.source
        );
    }
    Ok(())
}
