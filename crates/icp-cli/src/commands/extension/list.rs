use std::io::stdout;

use clap::Args;
use icp_app::context::Context;

use super::JsonExtension;

/// List installed extensions, one per line: name, version and source
#[derive(Args, Debug)]
pub(crate) struct ListArgs {
    /// Output the installed extensions as a JSON array, each as `show --json` prints it
    #[arg(long)]
    json: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &ListArgs) -> Result<(), anyhow::Error> {
    let installed = icp_app::extension::list(&ctx.dirs.extensions());
    if args.json {
        let all: Vec<_> = installed.iter().map(JsonExtension::of).collect();
        serde_json::to_writer(stdout(), &all)?;
        println!();
        return Ok(());
    }
    if installed.is_empty() {
        eprintln!(
            "No extensions installed. Install one with `icp extension install <NAME> <URL>`."
        );
    }
    let width = installed.iter().map(|e| e.name.len()).max().unwrap_or(0);
    let version_width = installed
        .iter()
        .map(|e| e.info.version.len())
        .max()
        .unwrap_or(0);
    for extension in &installed {
        println!(
            "{:width$}  {:version_width$}  {}",
            extension.name, extension.info.version, extension.source
        );
    }
    Ok(())
}
