//! The `icp-signed-message` file format, version 1.
//!
//! One canister call, composed and signed on a machine that holds the key and
//! has no network, carried to a machine that has network and no key, and
//! submitted there. `icp canister call --sign-only` writes these files and `icp
//! message send` reads them.
//!
//! The file is JSON so a human courier can look at what they are carrying, and
//! its four objects mirror the trust tiers:
//!
//! 1. **Authenticated** — everything inside `request.envelope`. The courier
//!    cannot alter any of it without invalidating the signature.
//! 2. **Acted upon, not authenticated** — `network` and `destination`. The
//!    sending machine has to trust these to know where to send; they cannot
//!    change *what* executes.
//! 3. **Display only** — `candid` and `summary`. Never used for a decision;
//!    everything shown to the operator is re-derived from the envelope.

use crate::network::RootKeySpec;
use crate::prelude::*;
use base64::engine::general_purpose::STANDARD as BASE64;
use candid::Principal;
use ic_agent::agent::{
    EffectiveId, EnvelopeContent, signed_query_inspect, signed_request_status_inspect,
    signed_update_inspect,
};
use ic_agent::{AgentError, RequestId};
use serde::{Deserialize, Serialize};
use snafu::prelude::*;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;

/// The `format` field every version 1 file carries.
pub const FORMAT: &str = "icp-signed-message";

/// The only format version this build reads or writes. An unknown version is
/// refused outright rather than parsed optimistically; fields a reader may
/// safely ignore are added without bumping it.
pub const VERSION: u32 = 1;

/// How wide the submission window always is.
///
/// The IC accepts an ingress message only while its `ingress_expiry` is in the
/// future *and* no more than `MAX_INGRESS_TTL` ahead of replica time, so the
/// submittable window is always `[ingress_expiry - 5min, ingress_expiry]`. The
/// width is not a parameter; only its placement is, which is what `--valid-from`
/// sets.
pub const SUBMISSION_WINDOW: Duration = Duration::minutes(5);

/// The management canister methods the IC routes by effective *subnet* id rather
/// than by canister id, and so the only ones a `subnet` destination may name.
const SUBNET_SCOPED_UPDATE_METHODS: [&str; 2] =
    ["create_canister", "provisional_create_canister_with_cycles"];
const SUBNET_SCOPED_QUERY_METHODS: [&str; 1] = ["list_canisters"];

/// A canister call signed for submission from another machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedMessage {
    /// Identifies the file. Always [`FORMAT`].
    pub format: String,

    /// A hard parse gate; see [`VERSION`].
    pub version: u32,

    pub request: Request,
    pub network: Network,
    pub destination: Destination,

    /// The canister's interface, as `.did` source text — not a path, which would
    /// point at a file the submitting machine does not have. Display only: it
    /// renders the argument in the confirmation summary and decodes the reply.
    /// Optional, because the submitting machine is online and can fall back to
    /// fetching `candid:service` itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candid: Option<String>,

    pub summary: Summary,
}

/// The signed request, and — for an update — what is needed to await its result
/// without a key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    #[serde(rename = "type")]
    pub call_type: CallType,

    /// The CBOR authentication envelope. The only authenticated content here.
    #[serde(with = "base64_bytes")]
    pub envelope: Vec<u8>,

    /// Hex-encoded request id, recomputed from the envelope on the way in.
    /// Update only — a query answers immediately, so it identifies nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,

    /// A pre-signed `read_state` for `request_status/<request_id>`, so the
    /// submitting machine can poll for the outcome with no key of its own.
    /// Update only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "base64_bytes::option"
    )]
    pub status_check: Option<Vec<u8>>,
}

/// Which submission API the signed envelope targets — *not* what kind of method
/// is being called.
///
/// A method is an update method, a query method, or a composite query, but there
/// are only two ways to invoke one: `/call` for replicated execution and
/// `/query` for non-replicated. The mapping is not one-to-one — an update method
/// can only go through `/call`, a composite query only through `/query`, and a
/// query method through either — so which one was signed for has to be recorded
/// here rather than re-derived from the interface.
///
/// [`CallType::Update`] is the envelope's `call` request type, spelled "update"
/// to match `sync-plugin.wit`'s `enum call-type { update, query }`, which draws
/// the same distinction for the same reason. Checked against the envelope's own
/// discriminant on the way in; selects `update_signed` over `query_signed` on
/// the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallType {
    Update,
    Query,
}

