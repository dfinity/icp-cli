use bip32::XPrv;
use bip39::{Language, Mnemonic, Seed};
use camino::{Utf8Path, Utf8PathBuf};
use clap::{ArgGroup, Parser};
use dialoguer::Password;
use icp_identity::{CreateFormat, CreateIdentityError, IdentityKey};
use itertools::Itertools;
use k256::{Secp256k1, SecretKey};
use parse_display::Display;
use pkcs8::{
    AssociatedOid, EncryptedPrivateKeyInfo, ObjectIdentifier, PrivateKeyInfo, SecretDocument,
    der::{Decode, pem::PemLabel},
};
use sec1::{EcParameters, EcPrivateKey};
use serde::Serialize;
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use std::{fs, io};

use crate::env::Env;

use super::DEFAULT_DERIVATION_PATH;

#[derive(Parser)]
#[command(group(ArgGroup::new("import-from").required(true)))]
pub struct ImportCmd {
    name: String,
    #[arg(long, group = "import-from")]
    from_pem: Option<Utf8PathBuf>,
    #[arg(long, group = "import-from")]
    read_seed_phrase: bool,
    #[arg(long, group = "import-from")]
    from_seed_file: Option<Utf8PathBuf>,
}

pub fn exec(env: &Env, cmd: ImportCmd) -> Result<LoadKeyMessage, ImportCmdError> {
    if let Some(from_pem) = cmd.from_pem {
        import_from_pem(env, &cmd.name, &from_pem)?;
    } else if let Some(path) = &cmd.from_seed_file {
        let phrase = fs::read_to_string(path).context(ReadSeedFileSnafu { path })?;
        import_from_seed_phrase(env, &cmd.name, &phrase)?;
    } else if cmd.read_seed_phrase {
        let phrase = Password::new()
            .with_prompt("Enter seed phrase")
            .interact()
            .context(ReadSeedPhraseFromTerminalSnafu)?;
        import_from_seed_phrase(env, &cmd.name, &phrase)?;
    } else {
        unreachable!();
    }
    Ok(LoadKeyMessage { name: cmd.name })
}

#[derive(Serialize, Display)]
#[display("Identity \"{name}\" created")]
pub struct LoadKeyMessage {
    name: String,
}

#[derive(Snafu, Debug)]
pub enum ImportCmdError {
    #[snafu(transparent)]
    PemImport { source: LoadKeyError },
    #[snafu(transparent)]
    SeedImport { source: DeriveKeyError },
}

fn import_from_pem(env: &Env, name: &str, path: &Utf8Path) -> Result<(), LoadKeyError> {
    let pem = fs::read_to_string(path).context(ReadFileSnafu { path })?;
    let sections = pem::parse_many(&pem).context(BadPemFileSnafu { path })?;
    let section = match sections
        .iter()
        .filter(|s| {
            s.tag() == PrivateKeyInfo::PEM_LABEL
                || s.tag() == EcPrivateKey::PEM_LABEL
                || s.tag() == EncryptedPrivateKeyInfo::PEM_LABEL
        })
        .exactly_one()
    {
        Ok(section) => section,
        Err(e) => {
            let count = e.count();
            if count == 0 {
                UnknownPemFormatSnafu {
                    expected: vec![
                        PrivateKeyInfo::PEM_LABEL,
                        EcPrivateKey::PEM_LABEL,
                        EncryptedPrivateKeyInfo::PEM_LABEL,
                    ],
                    found: sections.iter().map(|s| s.tag().to_string()).collect_vec(),
                }
                .fail()?
            } else {
                TooManyKeyBlocksSnafu { count, path }.fail()?
            }
        }
    };
    let decrypted_doc: SecretDocument;
    let key = match section.tag() {
        PrivateKeyInfo::PEM_LABEL | EncryptedPrivateKeyInfo::PEM_LABEL => {
            let pki = if section.tag() == PrivateKeyInfo::PEM_LABEL {
                PrivateKeyInfo::from_der(section.contents()).context(BadPemContentSnafu { path })?
            } else {
                let epki = EncryptedPrivateKeyInfo::from_der(section.contents())
                    .context(BadPemContentSnafu { path })?;
                let password = Password::new()
                    .with_prompt(format!("Enter the password to decrypt {path}"))
                    .interact()
                    .context(PasswordReadSnafu)?;
                decrypted_doc = epki
                    .decrypt(&password)
                    .context(DecryptionFailedSnafu { path })?;
                decrypted_doc
                    .decode_msg::<PrivateKeyInfo>()
                    .context(BadPemContentSnafu { path })?
            };
            ensure!(
                pki.algorithm.oid == elliptic_curve::ALGORITHM_OID,
                UnsupportedAlgorithmSnafu {
                    found: pki.algorithm.oid,
                    path,
                },
            );
            let curve = pki
                .algorithm
                .parameters_oid()
                .ok()
                .context(BadPemKeyStructureSnafu {
                    info: "missing field `parameters`",
                    path,
                })?;
            ensure!(
                curve == Secp256k1::OID,
                UnsupportedAlgorithmSnafu { found: curve, path }
            );
            SecretKey::from_slice(pki.private_key).context(BadPemKeySnafu { path })?
        }
        EcPrivateKey::PEM_LABEL => {
            let epk =
                EcPrivateKey::from_der(section.contents()).context(BadPemContentSnafu { path })?;
            let params = match epk.parameters {
                Some(params) => params,
                None => {
                    if let Some(param_section) =
                        sections.iter().find(|s| s.tag() == "EC PARAMETERS")
                    {
                        EcParameters::from_der(param_section.contents())
                            .context(BadPemContentSnafu { path })?
                    } else {
                        BadPemKeyStructureSnafu {
                            info: "missing field `parameters`",
                            path,
                        }
                        .fail()?
                    }
                }
            };
            let Some(curve) = params.named_curve() else {
                return BadPemKeyStructureSnafu {
                    info: "missing field `namedCurve`",
                    path,
                }
                .fail();
            };
            ensure!(
                curve == Secp256k1::OID,
                UnsupportedAlgorithmSnafu { found: curve, path },
            );
            SecretKey::from_slice(epk.private_key).context(BadPemKeySnafu { path })?
        }
        _ => unreachable!(),
    };
    icp_identity::create_identity(
        env.dirs(),
        name,
        IdentityKey::Secp256k1(key),
        CreateFormat::Plaintext,
    )?;
    Ok(())
}

