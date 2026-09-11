//! Talking to canisters.
//!
//! Modelled on what the sync-plugin interface already exposes to a guest,
//! because that is the irreducible set: submit a call, and read a certified
//! fact about a canister. Everything else in this crate — creating, installing,
//! settings, candid checks, the ledger reads — is built out of those.
//!
//! Certification is not the caller's business. A reader is *assumed* to return
//! certified answers, so verifying whatever proof that took is entirely inside
//! the implementation. That is also why each certified fact gets its own
//! method rather than a general "read the state tree": a caller running inside
//! a canister cannot read the state tree at all, and reaches the same facts
//! through management-canister calls instead.

use async_trait::async_trait;
use candid::Principal;
use snafu::{ResultExt, Snafu};

/// Which canister a call should be routed to, when that differs from the one
/// being called.
///
/// The management canister has no routing of its own, so a call to it must name
/// the canister it acts on; a subnet-scoped call names a subnet instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteTo {
    /// Route to the canister being called. The ordinary case.
    Callee,
    /// Route to some other canister — the target of a management-canister call.
    Canister(Principal),
    /// Route to a subnet, for a call that acts on the subnet rather than on any
    /// canister in it.
    Subnet(Principal),
}

/// Which of a caller's authorities a request is made under.
///
/// A caller may reach canisters through an *intermediary* that acts on its
/// behalf — `icp-cli`'s `--proxy` canister, say. The intermediary is what the
/// canister sees as its caller, so its permissions are the ones that apply,
/// and a request can ask to skip it and be made by the caller itself instead.
/// A caller with no intermediary has one authority, and both of these mean the
/// same thing for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Authority {
    /// However the caller normally reaches canisters: through its
    /// intermediary, if it has one.
    #[default]
    Mediated,
    /// The caller itself, skipping any intermediary.
    Direct,
}

/// One canister call.
#[derive(Clone, Debug)]
pub struct Call {
    /// Canister whose method is being called.
    pub canister: Principal,

    /// Method name.
    pub method: String,

    /// Candid-encoded arguments.
    pub arg: Vec<u8>,

    /// Where the call should be routed. See [`RouteTo`].
    pub route: RouteTo,

    /// Cycles to attach. Only meaningful for a call an implementation can fund
    /// — one made through a proxy canister, or from a canister of its own.
    pub cycles: u128,

    /// Whose authority to make the call under. See [`Authority`].
    pub authority: Authority,
}

impl Call {
    /// A plain call to `canister`, routed to it, with no cycles attached.
    pub fn new(canister: Principal, method: impl Into<String>, arg: Vec<u8>) -> Self {
        Self {
            canister,
            method: method.into(),
            arg,
            route: RouteTo::Callee,
            cycles: 0,
            authority: Authority::Mediated,
        }
    }

    /// A management-canister call acting on `target`, which is therefore what
    /// it must be routed to.
    pub fn management(method: impl Into<String>, target: Principal, arg: Vec<u8>) -> Self {
        Self {
            canister: Principal::management_canister(),
            method: method.into(),
            arg,
            route: RouteTo::Canister(target),
            cycles: 0,
            authority: Authority::Mediated,
        }
    }

    pub fn with_route(mut self, route: RouteTo) -> Self {
        self.route = route;
        self
    }

    pub fn with_cycles(mut self, cycles: u128) -> Self {
        self.cycles = cycles;
        self
    }

    /// Make the call under the caller's own authority, skipping any
    /// intermediary it otherwise goes through.
    pub fn direct(mut self) -> Self {
        self.authority = Authority::Direct;
        self
    }
}

/// A call did not produce a reply.
///
/// The distinction that matters to callers is whether the network reached a
/// verdict: a rejection is an answer, and its code is worth branching on (a
/// canister reported as stopped, or as not found). Anything else means the
/// caller learned nothing and may want to try again.
#[derive(Debug, Snafu)]
pub enum CallError {
    #[snafu(display("call to '{method}' on {canister} was rejected: {message}"))]
    Rejected {
        canister: Principal,
        method: String,
        /// The replica's error code, e.g. `IC0508`, when it gave one.
        code: Option<String>,
        message: String,
    },

