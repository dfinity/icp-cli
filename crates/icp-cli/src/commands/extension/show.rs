use std::io::stdout;

use clap::Args;
use icp_app::context::Context;

use super::{JsonExtension, runnable};

/// Show an installed extension: where it came from, its sha256 and version,
/// the canisters it may call, and its commands
#[derive(Args, Debug)]
pub(crate) struct ShowArgs {
    /// The name it is installed as
    name: String,

    /// Output as JSON
    #[arg(long)]
    json: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &ShowArgs) -> Result<(), anyhow::Error> {
    let installed = icp_app::extension::get(&ctx.dirs.extensions(), &args.name)?;
    if args.json {
        serde_json::to_writer(stdout(), &JsonExtension::of(&installed))?;
        println!();
        return Ok(());
    }

    let info = &installed.info;
    let state = match icp_app::extension::intact(&installed) {
        true => "",
        false => {
            "  ⚠ the kept component no longer has this hash; it will not run until reinstalled"
        }
    };
    println!("Name:       {}", installed.name);
    println!(
        "Extension:  {} {} — {}",
        info.name, info.version, info.summary
    );
    println!("Source:     {}", installed.source);
    println!("SHA-256:    {}{state}", installed.sha256);
    println!("Wasm:       {}", installed.wasm);
    println!("Installed:  {}", installed.installed_at);
    match info.canisters.is_empty() {
        true => println!("Calls no mainnet canister."),
        false => {
            println!("Calls on mainnet:");
            for canister in &info.canisters {
                println!("  {}  {}", canister.id, canister.label);
            }
        }
    }
    println!("Commands:");
    for command in runnable(&installed) {
        println!("  icp {command}");
    }
    Ok(())
}
