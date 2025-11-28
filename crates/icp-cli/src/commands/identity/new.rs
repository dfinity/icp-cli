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

#[derive(Debug, Args)]
pub(crate) struct NewArgs {
    name: String,
    #[arg(long, value_name = "FILE")]
    output_seed: Option<PathBuf>,
}

pub(crate) async fn exec(ctx: &Context, args: &NewArgs) -> Result<(), anyhow::Error> {
    let mnemonic = Mnemonic::new(
        MnemonicType::for_key_size(256).context("failed to get mnemonic type")?,
        Language::English,
    );

    ctx.dirs
        .identity()?
        .with_write(async |dirs| {
            create_identity(
                dirs,
                &args.name,
                IdentityKey::Secp256k1(derive_default_key_from_seed(&mnemonic)),
                CreateFormat::Plaintext,
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
