use anyhow::Context as _;
use clap::Args;
use dialoguer::Password;
use icp::context::Context;
use icp::fs::read_to_string;
use icp::identity::key::export_identity;
use icp::prelude::*;

#[derive(Debug, Args)]
pub(crate) struct ExportArgs {
    /// Name of the identity to export
    name: String,

    /// Read the password from a file instead of prompting (only required for identities created or imported with --storage password)
    #[arg(long, value_name = "FILE")]
    password_file: Option<PathBuf>,
}

pub(crate) async fn exec(ctx: &Context, args: &ExportArgs) -> Result<(), anyhow::Error> {
    let dirs = ctx.dirs.identity()?;

    let pem = dirs
        .with_read(async |dirs| {
            export_identity(dirs, &args.name, || {
                if let Some(path) = &args.password_file {
                    read_to_string(path)
                        .context("failed to read password file")
                        .map(|s| s.trim().to_string())
                        .map_err(|e| e.to_string())
                } else {
                    Password::new()
                        .with_prompt(format!("Enter the password for identity `{}`", args.name))
                        .interact()
                        .context("failed to read password from terminal")
                        .map_err(|e| e.to_string())
                }
            })
        })
        .await??;

    // Print the PEM to stdout
    println!("{}", pem.trim());

    Ok(())
}
