use clap::Args;
use dialoguer::Password;
use icp::{context::Context, identity::key::link_hsm_identity, prelude::*};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Args)]
pub(crate) struct HsmArgs {
    /// Name for the linked identity
    name: String,

    /// Path to the PKCS#11 module (shared library) for the HSM
    #[arg(long)]
    pkcs11_module: PathBuf,

    /// Slot index on the HSM device
    #[arg(long, default_value_t = 0)]
    slot: usize,

    /// Key ID on the HSM (e.g., "01" for PIV authentication key)
    #[arg(long)]
    key_id: String,

    /// Read HSM PIN from a file instead of prompting
    #[arg(long)]
    pin_file: Option<PathBuf>,
}

pub(crate) async fn exec(ctx: &Context, args: &HsmArgs) -> Result<(), HsmError> {
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

    ctx.dirs
        .identity()?
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

    println!("Identity \"{}\" linked to HSM", args.name);

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum HsmError {
    #[snafu(transparent)]
    LockIdentityDir { source: icp::fs::lock::LockError },

    #[snafu(display("failed to link HSM identity"))]
    LinkHsm {
        source: icp::identity::key::LinkHsmIdentityError,
    },
}
