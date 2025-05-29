use super::DEFAULT_DERIVATION_PATH;
use crate::env::Env;
use bip32::XPrv;
use bip39::{Language, Mnemonic, MnemonicType, Seed};
use camino::Utf8PathBuf;
use clap::Parser;
use icp_fs::fs;
use icp_identity::{
    CreateIdentityError,
    key::{CreateFormat, IdentityKey},
};
use k256::SecretKey;
use parse_display::Display;
use serde::Serialize;
use snafu::{ResultExt, Snafu};

#[derive(Debug, Parser)]
pub struct NewCmd {
    name: String,
    #[arg(long, value_name = "FILE")]
    output_seed: Option<Utf8PathBuf>,
}

pub fn exec(env: &Env, cmd: NewCmd) -> Result<NewIdentityMessage, NewIdentityError> {
    let mnemonic = Mnemonic::new(MnemonicType::for_key_size(256).unwrap(), Language::English);
    let path = DEFAULT_DERIVATION_PATH.parse().unwrap();
    let seed = Seed::new(&mnemonic, "");
    let pk = XPrv::derive_from_path(seed.as_bytes(), &path).context(DerivationSnafu)?;
    let key = SecretKey::from(pk.private_key());
    icp_identity::key::create_identity(
        env.dirs(),
        &cmd.name,
        IdentityKey::Secp256k1(key),
        CreateFormat::Plaintext,
    )?;
    if let Some(out_file) = cmd.output_seed {
        fs::write(&out_file, mnemonic.to_string().as_bytes())?;
        Ok(NewIdentityMessage::WrittenToFile { out_file })
    } else {
        Ok(NewIdentityMessage::Created {
            seed_phrase: mnemonic.to_string(),
        })
    }
}

#[derive(Debug, Snafu)]
pub enum NewIdentityError {
    #[snafu(transparent)]
    CreateIdentityError { source: CreateIdentityError },
    #[snafu(transparent)]
    WriteSeedFileError { source: fs::WriteFileError },
    #[snafu(display("failed to derive IC key from wallet seed"))]
    DerivationError { source: bip32::Error },
}

#[derive(Serialize, Display)]
#[serde(
    tag = "action",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum NewIdentityMessage {
    #[display("Seed phrase written to file {out_file}")]
    WrittenToFile { out_file: Utf8PathBuf },
    #[display("Your seed phrase: {seed_phrase}")]
    Created { seed_phrase: String },
}
