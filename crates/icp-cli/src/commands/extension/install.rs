use std::io::{IsTerminal, stderr, stdin};

use anyhow::{anyhow, bail};
use candid::Principal;
use clap::{Args, CommandFactory};
use dialoguer::Confirm;
use icp_app::context::Context;
use icp_app::extension::{Installed, check_name, download, sha256, wasm_path};
use icp_project::prelude::*;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tracing::info;
use url::Url;

/// Install an extension as a command of icp, or replace an installed one
#[derive(Args, Debug)]
pub(crate) struct InstallArgs {
    /// The command to install it as: `icp <NAME>`. Usually the name the
    /// extension gives itself, but any name works, e.g. `cyclepay-beta` to
    /// try a build beside the installed one
    name: String,

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
    // The name first: a bad one should fail before anything is downloaded.
    let name = &args.name;
    check_name(name)?;
    if crate::Cli::command().find_subcommand(name).is_some() || name == "help" {
        bail!("'{name}' is a built-in icp command; install the extension under another name");
    }

    let (wasm, source) = if args.source.contains("://") {
        let url = Url::parse(&args.source)?;
        (download(&url).await?, url.to_string())
    } else {
        let path = PathBuf::from(&args.source);
        // `dunce`, so that Windows records `C:\...` rather than `\\?\C:\...`.
        let path = dunce::canonicalize(&path)
            .ok()
            .and_then(|p| PathBuf::from_path_buf(p).ok())
            .unwrap_or(path);
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

    let dir = ctx.dirs.extensions();
    let previous = icp_app::extension::get(&dir, name).ok();

    eprintln!("Install as  icp {name}");
    eprintln!(
        "Extension   {} {} — {}",
        info.name, info.version, info.summary
    );
    eprintln!("Source      {source}");
    eprintln!("SHA-256     {sha256}");
    if let Some(previous) = &previous {
        eprintln!(
            "Replaces    {} {}, from {}",
            previous.info.name, previous.info.version, previous.source
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
        wasm: wasm_path(&dir, name, &sha256),
        name: name.clone(),
        source,
        sha256,

        installed_at: OffsetDateTime::now_utc()
            .replace_nanosecond(0)?
            .format(&Rfc3339)?,
        info,
    };
    icp_app::extension::install(&dir, &wasm, &installed)?;
    info!(
        "Installed {} {} as `icp {name}`. Run `icp {name} --help` to see its commands.",
        installed.info.name, installed.info.version
    );
    Ok(())
}

/// What the approval prompt points out about a canister the extension calls.
/// (It cannot be the management canister, which `validate` refuses.)
fn warning(id: Principal) -> String {
    match LEDGERS.iter().find(|(ledger, _)| *ledger == id.to_text()) {
        Some((_, token)) => format!("  ⚠ a ledger: can move your {token}"),
        None => String::new(),
    }
}
