use crate::env::Env;
use bip39::{Language, Mnemonic};
use camino::{Utf8Path, Utf8PathBuf};
use clap::{ArgGroup, Parser};
use dialoguer::Password;
use icp_fs::fs;
use icp_identity::{
    CreateIdentityError,
    key::{CreateFormat, IdentityKey},
    manifest::IdentityKeyAlgorithm,
};
use itertools::Itertools;
use k256::{Secp256k1, SecretKey};
use pem::Pem;
use pkcs8::{
    AssociatedOid, EncryptedPrivateKeyInfo, ObjectIdentifier, PrivateKeyInfo, SecretDocument,
    der::{Decode, pem::PemLabel},
};
use sec1::{EcParameters, EcPrivateKey};
use snafu::{OptionExt, ResultExt, Snafu};

#[derive(Debug, Parser)]
#[command(group(ArgGroup::new("import-from").required(true)))]
pub struct ImportCmd {
    name: String,

    #[arg(long, value_name = "FILE", group = "import-from")]
    from_pem: Option<Utf8PathBuf>,

    #[arg(long, group = "import-from")]
    read_seed_phrase: bool,

    #[arg(long, value_name = "FILE", group = "import-from")]
    from_seed_file: Option<Utf8PathBuf>,

    #[arg(long, value_name = "FILE", requires = "from_pem")]
    decryption_password_from_file: Option<Utf8PathBuf>,

    #[arg(long, value_enum)]
    assert_key_type: Option<IdentityKeyAlgorithm>,
}

pub fn exec(env: &Env, cmd: ImportCmd) -> Result<(), ImportCmdError> {
    if let Some(from_pem) = cmd.from_pem {
        import_from_pem(
            env,
            &cmd.name,
            &from_pem,
            cmd.decryption_password_from_file.as_deref(),
            cmd.assert_key_type,
        )?;
    } else if let Some(path) = &cmd.from_seed_file {
        let phrase = fs::read_to_string(path).map_err(DeriveKeyError::from)?;
        import_from_seed_phrase(env, &cmd.name, &phrase)?;
    } else if cmd.read_seed_phrase {
        let phrase = Password::new()
            .with_prompt("Enter seed phrase")
            .with_confirmation("Re-enter seed phrase", "Seed phrases do not match")
            .interact()
            .context(ReadSeedPhraseFromTerminalSnafu)?;
        import_from_seed_phrase(env, &cmd.name, &phrase)?;
    } else {
        unreachable!();
    }
    println!("Identity \"{}\" created", cmd.name);
    Ok(())
}

#[derive(Snafu, Debug)]
pub enum ImportCmdError {
    #[snafu(transparent)]
    PemImport { source: LoadKeyError },
    #[snafu(transparent)]
    SeedImport { source: DeriveKeyError },
}

fn import_from_pem(
    env: &Env,
    name: &str,
    path: &Utf8Path,
    decryption_password_file: Option<&Utf8Path>,
    known_key_type: Option<IdentityKeyAlgorithm>,
) -> Result<(), LoadKeyError> {
    let pem = fs::read_to_string(path)?;
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
    let key = match section.tag() {
        PrivateKeyInfo::PEM_LABEL | EncryptedPrivateKeyInfo::PEM_LABEL => {
            import_pkcs8(section, path, decryption_password_file, known_key_type)?
        }
        EcPrivateKey::PEM_LABEL => import_sec1(
            section,
            sections.iter().find(|s| s.tag() == "EC PARAMETERS"),
            path,
            known_key_type,
        )?,
        _ => unreachable!(),
    };
    icp_identity::key::create_identity(env.dirs(), name, key, CreateFormat::Plaintext)?;
    Ok(())
}

fn import_pkcs8(
    section: &Pem,
    path: &Utf8Path,
    decryption_password_file: Option<&Utf8Path>,
    known_key_type: Option<IdentityKeyAlgorithm>,
) -> Result<IdentityKey, LoadKeyError> {
    let decrypted_doc: SecretDocument;
    let pki = if section.tag() == PrivateKeyInfo::PEM_LABEL {
        PrivateKeyInfo::from_der(section.contents()).context(BadPemContentSnafu { path })?
    } else {
        let epki = EncryptedPrivateKeyInfo::from_der(section.contents())
            .context(BadPemContentSnafu { path })?;
        let password = if let Some(path) = decryption_password_file {
            fs::read_to_string(path)?
        } else {
            Password::new()
                .with_prompt(format!("Enter the password to decrypt {path}"))
                .interact()
                .context(PasswordTermReadSnafu)?
        };
        decrypted_doc = epki
            .decrypt(&password)
            .context(DecryptionFailedSnafu { path })?;
        decrypted_doc
            .decode_msg::<PrivateKeyInfo>()
            .context(BadPemContentSnafu { path })?
    };
    if let Some(known_key_type) = known_key_type {
        match known_key_type {
            IdentityKeyAlgorithm::Secp256k1 => Ok(IdentityKey::Secp256k1(
                SecretKey::from_sec1_der(pki.private_key).context(BadPemKeySnafu { path })?,
            )),
            // todo p256, ed25519
        }
    } else {
        match pki.algorithm.oid {
            elliptic_curve::ALGORITHM_OID => {
                let curve = pki
                    .algorithm
                    .parameters_oid()
                    .ok()
                    .context(IncompletePemKeySnafu {
                        field: "parameters",
                        path,
                    })?;
                match curve {
                    Secp256k1::OID => Ok(IdentityKey::Secp256k1(
                        SecretKey::from_sec1_der(pki.private_key)
                            .context(BadPemKeySnafu { path })?,
                    )),
                    // todo p256
                    _ => UnsupportedAlgorithmSnafu {
                        found: curve,
                        expected: vec![Secp256k1::OID],
                        path,
                    }
                    .fail(),
                }
            }
            // todo ed25519
            _ => UnsupportedAlgorithmSnafu {
                found: pki.algorithm.oid,
                expected: vec![elliptic_curve::ALGORITHM_OID],
                path,
            }
            .fail(),
        }
    }
}

