use bip39::{Language, Mnemonic, MnemonicType};
use clap::Args;
use icp::{
    fs::write_string,
    identity::{
        key::{CreateFormat, CreateIdentityError, IdentityKey, create_identity},
        seed::derive_default_key_from_seed,
    },
    prelude::*,
};

use crate::commands::{Context, Mode};

#[derive(Debug, Args)]
pub(crate) struct NewArgs {
    name: String,
    #[arg(long, value_name = "FILE")]
    output_seed: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    CreateIdentityError(#[from] CreateIdentityError),

    #[error("failed to write seed file")]
    WriteSeedFileError(#[from] icp::fs::Error),
}

pub(crate) async fn exec(ctx: &Context, args: &NewArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global | Mode::Project(_) => {
            let mnemonic = Mnemonic::new(
                MnemonicType::for_key_size(256).expect("failed to get mnemonic type"),
                Language::English,
            );

            create_identity(
                &ctx.dirs.identity(),
                &args.name,
                IdentityKey::Secp256k1(derive_default_key_from_seed(&mnemonic)),
                CreateFormat::Plaintext,
            )?;

            match &args.output_seed {
                Some(path) => {
                    write_string(path, mnemonic.as_ref())?;
                    println!("Seed phrase written to file {path}")
                }

                None => {
                    println!("Your seed phrase: {mnemonic}");
                }
            }
        }
    }

    Ok(())
}