impl CallType {
    pub fn as_str(self) -> &'static str {
        match self {
            CallType::Update => "update",
            CallType::Query => "query",
        }
    }
}

/// Where to submit. The envelope carries no URL, and the reply's certificate has
/// to be verified against a root key the submitting machine may not have
/// configured.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Network {
    pub url: Url,
    pub root_key: RootKeySpec,
}

/// Which endpoint shape routes the request, mirroring
/// [`ic_agent::agent::EffectiveId`].
///
/// Deliberately tagged rather than a bare principal: canister-scoped and
/// subnet-scoped requests go to different endpoints, and the discriminant cannot
/// be re-derived — it usually equals the canister id, but for management-canister
/// calls it comes from the argument, and for a few it is a subnet id.
///
/// There is deliberately no `From<Principal>`: `ic-agent` has one, and it yields
/// `EffectiveId::Canister`, so a bare principal handed to `update_signed` routes
/// to the canister endpoint without complaining. Construct the variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Destination {
    Canister(Principal),
    Subnet(Principal),
}

impl Destination {
    pub fn to_effective_id(self) -> EffectiveId {
        match self {
            Destination::Canister(p) => EffectiveId::Canister(p),
            Destination::Subnet(p) => EffectiveId::Subnet(p),
        }
    }
}

/// A human-readable echo of the envelope, for eyeballing the raw file. Never
/// acted upon — the submitting machine re-derives all of it and refuses the file
/// if the two disagree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub sender: Principal,
    pub canister_id: Principal,
    pub method: String,

    #[serde(with = "base64_bytes")]
    pub arg: Vec<u8>,

    pub signed_at: String,

    /// Both ends of the window are recorded so the file answers "when can I send
    /// this?" without arithmetic.
    pub valid_from: String,
    pub valid_until: String,
}

/// Where now sits relative to the message's five-minute submission window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    /// The window has not opened yet — a state the signer asked for, not a puzzle.
    NotYetValid,
    Valid,
    Expired,
}

/// Everything a submitter needs, all of it re-derived from the signed envelope
/// rather than read out of the file's metadata.
#[derive(Debug, Clone)]
#[allow(
    dead_code,
    reason = "read by the submitting side, which lands separately"
)]
pub struct Validated {
    pub call_type: CallType,
    pub sender: Principal,
    pub canister_id: Principal,
    pub method: String,
    pub arg: Vec<u8>,

    /// Update only.
    pub request_id: Option<RequestId>,

    pub valid_from: OffsetDateTime,
    pub valid_until: OffsetDateTime,
    pub window: WindowState,
}

