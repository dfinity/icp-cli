use anyhow::Context as _;
use bip39::{Language, Mnemonic, MnemonicType};
use clap::Args;
use icp::{
    fs::write_string,
    identity::{
        key::{CreateFormat, IdentityKey, create_identity},
        seed::derive_default_key_from_seed,
    },
    prelude::*,
};

use icp::context::Context;

use crate::commands::identity::StorageMode;

#[derive(Debug, Args)]
pub(crate) struct NewArgs {
    /// Name for the new identity
    name: String,

    /// Where to store the private key
    #[arg(long, value_enum, default_value_t)]
    storage: StorageMode,

    /// Write the seed phrase to a file instead of printing to stdout
    #[arg(long, value_name = "FILE")]
    output_seed: Option<PathBuf>,
}

pub(crate) async fn exec(ctx: &Context, args: &NewArgs) -> Result<(), anyhow::Error> {
    let mnemonic = Mnemonic::new(
        MnemonicType::for_key_size(256).context("failed to get mnemonic type")?,
        Language::English,
    );
    let format = match args.storage {
        StorageMode::Plaintext => CreateFormat::Plaintext,
        StorageMode::Keyring => CreateFormat::Keyring,
    };

    ctx.dirs
        .identity()?
        .with_write(async |dirs| {
            create_identity(
                dirs,
                &args.name,
                IdentityKey::Secp256k1(derive_default_key_from_seed(&mnemonic)),
                format,
            )
        })
        .await??;

    match &args.output_seed {
        Some(path) => {
            write_string(path, mnemonic.as_ref()).context("failed to write seed file")?;
            println!("Seed phrase written to file {path}")
        }

        None => {
            println!("Your seed phrase: {mnemonic}");
        }
    }

    Ok(())
}
