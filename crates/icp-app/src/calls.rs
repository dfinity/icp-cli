//! Reaching canisters over the IC's HTTP interface.
//!
//! The implementation of [`icp_project::calls::CanisterCalls`] for a machine
//! with an `ic-agent`: it owns the identity the calls are signed with, the
//! endpoint they go to, and the root key their answers are verified against.
//!
//! It also owns *proxy routing*. `--proxy` names a canister that forwards
//! management calls on the caller's behalf, and it applies to the whole
//! command, so it is a property of the caller rather than of any one call —
//! the proxy is the [`Authority::Mediated`] one, and a request that asks for
//! [`Authority::Direct`] skips it. A query made through a proxy necessarily
//! becomes an update, which is why the trait leaves how a query is answered to
//! the implementation.

use async_trait::async_trait;
use candid::{Encode, Nat, Principal};
use ic_agent::{
    Agent, AgentError,
    agent::{CallResponse, EffectiveId, SubnetType},
    hash_tree::{Label, LookupResult},
};
use ic_management_canister_types::{CanisterMetadataArgs, CanisterMetadataResult};
use icp_canister_interfaces::proxy::{ProxyArgs, ProxyResult};
use icp_project::calls::{Authority, Call, CallError, CanisterCalls, RouteTo};

/// [`CanisterCalls`] over an `ic-agent`, optionally forwarding through a proxy
/// canister.
pub struct AgentCalls {
    agent: Agent,
    proxy: Option<Principal>,
    caller: Principal,
}

impl AgentCalls {
    /// Builds a caller from `agent`. `proxy`, when given, is the canister every
    /// call is forwarded through.
    ///
    /// The caller's principal is read once here: it cannot change for the life
    /// of the agent, and every call would otherwise ask for it again.
    pub fn new(agent: Agent, proxy: Option<Principal>) -> Result<Self, AgentError> {
        let caller = agent
            .get_principal()
            .map_err(|message| AgentError::MessageError(message.clone()))?;
        Ok(Self {
            agent,
            proxy,
            caller,
        })
    }

    /// The proxy `call` should go through, if any: none when no proxy was
    /// configured, and none when the call asked to be made directly.
    fn mediator(&self, call: &Call) -> Option<Principal> {
        match call.authority {
            Authority::Mediated => self.proxy,
            Authority::Direct => None,
        }
    }

    /// Turns an agent error into the shape the trait speaks in: a rejection is
    /// a verdict and keeps its code, anything else reached none.
    fn wrap(canister: Principal, method: &str, err: AgentError) -> CallError {
        match &err {
            AgentError::CertifiedReject { reject, .. }
            | AgentError::UncertifiedReject { reject, .. } => CallError::Rejected {
                canister,
                method: method.to_owned(),
                code: reject.error_code.clone(),
                message: reject.reject_message.clone(),
            },
            _ => CallError::failed(canister, method, err),
        }
    }

    /// Forwards `call` through the proxy canister, unwrapping its reply.
    async fn through_proxy(&self, proxy: Principal, call: &Call) -> Result<Vec<u8>, CallError> {
        let args = ProxyArgs {
            canister_id: call.canister,
            method: call.method.clone(),
            args: call.arg.clone(),
            cycles: Nat::from(call.cycles),
        };
        let arg = Encode!(&args).map_err(|e| CallError::failed(proxy, "proxy", e))?;
        let reply = self
            .agent
            .update(&proxy, "proxy")
            .with_arg(arg)
            .await
            .map_err(|e| Self::wrap(proxy, "proxy", e))?;
        let (result,): (ProxyResult,) =
            candid::decode_args(&reply).map_err(|e| CallError::failed(proxy, "proxy", e))?;
        match result {
            ProxyResult::Ok(ok) => Ok(ok.result),
            // The proxy reports the inner call's failure as text; it reached a
            // verdict, so it is a rejection even though the code is lost.
            ProxyResult::Err(err) => Err(CallError::Rejected {
                canister: call.canister,
                method: call.method.clone(),
                code: None,
                message: err.format_error(),
            }),
        }
    }

