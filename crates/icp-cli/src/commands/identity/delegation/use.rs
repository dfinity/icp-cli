use crate::context::Context;
use clap::{Args, ValueHint};
use clap_complete::ArgValueCandidates;
use icp::{fs::json, prelude::*};

use crate::identity::{
    delegation::DelegationChain,
    key,
    manifest::{DelegationKeyStorage, PemFormat},
};
use snafu::{ResultExt, Snafu};
use tracing::{info, warn};

/// Complete a pending delegation identity by providing a signed delegation chain
///
/// Reads the JSON output of `icp identity delegation sign` from a file and attaches
/// it to the named identity, making it usable for signing.
#[derive(Debug, Args)]
pub(crate) struct UseArgs {
    /// Name of the pending delegation identity to complete
    #[arg(add = ArgValueCandidates::new(crate::complete::identity_names))]
    name: String,

    /// Path to the delegation chain JSON file (output of `icp identity delegation sign`)
    #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath)]
    from_json: PathBuf,
}

pub(crate) async fn exec(ctx: &Context, args: &UseArgs) -> Result<(), UseError> {
    let chain: DelegationChain = json::load(&args.from_json)?;

    let storage = ctx
        .identity_dirs()?
        .with_write(async |dirs| key::complete_delegation(dirs, &args.name, &chain))
        .await?
        .context(CompleteSnafu)?;

    info!("Identity `{}` delegation complete", args.name);

    if matches!(
        storage,
        DelegationKeyStorage::Pem {
            format: PemFormat::Plaintext
        }
    ) {
        warn!(
            "This identity is stored in plaintext and is not secure. Do not use it for anything of significant value."
        );
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum UseError {
    #[snafu(transparent)]
    LoadDelegationChain { source: json::Error },

    #[snafu(transparent)]
    LockIdentityDir { source: icp::fs::lock::LockError },

    #[snafu(display("failed to complete delegation identity"))]
    Complete {
        source: key::CompleteDelegationError,
    },
}
