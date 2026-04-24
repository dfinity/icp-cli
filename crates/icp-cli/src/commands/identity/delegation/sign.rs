use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::Args;
use ic_agent::{Identity as _, export::Principal, identity::Delegation as AgentDelegation};
use icp::{
    context::{Context, GetIdentityError},
    fs::read_to_string,
    identity::delegation::{
        Delegation as WireDelegation, DelegationChain, SignedDelegation as WireSignedDelegation,
    },
    prelude::*,
};
use pem::Pem;
use snafu::{OptionExt, ResultExt, Snafu};

use crate::options::IdentityOpt;

/// Sign a delegation from the selected identity to a target key
#[derive(Debug, Args)]
pub(crate) struct SignArgs {
    /// Public key PEM file of the key to delegate to
    #[arg(long, value_name = "FILE")]
    key_pem: PathBuf,

    /// Delegation validity duration (e.g. "30d", "24h", "3600s", or plain seconds)
    #[arg(long)]
    duration: DurationArg,

    /// Canister principals to restrict the delegation to (comma-separated)
    #[arg(long, value_delimiter = ',')]
    canisters: Option<Vec<Principal>>,

    #[command(flatten)]
    identity: IdentityOpt,
}

pub(crate) async fn exec(ctx: &Context, args: &SignArgs) -> Result<(), SignError> {
    let identity = ctx
        .get_identity(&args.identity.clone().into())
        .await
        .context(GetIdentitySnafu)?;

    let signer_pubkey = identity.public_key().context(AnonymousIdentitySnafu)?;

    let target_pubkey = der_pubkey_from_pem(&args.key_pem)?;

    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos() as u64;
    let expiration = now_nanos.saturating_add(args.duration.as_nanos());

    let delegation = AgentDelegation {
        pubkey: target_pubkey.clone(),
        expiration,
        targets: args.canisters.clone(),
    };

    let sig = identity
        .sign_delegation(&delegation)
        .map_err(|message| SignError::SignDelegation { message })?;

    let signature_bytes = sig.signature.context(AnonymousIdentitySnafu)?;

    // For a DelegatedIdentity (e.g. Internet Identity), sig.delegations holds the existing
    // chain linking the root key to the signing session key. These must be included before
    // the new delegation so the verifier can walk the full chain.
    let mut wire_delegations: Vec<WireSignedDelegation> = sig
        .delegations
        .unwrap_or_default()
        .into_iter()
        .map(|sd| WireSignedDelegation {
            signature: hex::encode(&sd.signature),
            delegation: WireDelegation {
                pubkey: hex::encode(&sd.delegation.pubkey),
                expiration: format!("{:x}", sd.delegation.expiration),
                targets: sd
                    .delegation
                    .targets
                    .as_ref()
                    .map(|ts| ts.iter().map(|p| hex::encode(p.as_slice())).collect()),
            },
        })
        .collect();

    wire_delegations.push(WireSignedDelegation {
        signature: hex::encode(&signature_bytes),
        delegation: WireDelegation {
            pubkey: hex::encode(&target_pubkey),
            expiration: format!("{expiration:x}"),
            targets: args
                .canisters
                .as_ref()
                .map(|ts| ts.iter().map(|p| hex::encode(p.as_slice())).collect()),
        },
    });

    let chain = DelegationChain {
        public_key: hex::encode(&signer_pubkey),
        delegations: wire_delegations,
    };

    let json = serde_json::to_string_pretty(&chain).context(SerializeSnafu)?;
    println!("{json}");

    Ok(())
}

/// Extract the DER-encoded SubjectPublicKeyInfo bytes from a `PUBLIC KEY` PEM file.
fn der_pubkey_from_pem(path: &Path) -> Result<Vec<u8>, SignError> {
    let pem_str = read_to_string(path).context(ReadKeyPemSnafu)?;
    let pem = pem_str.parse::<Pem>().context(ParseKeyPemSnafu { path })?;
    if pem.tag() != "PUBLIC KEY" {
        return UnexpectedPemTagSnafu {
            path,
            found: pem.tag().to_string(),
        }
        .fail();
    }
    Ok(pem.contents().to_vec())
}

/// A duration expressed as a plain number of seconds or with a unit suffix.
///
/// Accepted suffixes: `s` (seconds), `m` (minutes), `h` (hours), `d` (days).
/// A bare integer is interpreted as seconds.
#[derive(Debug, Clone)]
pub(crate) struct DurationArg(u64);

impl DurationArg {
    /// Duration in nanoseconds.
    pub(crate) fn as_nanos(&self) -> u64 {
        self.0.saturating_mul(1_000_000_000)
    }
}

impl FromStr for DurationArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (digits, multiplier) = if let Some(d) = s.strip_suffix('d') {
            (d, 86400u64)
        } else if let Some(h) = s.strip_suffix('h') {
            (h, 3600u64)
        } else if let Some(m) = s.strip_suffix('m') {
            (m, 60u64)
        } else if let Some(s2) = s.strip_suffix('s') {
            (s2, 1u64)
        } else {
            (s, 1u64)
        };

        let n: u64 = digits.parse().map_err(|_| {
            format!("invalid duration `{s}`: expected a number with optional suffix (s/m/h/d)")
        })?;

        Ok(DurationArg(n.saturating_mul(multiplier)))
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum SignError {
    #[snafu(display("failed to load identity"))]
    GetIdentity {
        #[snafu(source(from(GetIdentityError, Box::new)))]
        source: Box<GetIdentityError>,
    },

    #[snafu(display("anonymous identity cannot sign delegations"))]
    AnonymousIdentity,

    #[snafu(display("failed to read key PEM file"))]
    ReadKeyPem { source: icp::fs::IoError },

    #[snafu(display("corrupted PEM file `{path}`"))]
    ParseKeyPem {
        path: PathBuf,
        source: pem::PemError,
    },

    #[snafu(display("expected a PUBLIC KEY PEM in `{path}`, found `{found}`"))]
    UnexpectedPemTag { path: PathBuf, found: String },

    #[snafu(display("failed to sign delegation: {message}"))]
    SignDelegation { message: String },

    #[snafu(display("failed to serialize delegation chain"))]
    Serialize { source: serde_json::Error },
}