    /// Whether `canister` exists, which its controllers answer — see
    /// [`CanisterCalls::controllers`].
    ///
    /// A check that could not be made reads as "does not exist": every caller
    /// asks this while deciding whether to report a read failure of its own,
    /// and an existence check that failed too is no reason to suppress it.
    async fn exists(&self, canister: Principal) -> bool {
        self.controllers(canister)
            .await
            .is_ok_and(|controllers| controllers.is_some())
    }

    /// Ask the target's subnet to certify a metadata section, reporting only
    /// what the certificate proves.
    ///
    /// The section path is requested together with `controllers`, because a
    /// metadata path proven absent is equally what a canister that was never
    /// created looks like — `controllers` is written at creation, so its
    /// presence is what separates the two. A canister with no module installed
    /// has no sections at all, which the certificate reports as an absent path
    /// under a canister that exists, and so as `Ok(None)`.
    async fn certified_metadata_section(
        &self,
        canister: Principal,
        path: &str,
    ) -> Result<Option<Vec<u8>>, CallError> {
        let metadata_path: Vec<Label<Vec<u8>>> = vec![
            "canister".into(),
            Label::from_bytes(canister.as_slice()),
            "metadata".into(),
            path.into(),
        ];
        let controllers_path: Vec<Label<Vec<u8>>> = vec![
            "canister".into(),
            Label::from_bytes(canister.as_slice()),
            "controllers".into(),
        ];
        let method = "read_state(metadata)";
        let cert = self
            .agent
            .read_state_raw(
                vec![metadata_path.clone(), controllers_path.clone()],
                canister,
            )
            .await
            .map_err(|err| Self::wrap(canister, method, err))?;

        let unproven = |about: String| {
            Err(CallError::Rejected {
                canister,
                method: method.to_owned(),
                code: None,
                message: format!("the certificate proves nothing about {about}"),
            })
        };
        match cert.tree.lookup_path(&metadata_path) {
            LookupResult::Found(bytes) => Ok(Some(bytes.to_vec())),
            LookupResult::Absent => match cert.tree.lookup_path(&controllers_path) {
                LookupResult::Found(_) => Ok(None),
                LookupResult::Absent => Err(CallError::Rejected {
                    canister,
                    method: method.to_owned(),
                    code: None,
                    message: format!("canister {canister} does not exist"),
                }),
                _ => unproven(format!("canister {canister}")),
            },
            // Not proof of absence, just a certificate that says nothing about
            // the path — reporting the section missing off this would be a
            // guess, and a private section is exactly what it looks like.
            _ => unproven(format!("section `{path}` of canister {canister}")),
        }
    }

    /// Read a metadata section by having the proxy ask the management canister
    /// for it, which is what reaches a section private to the proxy's control.
    ///
    /// `read_state` is not a canister method, so it cannot be forwarded; the
    /// management canister's `canister_metadata` can. It does not distinguish
    /// an absent section from one the caller may not have, so a reply claiming
    /// absence is confirmed against a certificate before it is reported as
    /// one.
    async fn metadata_through_proxy(
        &self,
        proxy: Principal,
        canister: Principal,
        path: &str,
    ) -> Result<Option<Vec<u8>>, CallError> {
        let arg = Encode!(&CanisterMetadataArgs {
            canister_id: canister,
            name: path.to_owned(),
        })
        .map_err(|e| CallError::failed(canister, "canister_metadata", e))?;
        let call = Call::management("canister_metadata", canister, arg);

        match self.through_proxy(proxy, &call).await {
            Ok(reply) => {
                let (metadata,): (CanisterMetadataResult,) = candid::decode_args(&reply)
                    .map_err(|e| CallError::failed(canister, "canister_metadata", e))?;
                Ok(Some(metadata.value))
            }
            Err(err) => {
                let claims_absent = err
                    .message()
                    .is_some_and(|message| rejected_as_no_such_section(message, canister, path));
                if !claims_absent {
                    return Err(err);
                }
                // The management canister says the same thing about a section
                // that isn't there and one that is private to someone else, so
                // its word alone cannot be reported as absence. Only a
                // certificate proves the section absent.
                match self.certified_metadata_section(canister, path).await? {
                    None => Ok(None),
                    Some(_) => Err(CallError::Rejected {
                        canister,
                        method: "canister_metadata".to_owned(),
                        code: None,
                        message: format!(
                            "canister {canister} does not let {proxy} read section `{path}`"
                        ),
                    }),
                }
            }
        }
    }

