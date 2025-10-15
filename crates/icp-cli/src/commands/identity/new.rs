use crate::commands::Context;
use bip39::{Language, Mnemonic, MnemonicType};
use clap::Parser;
use icp::identity::{
    key::{CreateFormat, CreateIdentityError, IdentityKey, create_identity},
    seed::derive_default_key_from_seed,
};
use icp::{fs::write, prelude::*};
use snafu::{ResultExt, Snafu};
use tracing::info;

#[derive(Debug, Parser)]
pub struct NewCmd {
    name: String,
    #[arg(long, value_name = "FILE")]
    output_seed: Option<PathBuf>,
}

pub fn exec(ctx: &Context, cmd: NewCmd) -> Result<(), NewIdentityError> {
    let mnemonic = Mnemonic::new(MnemonicType::for_key_size(256).unwrap(), Language::English);
    let key = derive_default_key_from_seed(&mnemonic);
    create_identity(
        &ctx.dirs.identity(),
        &cmd.name,
        IdentityKey::Secp256k1(key),
        CreateFormat::Plaintext,
    )?;
    if let Some(out_file) = cmd.output_seed {
        write(&out_file, mnemonic.to_string().as_bytes()).context(WriteSeedFileSnafu)?;
        info!("Seed phrase written to file {out_file}");
        Ok(())
    } else {
        info!("Your seed phrase: {mnemonic}");
        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum NewIdentityError {
    #[snafu(transparent)]
    CreateIdentityError { source: CreateIdentityError },

    #[snafu(display("failed to write seed file"))]
    WriteSeedFileError { source: icp::fs::Error },
}