fn import_sec1(
    section: &Pem,
    param_section: Option<&Pem>,
    path: &Utf8Path,
    known_key_type: Option<IdentityKeyAlgorithm>,
) -> Result<IdentityKey, LoadKeyError> {
    let epk = EcPrivateKey::from_der(section.contents()).context(BadPemContentSnafu { path })?;
    if let Some(known_key_type) = known_key_type {
        match known_key_type {
            IdentityKeyAlgorithm::Secp256k1 => Ok(IdentityKey::Secp256k1(
                SecretKey::from_slice(epk.private_key).context(BadPemKeySnafu { path })?,
            )),
            // todo p256
        }
    } else {
        let params = if let Some(params) = epk.parameters {
            params
        } else if let Some(param_section) = param_section {
            EcParameters::from_der(param_section.contents()).context(BadPemContentSnafu { path })?
        } else {
            IncompletePemKeySnafu {
                field: "parameters",
                path,
            }
            .fail()?
        };
        let Some(curve) = params.named_curve() else {
            return IncompletePemKeySnafu {
                field: "namedCurve",
                path,
            }
            .fail();
        };
        match curve {
            Secp256k1::OID => Ok(IdentityKey::Secp256k1(
                SecretKey::from_slice(epk.private_key).context(BadPemKeySnafu { path })?,
            )),
            //todo p256
            _ => UnsupportedAlgorithmSnafu {
                found: curve,
                expected: vec![Secp256k1::OID],
                path,
            }
            .fail(),
        }
    }
}

fn import_from_seed_phrase(env: &Env, name: &str, phrase: &str) -> Result<(), DeriveKeyError> {
    let mnemonic = Mnemonic::from_phrase(phrase, Language::English).context(ParseMnemonicSnafu)?;
    let key = icp_identity::seed::derive_default_key_from_seed(&mnemonic);
    icp_identity::key::create_identity(
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

    #[snafu(transparent)]
    ReadFileError { source: fs::ReadFileError },

    #[snafu(display("expected 1 key block in PEM file `{path}`, found {count}"))]
    TooManyKeyBlocks { path: Utf8PathBuf, count: usize },

    #[snafu(display("corrupted PEM file `{path}`"))]
    BadPemFile {
        path: Utf8PathBuf,
        source: pem::PemError,
    },

    #[snafu(display("malformed key in PEM file `{path}`"))]
    BadPemContent {
        path: Utf8PathBuf,
        source: pkcs8::der::Error,
    },

    #[snafu(display("incomplete key in PEM file `{path}`: missing field `{field}`"))]
    IncompletePemKey { path: Utf8PathBuf, field: String },

    #[snafu(display("malformed key material in PEM file `{path}`"))]
    BadPemKey {
        path: Utf8PathBuf,
        source: elliptic_curve::Error,
    },

    #[snafu(display("failed to read password from terminal"))]
    PasswordTermReadError { source: dialoguer::Error },

    #[snafu(display("PEM file `{path}` uses unsupported algorithm {found}, expected {}", expected.iter().format(", ")))]
    UnsupportedAlgorithm {
        path: Utf8PathBuf,
        found: ObjectIdentifier,
        expected: Vec<ObjectIdentifier>,
    },

    #[snafu(display("failed to decrypt PEM file `{path}`"))]
    DecryptionFailed {
        path: Utf8PathBuf,
        source: pkcs8::Error,
    },

    #[snafu(transparent)]
    CreateIdentityError { source: CreateIdentityError },
}

#[derive(Debug, Snafu)]
pub enum DeriveKeyError {
    #[snafu(transparent)]
    ReadSeedFileError { source: fs::ReadFileError },

    #[snafu(display("failed to read seed phrase from terminal"))]
    ReadSeedPhraseFromTerminalError { source: dialoguer::Error },

    #[snafu(display("failed to parse seed phrase"))]
    ParseMnemonicError { source: bip39::ErrorKind },

    #[snafu(transparent)]
    CreateIdentityError { source: CreateIdentityError },
}
