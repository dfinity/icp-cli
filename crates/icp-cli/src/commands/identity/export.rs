use anyhow::Context as _;
use clap::Args;
use dialoguer::Password;
use elliptic_curve::zeroize::Zeroizing;
use icp::context::Context;
use icp::fs::read_to_string;
use icp::identity::key::{ExportFormat, export_identity};
use icp::prelude::*;

/// Print the PEM file for the identity
#[derive(Debug, Args)]
pub(crate) struct ExportArgs {
    /// Name of the identity to export
    name: String,

    /// Read the password from a file instead of prompting (only required for identities created or imported with --storage password)
    #[arg(long, value_name = "FILE")]
    password_file: Option<PathBuf>,

    /// Encrypt the exported PEM with a password
    #[arg(long)]
    encrypt: bool,

    /// Read the encryption password from a file instead of prompting
    #[arg(long, value_name = "FILE", requires = "encrypt")]
    encryption_password_file: Option<PathBuf>,
}

pub(crate) async fn exec(ctx: &Context, args: &ExportArgs) -> Result<(), anyhow::Error> {
    let dirs = ctx.dirs.identity()?;

    // Read password if necessary
    let export_format = if args.encrypt {
        let password = if let Some(path) = &args.encryption_password_file {
            read_to_string(path)
                .context("failed to read encryption password file")?
                .trim()
                .to_string()
        } else {
            Password::new()
                .with_prompt("Enter password to encrypt exported identity")
                .with_confirmation("Confirm password", "Passwords do not match")
                .interact()
                .context("failed to read password from terminal")?
        };
        ExportFormat::Encrypted {
            password: Zeroizing::new(password),
        }
    } else {
        ExportFormat::Plaintext
    };

    // Export pem from storage
    let pem = dirs
        .with_read(async |dirs| {
            export_identity(dirs, &args.name, export_format, || {
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