impl SignedMessage {
    /// Writes the message to `path`.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        crate::fs::json::save(path, self).context(SaveSnafu { path })
    }

    /// Renders the message exactly as [`SignedMessage::save`] would write it, for
    /// callers writing somewhere other than a file.
    pub fn to_json(&self) -> Result<String, Error> {
        serde_json::to_string_pretty(self).context(SerializeSnafu)
    }

    /// Reads a message from `path`. The result is unvalidated — call
    /// [`SignedMessage::validate`] before acting on any of it.
    pub fn load(path: &Path) -> Result<Self, Error> {
        crate::fs::json::load(path).context(LoadSnafu { path })
    }

    /// Checks the file against its envelope and reports where `now` falls in the
    /// submission window.
    ///
    /// Everything the caller goes on to display or act upon comes back in
    /// [`Validated`], decoded from the envelope; a metadata field that disagrees
    /// with the envelope means a malformed or tampered file and is an error, not
    /// a silent preference. The window state is *reported*, not enforced, so a
    /// caller can still show an expired message.
    pub fn validate(&self, now: OffsetDateTime) -> Result<Validated, Error> {
        ensure!(
            self.format == FORMAT,
            UnknownFormatSnafu {
                format: self.format.clone()
            }
        );
        ensure!(
            self.version == VERSION,
            UnsupportedVersionSnafu {
                version: self.version
            }
        );

        let content: EnvelopeContent = decode_envelope(&self.request.envelope)?;

        let (sender, canister_id, method, arg, ingress_expiry) = match &content {
            EnvelopeContent::Call {
                sender,
                canister_id,
                method_name,
                arg,
                ingress_expiry,
                ..
            } => {
                ensure!(
                    self.request.call_type == CallType::Update,
                    CallTypeMismatchSnafu {
                        declared: self.request.call_type,
                        envelope: "update",
                    }
                );
                (
                    *sender,
                    *canister_id,
                    method_name.clone(),
                    arg.clone(),
                    *ingress_expiry,
                )
            }
            EnvelopeContent::Query {
                sender,
                canister_id,
                method_name,
                arg,
                ingress_expiry,
                ..
            } => {
                ensure!(
                    self.request.call_type == CallType::Query,
                    CallTypeMismatchSnafu {
                        declared: self.request.call_type,
                        envelope: "query",
                    }
                );
                (
                    *sender,
                    *canister_id,
                    method_name.clone(),
                    arg.clone(),
                    *ingress_expiry,
                )
            }
            EnvelopeContent::ReadState { .. } => {
                return CallTypeMismatchSnafu {
                    declared: self.request.call_type,
                    envelope: "read_state",
                }
                .fail();
            }
        };

        // The summary is what a human reads out of the file, so it has to be the
        // envelope's own story. `ic-agent`'s inspectors compare exactly the
        // fields the envelope carries.
        let inspect = match self.request.call_type {
            CallType::Update => signed_update_inspect(
                self.summary.sender,
                self.summary.canister_id,
                &self.summary.method,
                &self.summary.arg,
                ingress_expiry,
                self.request.envelope.clone(),
            ),
            CallType::Query => signed_query_inspect(
                self.summary.sender,
                self.summary.canister_id,
                &self.summary.method,
                &self.summary.arg,
                ingress_expiry,
                self.request.envelope.clone(),
            ),
        };
        inspect.context(SummaryMismatchSnafu)?;

        let valid_until = timestamp_from_nanos(ingress_expiry)?;
        let valid_from = valid_until - SUBMISSION_WINDOW;
        ensure!(
            self.summary.valid_from == format_timestamp(valid_from)
                && self.summary.valid_until == format_timestamp(valid_until),
            WindowMismatchSnafu {
                recorded_from: self.summary.valid_from.clone(),
                recorded_until: self.summary.valid_until.clone(),
                envelope_from: format_timestamp(valid_from),
                envelope_until: format_timestamp(valid_until),
            }
        );

        let request_id = match self.request.call_type {
            CallType::Update => {
                let computed = content.to_request_id();
                let recorded = self
                    .request
                    .request_id
                    .as_deref()
                    .context(MissingRequestIdSnafu)?;
                ensure!(
                    recorded == computed.to_string(),
                    RequestIdMismatchSnafu {
                        recorded: recorded.to_owned(),
                        computed: computed.to_string(),
                    }
                );

                let status_check = self
                    .request
                    .status_check
                    .as_ref()
                    .context(MissingStatusCheckSnafu)?;
                // Sharing the call's expiry is the point: a status check with a
                // later expiry would itself be premature, and one with an earlier
                // expiry would die before the call it is waiting on.
                signed_request_status_inspect(
                    sender,
                    &computed,
                    ingress_expiry,
                    status_check.clone(),
                )
                .context(StatusCheckMismatchSnafu)?;

                Some(computed)
            }
            CallType::Query => {
                ensure!(
                    self.request.request_id.is_none() && self.request.status_check.is_none(),
                    QueryCarriesUpdateFieldsSnafu
                );
                None
            }
        };

        self.check_destination(&method)?;

        let window = if now < valid_from {
            WindowState::NotYetValid
        } else if now > valid_until {
            WindowState::Expired
        } else {
            WindowState::Valid
        };

        Ok(Validated {
            call_type: self.request.call_type,
            sender,
            canister_id,
            method,
            arg,
            request_id,
            valid_from,
            valid_until,
            window,
        })
    }

    /// A `subnet` destination is legal only for the management-canister methods
    /// the interface spec routes that way; anything else is malformed.
    fn check_destination(&self, method: &str) -> Result<(), Error> {
        let Destination::Subnet(subnet) = self.destination else {
            return Ok(());
        };
        let permitted = match self.request.call_type {
            CallType::Update => SUBNET_SCOPED_UPDATE_METHODS.contains(&method),
            CallType::Query => SUBNET_SCOPED_QUERY_METHODS.contains(&method),
        };
        ensure!(
            permitted && self.summary.canister_id == Principal::management_canister(),
            IllegalSubnetDestinationSnafu {
                subnet,
                method: method.to_owned(),
            }
        );
        Ok(())
    }
}

