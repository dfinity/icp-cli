use crate::context::Context;
use bip39::{Language, Mnemonic};
use clap::{ArgGroup, Parser};
use dialoguer::Password;
use icp::prelude::*;
use icp_fs::fs;
use icp_identity::{
    key::{CreateFormat, CreateIdentityError, IdentityKey, create_identity},
    manifest::IdentityKeyAlgorithm,
    seed::derive_default_key_from_seed,
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
    from_pem: Option<PathBuf>,

    #[arg(long, group = "import-from")]
    read_seed_phrase: bool,

    #[arg(long, value_name = "FILE", group = "import-from")]
    from_seed_file: Option<PathBuf>,

    #[arg(long, value_name = "FILE", requires = "from_pem")]
    decryption_password_from_file: Option<PathBuf>,

    #[arg(long, value_enum)]
    assert_key_type: Option<IdentityKeyAlgorithm>,
}

pub fn exec(ctx: &Context, cmd: ImportCmd) -> Result<(), ImportCmdError> {
    if let Some(from_pem) = cmd.from_pem {
        import_from_pem(
            ctx,
            &cmd.name,
            &from_pem,
            cmd.decryption_password_from_file.as_deref(),
            cmd.assert_key_type,
        )?;
    } else if let Some(path) = &cmd.from_seed_file {
        let phrase = fs::read_to_string(path).map_err(DeriveKeyError::from)?;
        import_from_seed_phrase(ctx, &cmd.name, &phrase)?;
    } else if cmd.read_seed_phrase {
        let phrase = Password::new()
            .with_prompt("Enter seed phrase")
            .with_confirmation("Re-enter seed phrase", "Seed phrases do not match")
            .interact()
            .context(ReadSeedPhraseFromTerminalSnafu)?;
        import_from_seed_phrase(ctx, &cmd.name, &phrase)?;
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
    ctx: &Context,
    name: &str,
    path: &Path,
    decryption_password_file: Option<&Path>,
    known_key_type: Option<IdentityKeyAlgorithm>,
) -> Result<(), LoadKeyError> {
    // the pem file may be in SEC1 format or PKCS#8 format
    // - if SEC1, the key algorithm can be embedded, separate, or missing
    // - if PKCS#8, the key may or may not be encrypted
    let pem = fs::read_to_string(path)?;
    let sections = pem::parse_many(&pem).context(BadPemFileSnafu { path })?;
    let section = match sections
        .iter()
        .filter(|s| {
            // PKCS#8, unencrypted
            s.tag() == PrivateKeyInfo::PEM_LABEL
                // SEC1, unencrypted
                || s.tag() == EcPrivateKey::PEM_LABEL
                // PKCS#8, encrypted
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
    create_identity(ctx.dirs(), name, key, CreateFormat::Plaintext)?;
    Ok(())
}

fn import_pkcs8(
    section: &Pem,
    path: &Path,
    decryption_password_file: Option<&Path>,
    known_key_type: Option<IdentityKeyAlgorithm>,
) -> Result<IdentityKey, LoadKeyError> {
    // first, grab the actual key structure from the doc, which entails decrypting it if it's encrypted
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
    // second, figure out what algorithm the key is for
    if let Some(known_key_type) = known_key_type {
        // if the user knows what it is, we do not have to check
        match known_key_type {
            IdentityKeyAlgorithm::Secp256k1 => Ok(IdentityKey::Secp256k1(
                SecretKey::from_sec1_der(pki.private_key).context(BadPemKeySnafu { path })?,
            )),
            // todo p256, ed25519
        }
    } else {
        // parse the algorithm information from the metadata
        match pki.algorithm.oid {
            // ECDSA keys are marked as 'generic EC' and the parameters must be further deserialized to get the real algo
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
    path: &Path,
    known_key_type: Option<IdentityKeyAlgorithm>,
) -> Result<IdentityKey, LoadKeyError> {
    let epk = EcPrivateKey::from_der(section.contents()).context(BadPemContentSnafu { path })?;
    // figure out what algorithm the key is for
    if let Some(known_key_type) = known_key_type {
        // if the user knows what it is, we do not have to check
        match known_key_type {
            IdentityKeyAlgorithm::Secp256k1 => Ok(IdentityKey::Secp256k1(
                SecretKey::from_slice(epk.private_key).context(BadPemKeySnafu { path })?,
            )),
            // todo p256
        }
    } else {
        // the algorithm information can be found in two places:
        let params = if let Some(params) = epk.parameters {
            // 1. if it is embedded in the key, everything is great
            params
        } else if let Some(param_section) = param_section {
            // 2. some keys (esp. generated by OpenSSL) have both an "EC PARAMETERS" section and an "EC PRIVATE KEY" section
            //    if this is one such key, the EC PARAMETERS section should have what we're looking for
            EcParameters::from_der(param_section.contents()).context(BadPemContentSnafu { path })?
        } else {
            // 3. and if neither of those exists, even though it's almost certainly a k256 key,
            //    make sure the user is not making a mistake. They can override this with a flag.
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

fn import_from_seed_phrase(ctx: &Context, name: &str, phrase: &str) -> Result<(), DeriveKeyError> {
    let mnemonic = Mnemonic::from_phrase(phrase, Language::English).context(ParseMnemonicSnafu)?;
    let key = derive_default_key_from_seed(&mnemonic);
    create_identity(
        ctx.dirs(),
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
    ReadFileError { source: fs::ReadToStringError },

    #[snafu(display("expected 1 key block in PEM file `{path}`, found {count}"))]
    TooManyKeyBlocks { path: PathBuf, count: usize },

    #[snafu(display("corrupted PEM file `{path}`"))]
    BadPemFile {
        path: PathBuf,
        source: pem::PemError,
    },

    #[snafu(display("malformed key in PEM file `{path}`"))]
    BadPemContent {
        path: PathBuf,
        source: pkcs8::der::Error,
    },

    #[snafu(display(
        "incomplete key in PEM file `{path}`: missing field `{field}` \
        (if you know what kind of key it is, use `--assert-key-type`)"
    ))]
    IncompletePemKey { path: PathBuf, field: String },

    #[snafu(display("malformed key material in PEM file `{path}`"))]
    BadPemKey {
        path: PathBuf,
        source: elliptic_curve::Error,
    },

    #[snafu(display("failed to read password from terminal"))]
    PasswordTermReadError { source: dialoguer::Error },

    #[snafu(display("PEM file `{path}` uses unsupported algorithm {found}, expected {}", expected.iter().format(", ")))]
    UnsupportedAlgorithm {
        path: PathBuf,
        found: ObjectIdentifier,
        expected: Vec<ObjectIdentifier>,
    },

    #[snafu(display("failed to decrypt PEM file `{path}`"))]
    DecryptionFailed { path: PathBuf, source: pkcs8::Error },

    #[snafu(transparent)]
    CreateIdentityError { source: CreateIdentityError },
}

#[derive(Debug, Snafu)]
pub enum DeriveKeyError {
    #[snafu(transparent)]
    ReadSeedFile { source: fs::ReadToStringError },

    #[snafu(display("failed to read seed phrase from terminal"))]
    ReadSeedPhraseFromTerminal { source: dialoguer::Error },

    #[snafu(display("failed to parse seed phrase"))]
    ParseMnemonic { source: bip39::ErrorKind },

    #[snafu(transparent)]
    CreateIdentity { source: CreateIdentityError },
}