    /// The call never reached a verdict — a transport failure, a timeout, a
    /// malformed reply. The cause is carried whole because what a call travels
    /// over is the implementation's business.
    #[snafu(display("call to '{method}' on {canister} failed"))]
    Failed {
        canister: Principal,
        method: String,
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

impl CallError {
    /// Builds a [`CallError::Failed`] for `call` from an implementation's own
    /// error.
    pub fn failed(
        canister: Principal,
        method: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Failed {
            canister,
            method: method.into(),
            source: Box::new(source),
        }
    }

    /// The replica's error code, when this was a rejection that carried one.
    ///
    /// Callers branch on this rather than on message text: `IC0508` means a
    /// canister was observed stopped, `IC0301` that it does not exist.
    pub fn code(&self) -> Option<&str> {
        match self {
            CallError::Rejected { code, .. } => code.as_deref(),
            CallError::Failed { .. } => None,
        }
    }

    /// Whether the network reached a verdict. A rejection did; a transport
    /// failure did not, and tells the caller nothing about the canister.
    pub fn is_rejection(&self) -> bool {
        matches!(self, CallError::Rejected { .. })
    }

    /// The rejection message, for a caller that has to fall back on matching
    /// text because no error code was given.
    pub fn message(&self) -> Option<&str> {
        match self {
            CallError::Rejected { message, .. } => Some(message),
            CallError::Failed { .. } => None,
        }
    }
}

/// How this crate reaches canisters.
///
/// Every answer is certified. What that takes — verifying a state-tree
/// certificate against a root key, or simply trusting a management-canister
/// reply made from inside the same subnet — belongs to the implementation.
#[async_trait]
pub trait CanisterCalls: Send + Sync {
    /// The principal these calls are made as.
    fn caller(&self) -> Principal;

    /// Submit an update call and return its reply.
    async fn update(&self, call: Call) -> Result<Vec<u8>, CallError>;

    /// Submit a query call and return its reply.
    ///
    /// A caller asks for a query when the method is one; how the answer is
    /// actually obtained is the implementation's business. One that has to
    /// route through something accepting only updates will do that instead,
    /// and the reply is the same either way.
    async fn query(&self, call: Call) -> Result<Vec<u8>, CallError>;

    /// A canister's custom-section metadata.
    ///
    /// `Ok(None)` means the section is certified *absent* from a canister that
    /// exists. A canister that does not exist is an error, since the two are
    /// otherwise indistinguishable and callers use this to tell them apart.
    async fn metadata_section(
        &self,
        canister: Principal,
        path: &str,
    ) -> Result<Option<Vec<u8>>, CallError>;

    /// A canister's controllers, or `None` when there is no such canister.
    ///
    /// Controllers are set when a canister is created, so a caller may read
    /// their absence as the canister's — and an implementation reads it the
    /// same way, to tell a certified absence apart from a canister that was
    /// never there.
    async fn controllers(&self, canister: Principal) -> Result<Option<Vec<Principal>>, CallError>;

    /// The hash of a canister's installed module, or `None` when it has none.
    ///
    /// A canister that does not exist is an error, as in
    /// [`Self::metadata_section`] and for the same reason: nothing installed
    /// and nothing there look alike, and separating them is the
    /// implementation's job rather than the caller's.
    async fn module_hash(&self, canister: Principal) -> Result<Option<Vec<u8>>, CallError>;

    /// Which subnet `canister` lives on.
    async fn subnet_of(&self, canister: Principal) -> Result<Principal, CallError>;

    /// Whether canisters on `subnet` are created through an engine operator
    /// rather than through the management canister.
    ///
    /// Its own question rather than a general topology read, for the same
    /// reason the certified reads above are: a caller inside a canister cannot
    /// consult the registry, and would have to ask a management canister.
    async fn subnet_uses_engine_operator(&self, subnet: Principal) -> Result<bool, CallError>;
}

/// A typed call failed, or its arguments or reply would not encode.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum TypedCallError {
    #[snafu(display("failed to encode the arguments for '{method}'"))]
    Encode {
        source: candid::Error,
        method: String,
    },

    #[snafu(transparent)]
    Call { source: CallError },

    #[snafu(display("failed to decode the reply from '{method}'"))]
    Decode {
        source: candid::Error,
        method: String,
    },
}

/// Make an update call with Candid arguments and decode its reply.
pub async fn update_typed<A, R>(
    calls: &dyn CanisterCalls,
    canister: Principal,
    method: &str,
    args: A,
    route: RouteTo,
    cycles: u128,
) -> Result<R, TypedCallError>
where
    A: candid::utils::ArgumentEncoder,
    R: for<'a> candid::utils::ArgumentDecoder<'a>,
{
    let arg = candid::encode_args(args).context(EncodeSnafu { method })?;
    let reply = calls
        .update(
            Call::new(canister, method, arg)
                .with_route(route)
                .with_cycles(cycles),
        )
        .await?;
    candid::decode_args(&reply).context(DecodeSnafu { method })
}

/// Make a query call with Candid arguments and decode its reply.
pub async fn query_typed<A, R>(
    calls: &dyn CanisterCalls,
    canister: Principal,
    method: &str,
    args: A,
) -> Result<R, TypedCallError>
where
    A: candid::utils::ArgumentEncoder,
    R: for<'a> candid::utils::ArgumentDecoder<'a>,
{
    let arg = candid::encode_args(args).context(EncodeSnafu { method })?;
    let reply = calls.query(Call::new(canister, method, arg)).await?;
    candid::decode_args(&reply).context(DecodeSnafu { method })
}
