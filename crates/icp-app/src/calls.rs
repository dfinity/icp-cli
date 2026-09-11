//! Reaching canisters over the IC's HTTP interface.
//!
//! The implementation of [`icp_project::calls::CanisterCalls`] for a machine
//! with an `ic-agent`: it owns the identity the calls are signed with, the
//! endpoint they go to, and the root key their answers are verified against.
//!
//! It also owns *proxy routing*. `--proxy` names a canister that forwards
//! management calls on the caller's behalf, and it applies to the whole
//! command, so it is a property of the caller rather than of any one call. A
//! query made through a proxy necessarily becomes an update, which is why the
//! trait leaves how a query is answered to the implementation.

use async_trait::async_trait;
use candid::{Encode, Nat, Principal};
use ic_agent::{
    Agent, AgentError,
    agent::{CallResponse, EffectiveId, SubnetType},
};
use icp_canister_interfaces::proxy::{ProxyArgs, ProxyResult};
use icp_project::calls::{Call, CallError, CanisterCalls, RouteTo};

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

    /// Whether `canister` exists, as far as the certified state tree says.
    ///
    /// The `controllers` path is written when a canister is created, so its
    /// presence is what separates a canister with nothing in it from one that
    /// was never created at all.
    ///
    /// A check that could not be made reads as "does not exist": every caller
    /// asks this while deciding whether to report a read failure of its own,
    /// and an existence check that failed too is no reason to suppress it.
    async fn exists(&self, canister: Principal) -> bool {
        self.agent
            .read_state_canister_controllers(canister)
            .await
            .is_ok()
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
        if let Some(proxy) = self.proxy {
            return self.through_proxy(proxy, &call).await;
        }
        if let RouteTo::Subnet(subnet) = call.route {
            return self.to_subnet(subnet, &call).await;
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
        if let Some(proxy) = self.proxy {
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
    ) -> Result<Option<Vec<u8>>, CallError> {
        match self
            .agent
            .read_state_canister_metadata(canister, path)
            .await
        {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) => {
                // A path the certificate will not certify looks the same as a
                // canister that was never created, which is the error this
                // reports rather than "no such section".
                if self.exists(canister).await {
                    Ok(None)
                } else {
                    Err(Self::wrap(canister, "read_state(metadata)", err))
                }
            }
        }
    }

    async fn controllers(&self, canister: Principal) -> Result<Vec<Principal>, CallError> {
        self.agent
            .read_state_canister_controllers(canister)
            .await
            .map_err(|e| Self::wrap(canister, "read_state(controllers)", e))
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
