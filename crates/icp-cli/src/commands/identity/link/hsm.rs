use crate::context::Context;
use clap::{Args, ValueHint};
use dialoguer::Password;
use icp::prelude::*;

use crate::identity::{key::link_hsm_identity, manifest::IdentityList};
use snafu::{ResultExt, Snafu, ensure};
use tracing::info;

/// Link an HSM key to a new identity
#[derive(Debug, Args)]
pub(crate) struct HsmArgs {
    /// Name for the linked identity
    name: String,

    /// Path to the PKCS#11 module (shared library) for the HSM
    #[arg(long, value_hint = ValueHint::FilePath)]
    pkcs11_module: PathBuf,

    /// Slot index on the HSM device
    #[arg(long, default_value_t = 0)]
    slot: usize,

    /// Key ID on the HSM (e.g., "01" for PIV authentication key)
    #[arg(long)]
    key_id: String,

    /// Read HSM PIN from a file instead of prompting
    #[arg(long, value_hint = ValueHint::FilePath)]
    pin_file: Option<PathBuf>,
}

pub(crate) async fn exec(ctx: &Context, args: &HsmArgs) -> Result<(), HsmError> {
    ctx.identity_dirs()?
        .with_read(async |dirs| -> Result<(), HsmError> {
            let list = IdentityList::load_from(dirs).context(LoadIdentityListSnafu)?;
            ensure!(
                !list.identities.contains_key(&args.name),
                NameTakenSnafu { name: &args.name }
            );
            Ok(())
        })
        .await??;

    let pin_func: Box<dyn FnOnce() -> Result<String, String>> = match &args.pin_file {
        Some(path) => {
            let path = path.clone();
            Box::new(move || {
                icp::fs::read_to_string(&path)
                    .map(|s| s.trim().to_string())
                    .map_err(|e| e.to_string())
            })
        }
        None => Box::new(|| {
            Password::new()
                .with_prompt("Enter HSM PIN")
                .interact()
                .map_err(|e| e.to_string())
        }),
    };

    ctx.identity_dirs()?
        .with_write(async |dirs| {
            link_hsm_identity(
                dirs,
                &args.name,
                args.pkcs11_module.clone(),
                args.slot,
                args.key_id.clone(),
                pin_func,
            )
        })
        .await?
        .context(LinkHsmSnafu)?;

    info!("Identity `{}` linked to HSM", args.name);

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum HsmError {
    #[snafu(display("identity `{name}` already exists"))]
    NameTaken { name: String },

    #[snafu(display("failed to load identity list"))]
    LoadIdentityList {
        source: crate::identity::manifest::LoadIdentityManifestError,
    },

    #[snafu(transparent)]
    LockIdentityDir { source: icp::fs::lock::LockError },

    #[snafu(display("failed to link HSM identity"))]
    LinkHsm {
        source: crate::identity::key::LinkHsmIdentityError,
    },
}