    /// A subnet-scoped update: routed to a subnet rather than to any canister
    /// on it, which the agent only exposes through a signed submission.
    async fn to_subnet(&self, subnet: Principal, call: &Call) -> Result<Vec<u8>, CallError> {
        let effective = EffectiveId::Subnet(subnet);
        let fail = |e: AgentError| Self::wrap(call.canister, &call.method, e);

        let signed = self
            .agent
            .update(&call.canister, &call.method)
            .with_arg(call.arg.clone())
            .sign()
            .map_err(fail)?;
        let response = self
            .agent
            .update_signed(effective, signed.signed_update)
            .await
            .map_err(fail)?;
        match response {
            CallResponse::Response(bytes) => Ok(bytes),
            CallResponse::Poll(request_id) => {
                let status = self
                    .agent
                    .sign_request_status(effective, request_id)
                    .map_err(fail)?;
                Ok(self
                    .agent
                    .wait_signed(&request_id, effective, status.signed_request_status)
                    .await
                    .map_err(fail)?
                    .0)
            }
        }
    }
}

/// Whether the management canister rejected a metadata read by claiming the
/// target has no such section, rather than because the read itself failed.
///
/// The claim is not proof: the same rejection covers a section private to
/// someone other than the proxy, so the caller confirms it against a
/// certificate. A proxied read comes back as reject text with no code
/// attached, so recognizing the claim at all means matching the replica's
/// wording. Both sentences name the canister and one names the section, so the
/// match is anchored on the values this call supplied rather than on a loose
/// phrase that text relayed from elsewhere might happen to contain. A reword
/// upstream turns the claim into an error rather than into a wrong answer.
fn rejected_as_no_such_section(message: &str, canister: Principal, path: &str) -> bool {
    // A canister with no module installed has no sections at all, so it reports
    // absence in its own words. The certificate says the same thing about it:
    // the metadata path is absent while the canister itself is there.
    message.contains(&format!(
        "The canister {canister} has no Wasm module and hence no metadata is available."
    )) || message.contains(&format!(
        "The canister {canister} has no metadata section with the name {path}."
    ))
}

/// Wraps a resolved agent as the caller this workspace's operations take,
/// forwarding through `proxy` when one was asked for.
pub fn calls(
    agent: Agent,
    proxy: Option<Principal>,
) -> Result<std::sync::Arc<dyn CanisterCalls>, AgentError> {
    Ok(std::sync::Arc::new(AgentCalls::new(agent, proxy)?))
}

#[async_trait]
impl CanisterCalls for AgentCalls {
    fn caller(&self) -> Principal {
        self.caller
    }

    async fn update(&self, call: Call) -> Result<Vec<u8>, CallError> {
        // A subnet-scoped call names the subnet it acts on, and a proxy could
        // only act on the subnet it lives on itself — so this comes first,
        // rather than dropping the one thing the call is about. The commands
        // that can name a subnet refuse to name a proxy as well.
        if let RouteTo::Subnet(subnet) = call.route {
            return self.to_subnet(subnet, &call).await;
        }
        if let Some(proxy) = self.mediator(&call) {
            return self.through_proxy(proxy, &call).await;
        }
        let mut builder = self
            .agent
            .update(&call.canister, &call.method)
            .with_arg(call.arg.clone());
        if let RouteTo::Canister(effective) = call.route {
            builder = builder.with_effective_canister_id(effective);
        }
        builder
            .await
            .map_err(|e| Self::wrap(call.canister, &call.method, e))
    }