fn import_from_seed_phrase(env: &Env, name: &str, phrase: &str) -> Result<(), DeriveKeyError> {
    let mnemonic = Mnemonic::from_phrase(phrase, Language::English).context(ParseMnemonicSnafu)?;
    let path = DEFAULT_DERIVATION_PATH.parse().unwrap();
    let seed = Seed::new(&mnemonic, "");
    let pk = XPrv::derive_from_path(seed.as_bytes(), &path).context(DerivationSnafu)?;
    let key = SecretKey::from(pk.private_key());
    icp_identity::create_identity(
        env.dirs(),
        name,
        IdentityKey::Secp256k1(key),
        CreateFormat::Plaintext,
    )?;
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum LoadKeyError {
    #[snafu(display("unknown PEM formats: expected {}; found {}", expected.join(", "), found.join(", ")))]
    UnknownPemFormat {
        expected: Vec<&'static str>,
        found: Vec<String>,
    },
    #[snafu(display("failed to read file `{path}`: {source}"))]
    ReadFileError {
        path: Utf8PathBuf,
        source: io::Error,
    },
    #[snafu(display("expected 1 key block in PEM file `{path}`, found {count}"))]
    TooManyKeyBlocks { path: Utf8PathBuf, count: usize },
    #[snafu(display("corrupted PEM file `{path}`: {source}"))]
    BadPemFile {
        path: Utf8PathBuf,
        source: pem::PemError,
    },
    #[snafu(display("malformed key in PEM file `{path}`: {source}"))]
    BadPemContent {
        path: Utf8PathBuf,
        source: pkcs8::der::Error,
    },
    #[snafu(display("incomplete key in PEM file `{path}`: {info}"))]
    BadPemKeyStructure { path: Utf8PathBuf, info: String },
    #[snafu(display("malformed key material in PEM file `{path}`: {source}"))]
    BadPemKey {
        path: Utf8PathBuf,
        source: elliptic_curve::Error,
    },
    #[snafu(display("failed to read password: {source}"))]
    PasswordReadError { source: dialoguer::Error },
    #[snafu(display(
        "PEM file `{path}` uses unsupported algorithm {found}, expected {} (id-ecPublicKey)", // todo ed25519 support
        elliptic_curve::ALGORITHM_OID
    ))]
    UnsupportedAlgorithm {
        path: Utf8PathBuf,
        found: ObjectIdentifier,
    },
    #[snafu(display("failed to decrypt PEM file `{path}`: {source}"))]
    DecryptionFailed {
        path: Utf8PathBuf,
        source: pkcs8::Error,
    },
    #[snafu(transparent)]
    CreateIdentityError { source: CreateIdentityError },
}

#[derive(Debug, Snafu)]
pub enum DeriveKeyError {
    #[snafu(display("failed to read seed file `{path}`: {source}"))]
    ReadSeedFileError {
        path: Utf8PathBuf,
        source: io::Error,
    },
    #[snafu(display("failed to read seed phrase from terminal: {source}"))]
    ReadSeedPhraseFromTerminalError { source: dialoguer::Error },
    #[snafu(display("failed to parse seed phrase: {source}"))]
    ParseMnemonicError { source: bip39::ErrorKind },
    #[snafu(display("failed to derive IC key from wallet seed: {source}"))]
    DerivationError { source: bip32::Error },
    #[snafu(transparent)]
    CreateIdentityError { source: CreateIdentityError },
}