/// CBOR-decodes an authentication envelope down to the content that was signed.
fn decode_envelope(envelope: &[u8]) -> Result<EnvelopeContent, Error> {
    let envelope: ic_agent::agent::Envelope =
        serde_cbor::from_slice(envelope).context(MalformedEnvelopeSnafu)?;
    Ok(envelope.content.into_owned())
}

fn timestamp_from_nanos(nanos: u64) -> Result<OffsetDateTime, Error> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(nanos))
        .ok()
        .context(UnrepresentableExpirySnafu { nanos })
}

/// Formats an instant the way the file records it: RFC 3339 in UTC.
///
/// Every timestamp written into a file goes through here, which is what lets
/// [`SignedMessage::validate`] check the recorded window against the envelope by
/// comparing rendered strings — no reparsing, and no precision lost either way.
pub fn format_timestamp(t: OffsetDateTime) -> String {
    t.format(&Rfc3339)
        .expect("an OffsetDateTime is always representable as RFC 3339")
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("failed to write the signed message to {path}"))]
    Save {
        source: crate::fs::json::Error,
        path: PathBuf,
    },

    #[snafu(display("failed to serialize the signed message"))]
    Serialize { source: serde_json::Error },

    #[snafu(display("failed to read the signed message at {path}"))]
    Load {
        source: crate::fs::json::Error,
        path: PathBuf,
    },

    #[snafu(display("not an {FORMAT} file: its format is '{format}'"))]
    UnknownFormat { format: String },

    #[snafu(display(
        "signed message format version {version} is not supported; this build reads version {VERSION}"
    ))]
    UnsupportedVersion { version: u32 },

    #[snafu(display("the signed request could not be decoded"))]
    MalformedEnvelope { source: serde_cbor::Error },

    #[snafu(display(
        "the file declares a {} request but its envelope is a {envelope} request",
        declared.as_str()
    ))]
    CallTypeMismatch {
        declared: CallType,
        envelope: &'static str,
    },

    #[snafu(display("the summary does not match the signed request"))]
    SummaryMismatch {
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },

    #[snafu(display(
        "the summary records a submission window of {recorded_from} to {recorded_until}, \
         but the signed request expires over {envelope_from} to {envelope_until}"
    ))]
    WindowMismatch {
        recorded_from: String,
        recorded_until: String,
        envelope_from: String,
        envelope_until: String,
    },

    #[snafu(display("an update message must carry a request_id"))]
    MissingRequestId,

    #[snafu(display(
        "the recorded request_id {recorded} is not the one the signed request hashes to ({computed})"
    ))]
    RequestIdMismatch { recorded: String, computed: String },

    #[snafu(display("an update message must carry a status_check"))]
    MissingStatusCheck,

    #[snafu(display("the status_check does not read the status of this request"))]
    StatusCheckMismatch {
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },

    #[snafu(display(
        "a query message answers immediately and must carry neither request_id nor status_check"
    ))]
    QueryCarriesUpdateFields,

    #[snafu(display(
        "'{method}' cannot be routed to subnet {subnet}: only management canister calls the \
         interface spec scopes to a subnet may name a subnet destination"
    ))]
    IllegalSubnetDestination { subnet: Principal, method: String },

    #[snafu(display("the signed request expires at {nanos}ns, which is not a representable time"))]
    UnrepresentableExpiry { nanos: u64 },
}

/// Byte fields are base64 rather than hex: an argument can be large — an
/// air-gapped `install_code` carries a whole Wasm module — where hex doubles the
/// file and base64 adds a third.
mod base64_bytes {
    use super::BASE64;
    use base64::Engine as _;
    use serde::{Deserialize as _, Deserializer, Serializer, de::Error as _};