    async fn query(&self, call: Call) -> Result<Vec<u8>, CallError> {
        // A proxy only accepts updates, so a query through one becomes an
        // update. The reply is the same either way.
        if let Some(proxy) = self.mediator(&call) {
            return self.through_proxy(proxy, &call).await;
        }
        let mut builder = self
            .agent
            .query(&call.canister, &call.method)
            .with_arg(call.arg.clone());
        if let RouteTo::Canister(effective) = call.route {
            builder = builder.with_effective_canister_id(effective);
        }
        builder
            .call()
            .await
            .map_err(|e| Self::wrap(call.canister, &call.method, e))
    }

    async fn metadata_section(
        &self,
        canister: Principal,
        path: &str,
        authority: Authority,
    ) -> Result<Option<Vec<u8>>, CallError> {
        match self.proxy {
            Some(proxy) if authority == Authority::Mediated => {
                self.metadata_through_proxy(proxy, canister, path).await
            }
            _ => self.certified_metadata_section(canister, path).await,
        }
    }

    async fn controllers(&self, canister: Principal) -> Result<Option<Vec<Principal>>, CallError> {
        match self.agent.read_state_canister_controllers(canister).await {
            Ok(controllers) => Ok(Some(controllers)),
            Err(AgentError::LookupPathAbsent(_)) => Ok(None),
            Err(err) => Err(Self::wrap(canister, "read_state(controllers)", err)),
        }
    }

    async fn module_hash(&self, canister: Principal) -> Result<Option<Vec<u8>>, CallError> {
        let err = match self.agent.read_state_canister_module_hash(canister).await {
            Ok(hash) => return Ok(Some(hash)),
            Err(err) => err,
        };
        // No module-hash path means nothing is installed — or that there is no
        // canister to install into, which is not an answer this can give.
        if matches!(err, AgentError::LookupPathAbsent(_)) && self.exists(canister).await {
            return Ok(None);
        }
        Err(Self::wrap(canister, "read_state(module_hash)", err))
    }

    async fn subnet_of(&self, canister: Principal) -> Result<Principal, CallError> {
        Ok(self
            .agent
            .get_subnet_by_canister(&canister)
            .await
            .map_err(|e| Self::wrap(canister, "subnet_of", e))?
            .id())
    }

    async fn subnet_uses_engine_operator(&self, subnet: Principal) -> Result<bool, CallError> {
        let info = self
            .agent
            .get_subnet_by_id(&subnet)
            .await
            .map_err(|e| Self::wrap(subnet, "subnet_type", e))?;
        Ok(matches!(info.subnet_type(), Some(SubnetType::CloudEngine)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The replica's own wording for the two ways a target reports it has no
    /// section, copied from `CanisterManagerError` in the IC repo. Both are
    /// absence, not failure, so both must reach the plugin as `none`.
    #[test]
    fn management_canister_absence_rejects_are_recognized() {
        let target = Principal::from_text("aaaaa-aa").unwrap();
        let other = Principal::from_text("2vxsx-fae").unwrap();

        let no_module = format!(
            "Proxy call failed: The canister {target} has no Wasm module and hence no metadata is available."
        );
        let no_section = format!(
            "Proxy call failed: The canister {target} has no metadata section with the name candid:service."
        );
        assert!(rejected_as_no_such_section(
            &no_module,
            target,
            "candid:service"
        ));
        assert!(rejected_as_no_such_section(
            &no_section,
            target,
            "candid:service"
        ));

        // A section by another name, a canister other than the one asked about,
        // and an unrelated failure are all reads that failed.
        assert!(!rejected_as_no_such_section(&no_section, target, "dfx"));
        assert!(!rejected_as_no_such_section(
            &no_module,
            other,
            "candid:service"
        ));
        assert!(!rejected_as_no_such_section(
            &format!("Proxy call failed: Canister {target} not found."),
            target,
            "candid:service"
        ));
    }
}
