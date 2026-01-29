use clap::Args;
use dialoguer::Password;
use icp::{context::Context, identity::key::link_hsm_identity, prelude::*};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Args)]
pub(crate) struct LinkArgs {
    /// Name for the linked identity
    name: String,

    /// Path to the PKCS#11 module (shared library) for the HSM
    #[arg(long)]
    hsm_pkcs11_module: PathBuf,

    /// Slot index on the HSM device
    #[arg(long, default_value_t = 0)]
    hsm_slot: usize,

    /// Key ID on the HSM (e.g., "01" for PIV authentication key)
    #[arg(long)]
    hsm_key_id: String,
}

pub(crate) async fn exec(ctx: &Context, args: &LinkArgs) -> Result<(), LinkError> {
    ctx.dirs
        .identity()?
        .with_write(async |dirs| {
            link_hsm_identity(
                dirs,
                &args.name,
                args.hsm_pkcs11_module.clone(),
                args.hsm_slot,
                args.hsm_key_id.clone(),
                || {
                    Password::new()
                        .with_prompt("Enter HSM PIN")
                        .interact()
                        .map_err(|e| e.to_string())
                },
            )
        })
        .await?
        .context(LinkHsmSnafu)?;

    println!("Identity \"{}\" linked to HSM", args.name);

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum LinkError {
    #[snafu(transparent)]
    LockIdentityDir { source: icp::fs::lock::LockError },

    #[snafu(display("failed to link HSM identity"))]
    LinkHsm {
        source: icp::identity::key::LinkHsmIdentityError,
    },
}