    pub(super) fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&BASE64.encode(bytes))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(d)?;
        BASE64.decode(&encoded).map_err(D::Error::custom)
    }

    pub(super) mod option {
        use super::*;

        pub(in super::super) fn serialize<S: Serializer>(
            bytes: &Option<Vec<u8>>,
            s: S,
        ) -> Result<S::Ok, S::Error> {
            match bytes {
                Some(bytes) => super::serialize(bytes, s),
                None => s.serialize_none(),
            }
        }

        pub(in super::super) fn deserialize<'de, D: Deserializer<'de>>(
            d: D,
        ) -> Result<Option<Vec<u8>>, D::Error> {
            let encoded = Option::<String>::deserialize(d)?;
            encoded
                .map(|encoded| BASE64.decode(&encoded).map_err(D::Error::custom))
                .transpose()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use camino_tempfile::tempdir;
    use ic_agent::{Agent, identity::AnonymousIdentity};

    const CANISTER: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
    const SUBNET: &str = "fuqsr-in2lc-zbcjj-ydmcw-pzq7h-4xm2z-pto4i-dcyee-5z4rz-x63ji-nae";

    fn canister() -> Principal {
        Principal::from_text(CANISTER).expect("a valid principal")
    }

    /// An agent built the way `--sign-only` builds one: no root key, and an
    /// ingress expiry pinned so the status check it derives lands on
    /// `valid_until`.
    fn signing_agent(valid_until: OffsetDateTime) -> Agent {
        let remaining = (valid_until - OffsetDateTime::now_utc())
            .try_into()
            .expect("the test window must be in the future");
        Agent::builder()
            .with_url("http://localhost:1")
            .with_identity(AnonymousIdentity)
            .with_ingress_expiry(remaining)
            .build()
            .expect("building an agent makes no request")
    }

    /// A minute-aligned window opening `offset` from now, as the signing path
    /// produces it.
    fn window(offset: Duration) -> (OffsetDateTime, OffsetDateTime) {
        let until = (OffsetDateTime::now_utc() + offset + SUBMISSION_WINDOW)
            .replace_nanosecond(0)
            .and_then(|t| t.replace_second(0))
            .expect("0 is a valid second and nanosecond");
        (until - SUBMISSION_WINDOW, until)
    }

    fn update_message_for(
        canister_id: Principal,
        method: &str,
        offset: Duration,
    ) -> (SignedMessage, OffsetDateTime, OffsetDateTime) {
        let (valid_from, valid_until) = window(offset);
        let agent = signing_agent(valid_until);
        let signed = agent
            .update(&canister_id, method)
            .with_arg(b"arg".to_vec())
            .expire_at(valid_until)
            .sign()
            .expect("signing makes no request");
        let status_check = agent
            .sign_request_status(EffectiveId::Canister(canister_id), signed.request_id)
            .expect("signing makes no request");
        assert_eq!(
            status_check.ingress_expiry, signed.ingress_expiry,
            "a minute-aligned expiry must survive ic-agent's own truncation"
        );

        let message = SignedMessage {
            format: FORMAT.to_string(),
            version: VERSION,
            request: Request {
                call_type: CallType::Update,
                envelope: signed.signed_update,
                request_id: Some(signed.request_id.to_string()),
                status_check: Some(status_check.signed_request_status),
            },
            network: Network {
                url: "https://icp-api.io".parse().expect("a valid url"),
                root_key: RootKeySpec::Mainnet,
            },
            destination: Destination::Canister(canister_id),
            candid: Some(r#"service : { "greet" : (text) -> (text) }"#.to_string()),
            summary: Summary {
                sender: signed.sender,
                canister_id,
                method: method.to_string(),
                arg: b"arg".to_vec(),
                signed_at: format_timestamp(OffsetDateTime::now_utc()),
                valid_from: format_timestamp(valid_from),
                valid_until: format_timestamp(valid_until),
            },
        };
        (message, valid_from, valid_until)
    }

    fn update_message() -> (SignedMessage, OffsetDateTime, OffsetDateTime) {
        update_message_for(canister(), "greet", Duration::ZERO)
    }

    fn query_message() -> SignedMessage {
        let (valid_from, valid_until) = window(Duration::ZERO);
        let agent = signing_agent(valid_until);
        let signed = agent
            .query(&canister(), "greet")
            .with_arg(b"arg".to_vec())
            .expire_at(valid_until)
            .sign()
            .expect("signing makes no request");

        SignedMessage {
            format: FORMAT.to_string(),
            version: VERSION,
            request: Request {
                call_type: CallType::Query,
                envelope: signed.signed_query,
                request_id: None,
                status_check: None,
            },
            network: Network {
                url: "https://icp-api.io".parse().expect("a valid url"),
                root_key: RootKeySpec::Mainnet,
            },
            destination: Destination::Canister(canister()),
            candid: None,
            summary: Summary {
                sender: signed.sender,
                canister_id: canister(),
                method: "greet".to_string(),
                arg: b"arg".to_vec(),
                signed_at: format_timestamp(OffsetDateTime::now_utc()),
                valid_from: format_timestamp(valid_from),
                valid_until: format_timestamp(valid_until),
            },
        }
    }

    #[test]
    fn round_trips_through_a_file() {
        let (message, _, _) = update_message();
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("message.json");
        message.save(&path).expect("save");

        let loaded = SignedMessage::load(&path).expect("load");
        let validated = loaded
            .validate(OffsetDateTime::now_utc())
            .expect("a message we just signed must validate");

        assert_eq!(validated.canister_id, canister());
        assert_eq!(validated.method, "greet");
        assert_eq!(validated.arg, b"arg");
        assert_eq!(validated.call_type, CallType::Update);
        assert_eq!(validated.window, WindowState::Valid);
        assert_eq!(
            validated
                .request_id
                .expect("an update carries one")
                .to_string(),
            message.request.request_id.expect("an update carries one"),
        );
    }

    #[test]
    fn writes_the_documented_json_shape() {
        let (message, _, valid_until) = update_message();
        let json: serde_json::Value =
            serde_json::from_str(&message.to_json().expect("serialize")).expect("valid json");

        assert_eq!(json["format"], FORMAT);
        assert_eq!(json["version"], VERSION);
        assert_eq!(json["request"]["type"], "update");
        // Tagged, so a submitter can tell a canister route from a subnet one.
        assert_eq!(json["destination"]["canister"], CANISTER);
        assert_eq!(json["network"]["root_key"], "mainnet");
        assert_eq!(
            json["summary"]["valid_until"],
            format_timestamp(valid_until)
        );
        // Base64, not hex: an argument can be a whole Wasm module.
        assert_eq!(
            BASE64
                .decode(json["summary"]["arg"].as_str().expect("a string"))
                .expect("base64"),
            b"arg",
        );
    }

    #[test]
    fn query_message_omits_the_update_only_fields() {
        let message = query_message();
        let json: serde_json::Value =
            serde_json::from_str(&message.to_json().expect("serialize")).expect("valid json");
        assert_eq!(json["request"]["type"], "query");
        assert!(json["request"].get("request_id").is_none());
        assert!(json["request"].get("status_check").is_none());

        let validated = message
            .validate(OffsetDateTime::now_utc())
            .expect("validate");
        assert_eq!(validated.call_type, CallType::Query);
        assert!(validated.request_id.is_none());
    }

    #[test]
    fn query_carrying_update_fields_is_rejected() {
        let (update, _, _) = update_message();
        let mut message = query_message();
        message.request.status_check = update.request.status_check;

        assert!(matches!(
            message.validate(OffsetDateTime::now_utc()),
            Err(Error::QueryCarriesUpdateFields),
        ));
    }

    #[test]
    fn declared_type_must_match_the_envelope() {
        let mut message = query_message();
        message.request.call_type = CallType::Update;

        assert!(matches!(
            message.validate(OffsetDateTime::now_utc()),
            Err(Error::CallTypeMismatch { .. }),
        ));
    }

    #[test]
    fn tampered_summary_is_rejected() {
        for tamper in [
            (|m: &mut SignedMessage| m.summary.method = "transfer".to_string()) as fn(&mut _),
            |m: &mut SignedMessage| m.summary.arg = b"other".to_vec(),
            |m: &mut SignedMessage| m.summary.canister_id = Principal::management_canister(),
            |m: &mut SignedMessage| m.summary.sender = Principal::management_canister(),
        ] {
            let (mut message, _, _) = update_message();
            tamper(&mut message);
            assert!(
                matches!(
                    message.validate(OffsetDateTime::now_utc()),
                    Err(Error::SummaryMismatch { .. }),
                ),
                "a summary that disagrees with the envelope is an error, not a preference",
            );
        }
    }

    #[test]
    fn tampered_window_is_rejected() {
        let (mut message, _, valid_until) = update_message();
        message.summary.valid_until = format_timestamp(valid_until + Duration::hours(1));

        assert!(matches!(
            message.validate(OffsetDateTime::now_utc()),
            Err(Error::WindowMismatch { .. }),
        ));
    }

    #[test]
    fn request_id_must_be_the_envelope_hash() {
        let (mut message, _, _) = update_message();
        message.request.request_id = Some("00".repeat(32));

        assert!(matches!(
            message.validate(OffsetDateTime::now_utc()),
            Err(Error::RequestIdMismatch { .. }),
        ));
    }

    #[test]
    fn status_check_must_read_this_requests_status() {
        let (mut message, _, valid_until) = update_message();
        // A status check signed for some *other* call: correctly formed, same
        // sender, same window — but it would poll the wrong request.
        let agent = signing_agent(valid_until);
        let other = agent
            .update(&canister(), "greet")
            .with_arg(b"different".to_vec())
            .expire_at(valid_until)
            .sign()
            .expect("signing makes no request");
        let elsewhere = agent
            .sign_request_status(EffectiveId::Canister(canister()), other.request_id)
            .expect("signing makes no request");
        message.request.status_check = Some(elsewhere.signed_request_status);

        assert!(matches!(
            message.validate(OffsetDateTime::now_utc()),
            Err(Error::StatusCheckMismatch { .. }),
        ));
    }

    #[test]
    fn missing_update_fields_are_rejected() {
        let (mut message, _, _) = update_message();
        let status_check = message.request.status_check.take();
        assert!(matches!(
            message.validate(OffsetDateTime::now_utc()),
            Err(Error::MissingStatusCheck),
        ));

        message.request.status_check = status_check;
        message.request.request_id = None;
        assert!(matches!(
            message.validate(OffsetDateTime::now_utc()),
            Err(Error::MissingRequestId),
        ));
    }

    #[test]
    fn reports_each_window_state() {
        let (message, valid_from, valid_until) = update_message_for(
            canister(),
            "greet",
            // Placed in the future so every state can be asked about.
            Duration::hours(1),
        );

        let state = |now| message.validate(now).expect("validate").window;
        assert_eq!(
            state(valid_from - Duration::seconds(1)),
            WindowState::NotYetValid
        );
        assert_eq!(state(valid_from), WindowState::Valid);
        assert_eq!(state(valid_until), WindowState::Valid);
        assert_eq!(
            state(valid_until + Duration::seconds(1)),
            WindowState::Expired
        );
    }

    #[test]
    fn the_window_is_always_five_minutes_wide() {
        let (message, valid_from, valid_until) = update_message();
        assert_eq!(valid_until - valid_from, SUBMISSION_WINDOW);

        let validated = message
            .validate(OffsetDateTime::now_utc())
            .expect("validate");
        assert_eq!(validated.valid_from, valid_from);
        assert_eq!(validated.valid_until, valid_until);
    }

    #[test]
    fn subnet_destination_is_legal_only_for_subnet_scoped_methods() {
        let subnet = Principal::from_text(SUBNET).expect("a valid principal");

        let (mut creating, _, _) = update_message_for(
            Principal::management_canister(),
            "create_canister",
            Duration::ZERO,
        );
        creating.destination = Destination::Subnet(subnet);
        creating
            .validate(OffsetDateTime::now_utc())
            .expect("canister creation is routed by subnet");

        let (mut greeting, _, _) = update_message();
        greeting.destination = Destination::Subnet(subnet);
        assert!(matches!(
            greeting.validate(OffsetDateTime::now_utc()),
            Err(Error::IllegalSubnetDestination { .. }),
        ));

        // Right method, but on an ordinary canister rather than the management one.
        let (mut impostor, _, _) =
            update_message_for(canister(), "create_canister", Duration::ZERO);
        impostor.destination = Destination::Subnet(subnet);
        assert!(matches!(
            impostor.validate(OffsetDateTime::now_utc()),
            Err(Error::IllegalSubnetDestination { .. }),
        ));
    }

    #[test]
    fn unknown_format_and_version_are_refused_outright() {
        let (mut message, _, _) = update_message();
        message.version = VERSION + 1;
        assert!(matches!(
            message.validate(OffsetDateTime::now_utc()),
            Err(Error::UnsupportedVersion { .. }),
        ));

        message.version = VERSION;
        message.format = "quill".to_string();
        assert!(matches!(
            message.validate(OffsetDateTime::now_utc()),
            Err(Error::UnknownFormat { .. }),
        ));
    }

    #[test]
    fn malformed_envelope_is_refused() {
        let (mut message, _, _) = update_message();
        message.request.envelope = b"not cbor".to_vec();

        assert!(matches!(
            message.validate(OffsetDateTime::now_utc()),
            Err(Error::MalformedEnvelope { .. }),
        ));
    }
}
