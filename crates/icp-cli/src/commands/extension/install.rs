use std::io::{IsTerminal, stderr, stdin};

use anyhow::{anyhow, bail};
use candid::Principal;
use clap::{Args, CommandFactory};
use dialoguer::Confirm;
use icp_app::context::Context;
use icp_app::extension::{Installed, download, sha256};
use icp_project::prelude::*;
use tracing::info;
use url::Url;

/// Install an extension, or replace an installed one
#[derive(Args, Debug)]
pub(crate) struct InstallArgs {
    /// The extension's wasm component: an https URL or a local path
    source: String,

    /// Refuse the install unless the component has this sha256 (hex)
    #[arg(long)]
    sha256: Option<String>,

    /// Approve the install without asking
    #[arg(long, short)]
    yes: bool,
}

/// Canisters an approved extension could move a user's funds through, which
/// the approval prompt points out.
const LEDGERS: &[(&str, &str)] = &[
    ("ryjl3-tyaaa-aaaaa-aaaba-cai", "ICP"),
    ("um5iw-rqaaa-aaaaq-qaaba-cai", "cycles"),
    ("xevnm-gaaaa-aaaar-qafnq-cai", "ckUSDC"),
    ("mxzaz-hqaaa-aaaar-qaada-cai", "ckBTC"),
    ("ss2fx-dyaaa-aaaar-qacoq-cai", "ckETH"),
];

pub(crate) async fn exec(ctx: &Context, args: &InstallArgs) -> Result<(), anyhow::Error> {
    let (wasm, source) = if args.source.contains("://") {
        let url = Url::parse(&args.source)?;
        (download(&url).await?, url.to_string())
    } else {
        let path = PathBuf::from(&args.source);
        let path = path.canonicalize_utf8().unwrap_or(path);
        (icp_project::fs::read(&path)?, path.to_string())
    };

    let sha256 = sha256(&wasm);
    if let Some(expected) = &args.sha256
        && !expected.eq_ignore_ascii_case(&sha256)
    {
        bail!("the component's sha256 is {sha256}, not {expected} as --sha256 requires");
    }

    let info = icp_extension::describe(&wasm)?;
    icp_extension::validate(&info)?;
    if crate::Cli::command().find_subcommand(&info.name).is_some() {
        bail!(
            "the extension is named '{}', which is a built-in icp command",
            info.name
        );
    }

    let dir = ctx.dirs.extensions();
    let previous = icp_app::extension::get(&dir, &info.name).ok();

    eprintln!(
        "Extension  {} {} — {}",
        info.name, info.version, info.summary
    );
    eprintln!("Source     {source}");
    eprintln!("SHA-256    {sha256}");
    if let Some(previous) = &previous {
        eprintln!(
            "Replaces   {} {}",
            previous.info.name, previous.info.version
        );
    }
    if info.canisters.is_empty() {
        eprintln!("Calls no mainnet canister.");
    } else {
        eprintln!("Calls as your identity, on mainnet:");
        for canister in &info.canisters {
            eprintln!(
                "  {}  {}{}",
                canister.id,
                canister.label,
                warning(canister.id)
            );
        }
    }
    eprintln!("On other networks (local, testnets) it may call any canister.");

    let approved = if args.yes {
        true
    } else if stdin().is_terminal() && stderr().is_terminal() {
        Confirm::new()
            .with_prompt("Install?")
            .default(false)
            .interact()?
    } else if args.sha256.is_some() {
        true
    } else {
        bail!("nobody is at the terminal to approve the install: pass --yes or --sha256");
    };
    if !approved {
        return Err(anyhow!("install declined"));
    }

    let installed = Installed {
        info,
        sha256,
        source,
    };
    icp_app::extension::install(&dir, &wasm, &installed)?;
    info!(
        "Installed {} {}. Run `icp {} --help` to see its commands.",
        installed.info.name, installed.info.version, installed.info.name
    );
    Ok(())
}

fn warning(id: Principal) -> String {
    if id == Principal::management_canister() {
        return "  ⚠ the management canister: can act on every canister you control".into();
    }
    match LEDGERS.iter().find(|(ledger, _)| *ledger == id.to_text()) {
        Some((_, token)) => format!("  ⚠ a ledger: can move your {token}"),
        None => String::new(),
    }
}
