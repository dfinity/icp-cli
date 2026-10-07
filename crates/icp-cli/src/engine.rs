//! Cloud engines, by name.
//!
//! A cloud engine is a subnet a user rents, and the commands that place a
//! canister take the subnet's id. Users know their engines by name, so this
//! resolves a name to the subnet behind it by asking the engine canister —
//! the registry of engines — which engines the caller may see, and what each
//! is called.
//!
//! What `--engine` was given, and the lookup it stands for.

use std::str::FromStr;

use candid::Principal;
use futures::future::try_join_all;
use icp_canister_interfaces::engine_canister::{
    ENGINE_CANISTER_ID_ENV, Engine, EngineMetadata, GET_ENGINE_METADATA_METHOD,
    LIST_VISIBLE_ENGINES_METHOD, MetadataResult,
};
use icp_project::calls::{CanisterCalls, TypedCallError, query_typed};
use itertools::Itertools;
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use tracing::warn;

/// What `--engine` was given: the engine's subnet outright, or a name to look
/// the subnet up by.
///
/// A principal is taken to be the subnet itself, so `--engine <subnet-id>` is
/// `--subnet <subnet-id>` and needs no registry. Anything else is an engine's
/// name or id, for [`resolve_subnet`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EngineSelector {
    Subnet(Principal),
    Named(String),
}

impl FromStr for EngineSelector {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match Principal::from_text(s) {
            Ok(subnet) => Self::Subnet(subnet),
            Err(_) => Self::Named(s.to_owned()),
        })
    }
}

/// The subnet of the engine the caller calls `engine`, among the engines it
/// may see on `registry`.
///
/// `engine` matches an engine's id, its name, or `name/slug` — the slug being
/// the disambiguator the registry keeps for engines that share a name. An id
/// match wins outright; a name shared by several visible engines is refused
/// rather than guessed at, naming each so the user can pick one. A deleted
/// engine is never matched.
pub(crate) async fn resolve_subnet(
    calls: &dyn CanisterCalls,
    registry: Principal,
    engine: &str,
) -> Result<Principal, ResolveEngineError> {
    let caller = calls.caller();
    let engines = list_visible_engines(calls, registry, caller).await?;

    // The name is a separate query per engine. Deleted engines are left out
    // before asking, not after: nothing can be deployed to them, and there is
    // no point in a round trip to learn what they were called.
    let live: Vec<Engine> = engines
        .into_iter()
        .filter(|e| e.deleted_at.is_none())
        .collect();
    let named: Vec<(&Engine, Option<EngineMetadata>)> =
        try_join_all(live.iter().map(|e| async move {
            get_engine_metadata(calls, registry, &e.id)
                .await
                .map(|metadata| (e, metadata))
        }))
        .await?;

    // An id is unique and settles the question on its own, so it is answered
    // before names are looked at: an engine whose *name* happens to be another
    // engine's id must not make that id ambiguous. An engine the registry
    // would not name is still there by id.
    if let Some((found, metadata)) = named.iter().find(|(e, _)| e.id == engine) {
        return found.subnet_id.context(NoSubnetSnafu {
            engine: metadata
                .as_ref()
                .map_or(found.id.as_str(), |m| m.name.as_str()),
            engine_id: &found.id,
        });
    }
    let matches: Vec<&(&Engine, Option<EngineMetadata>)> = named
        .iter()
        .filter(|(_, m)| {
            m.as_ref()
                .is_some_and(|m| m.name == engine || format!("{}/{}", m.name, m.slug) == engine)
        })
        .collect();

    ensure!(
        matches.len() <= 1,
        AmbiguousSnafu {
            engine,
            matches: matches
                .iter()
                .filter_map(|(e, m)| m.as_ref().map(|m| (e, m)))
                .map(|(e, m)| format!("{} (slug {}, id {})", m.name, m.slug, e.id))
                .collect::<Vec<_>>(),
        }
    );
    let (found, metadata) = matches.first().context(NotVisibleSnafu {
        engine,
        caller,
        visible: named
            .iter()
            .map(|(e, m)| m.as_ref().map_or_else(|| e.id.clone(), |m| m.name.clone()))
            .collect::<Vec<_>>(),
    })?;

    found.subnet_id.context(NoSubnetSnafu {
        engine: metadata
            .as_ref()
            .map_or(found.id.as_str(), |m| m.name.as_str()),
        engine_id: &found.id,
    })
}

async fn list_visible_engines(
    calls: &dyn CanisterCalls,
    registry: Principal,
    caller: Principal,
) -> Result<Vec<Engine>, ResolveEngineError> {
    match query_typed::<_, (Vec<Engine>,)>(calls, registry, LIST_VISIBLE_ENGINES_METHOD, ()).await {
        Ok((engines,)) => Ok(engines),
        // The registry is not deployed on this network at all — a local
        // network, say. That is a different problem from a failed query, and
        // one the user can act on.
        Err(TypedCallError::Call { source }) if source.is_canister_not_found() => {
            RegistryNotFoundSnafu { registry }.fail()
        }
        Err(source) => Err(source).context(ListVisibleEnginesSnafu { registry, caller }),
    }
}

/// An engine's metadata, or `None` when the registry would not give it: the
/// engine then cannot be matched by name, which is worth a warning rather than
/// refusing to resolve every other engine.
async fn get_engine_metadata(
    calls: &dyn CanisterCalls,
    registry: Principal,
    engine_id: &str,
) -> Result<Option<EngineMetadata>, ResolveEngineError> {
    let (result,): (MetadataResult,) = query_typed(
        calls,
        registry,
        GET_ENGINE_METADATA_METHOD,
        (engine_id.to_owned(),),
    )
    .await
    .context(GetEngineMetadataSnafu {
        engine_id,
        registry,
    })?;
    match result {
        MetadataResult::Ok(metadata) => Ok(Some(metadata)),
        MetadataResult::Err(error) => {
            warn!("Engine canister {registry} gave no metadata for engine '{engine_id}': {error}");
            Ok(None)
        }
    }
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum ResolveEngineError {
    #[snafu(display(
        "the engine canister {registry} does not exist on this network. \
         An engine can be named only on a network where the engine canister is deployed; \
         `{ENGINE_CANISTER_ID_ENV}` chooses which one"
    ))]
    RegistryNotFound { registry: Principal },

    #[snafu(display(
        "failed to list the engines visible to {caller} on engine canister {registry}"
    ))]
    ListVisibleEngines {
        source: TypedCallError,
        registry: Principal,
        caller: Principal,
    },

    #[snafu(display(
        "failed to read the metadata of engine '{engine_id}' from engine canister {registry}"
    ))]
    GetEngineMetadata {
        source: TypedCallError,
        engine_id: String,
        registry: Principal,
    },

    #[snafu(display("{}", not_visible_message(engine, *caller, visible)))]
    NotVisible {
        engine: String,
        caller: Principal,
        visible: Vec<String>,
    },

    #[snafu(display(
        "several engines are named '{engine}'; name one by `name/slug` or by id: {}",
        matches.iter().join(", ")
    ))]
    Ambiguous {
        engine: String,
        matches: Vec<String>,
    },

    #[snafu(display("engine '{engine}' ({engine_id}) has no subnet yet"))]
    NoSubnet { engine: String, engine_id: String },
}

fn not_visible_message(engine: &str, caller: Principal, visible: &[String]) -> String {
    let mut message = format!("no engine named '{engine}' is visible to {caller}");
    if visible.is_empty() {
        message.push_str(". This identity sees no engines; check `--identity`");
    } else {
        message.push_str(&format!(
            ". Visible engines: {}",
            visible.iter().sorted().dedup().join(", ")
        ));
    }
    message
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use candid::{Decode, Encode};
    use icp_canister_interfaces::engine_canister::EngineError;
    use icp_project::calls::{Authority, Call, CallError};

    use super::*;

    const REGISTRY: &str = "q6cfj-fyaaa-aaaar-qb77q-cai";
    const SUBNET_A: &str = "uzr34-akd3s-xrdag-3ql62-ocgoh-ld2ao-tamcv-54e7j-krwgb-2gm4z-oqe";
    const SUBNET_B: &str = "tdb26-jop6k-aogll-7ltgs-eruif-6kk7m-qpktf-gdiqx-mxtrf-vb5e6-eqe";

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    fn engine(id: &str, subnet: Option<&str>) -> Engine {
        Engine {
            id: id.to_owned(),
            owner: Principal::anonymous(),
            subnet_id: subnet.map(principal),
            deleted_at: None,
            engine_operator_id: None,
        }
    }

    fn metadata(name: &str, slug: &str) -> MetadataResult {
        MetadataResult::Ok(EngineMetadata {
            name: name.to_owned(),
            slug: slug.to_owned(),
        })
    }

    /// An engine canister with a fixed set of engines, each with the metadata
    /// its id maps to. An engine without an entry gets `NotFound`.
    struct FakeRegistry {
        engines: Vec<Engine>,
        metadata: Vec<(&'static str, MetadataResult)>,
        /// Rejects every call as if the canister did not exist.
        absent: bool,
    }

    #[async_trait]
    impl CanisterCalls for FakeRegistry {
        fn caller(&self) -> Principal {
            principal("2vxsx-fae")
        }

        async fn update(&self, _call: Call) -> Result<Vec<u8>, CallError> {
            unimplemented!()
        }

        async fn query(&self, call: Call) -> Result<Vec<u8>, CallError> {
            assert_eq!(call.canister, principal(REGISTRY));
            if self.absent {
                return Err(CallError::Rejected {
                    canister: call.canister,
                    method: call.method,
                    code: Some("IC0301".to_owned()),
                    message: format!("Canister {REGISTRY} not found"),
                });
            }
            match call.method.as_str() {
                LIST_VISIBLE_ENGINES_METHOD => Ok(Encode!(&self.engines).unwrap()),
                GET_ENGINE_METADATA_METHOD => {
                    let id = Decode!(&call.arg, String).unwrap();
                    let result = self
                        .metadata
                        .iter()
                        .find(|(engine_id, _)| *engine_id == id)
                        .map(|(_, result)| result.clone())
                        .unwrap_or(MetadataResult::Err(EngineError::NotFound));
                    Ok(Encode!(&result).unwrap())
                }
                other => panic!("unexpected query {other}"),
            }
        }

        async fn metadata_section(
            &self,
            _canister: Principal,
            _path: &str,
            _authority: Authority,
        ) -> Result<Option<Vec<u8>>, CallError> {
            unimplemented!()
        }

        async fn controllers(
            &self,
            _canister: Principal,
        ) -> Result<Option<Vec<Principal>>, CallError> {
            unimplemented!()
        }

        async fn module_hash(&self, _canister: Principal) -> Result<Option<Vec<u8>>, CallError> {
            unimplemented!()
        }

        async fn subnet_of(&self, _canister: Principal) -> Result<Principal, CallError> {
            unimplemented!()
        }

        async fn subnet_uses_engine_operator(&self, _subnet: Principal) -> Result<bool, CallError> {
            unimplemented!()
        }
    }

    fn registry() -> FakeRegistry {
        FakeRegistry {
            engines: vec![
                engine("eng-1", Some(SUBNET_A)),
                engine("eng-2", Some(SUBNET_B)),
                engine("eng-3", None),
                Engine {
                    deleted_at: Some(1),
                    ..engine("eng-4", Some(SUBNET_B))
                },
            ],
            metadata: vec![
                ("eng-1", metadata("alpha", "one")),
                ("eng-2", metadata("beta", "two")),
                ("eng-3", metadata("gamma", "three")),
                ("eng-4", metadata("deleted", "four")),
            ],
            absent: false,
        }
    }

    async fn resolve(
        registry: &FakeRegistry,
        engine: &str,
    ) -> Result<Principal, ResolveEngineError> {
        resolve_subnet(registry, principal(REGISTRY), engine).await
    }

    #[test]
    fn selector_takes_a_principal_as_the_subnet() {
        assert_eq!(
            SUBNET_A.parse::<EngineSelector>().unwrap(),
            EngineSelector::Subnet(principal(SUBNET_A))
        );
        assert_eq!(
            "alpha".parse::<EngineSelector>().unwrap(),
            EngineSelector::Named("alpha".to_owned())
        );
    }

    #[tokio::test]
    async fn resolves_an_engine_by_name_id_or_name_and_slug() {
        let registry = registry();
        assert_eq!(
            resolve(&registry, "alpha").await.unwrap(),
            principal(SUBNET_A)
        );
        assert_eq!(
            resolve(&registry, "eng-2").await.unwrap(),
            principal(SUBNET_B)
        );
        assert_eq!(
            resolve(&registry, "beta/two").await.unwrap(),
            principal(SUBNET_B)
        );
    }

    #[tokio::test]
    async fn an_unknown_name_lists_the_visible_engines() {
        let err = resolve(&registry(), "delta").await.unwrap_err();
        assert!(
            matches!(err, ResolveEngineError::NotVisible { .. }),
            "{err}"
        );
        let message = err.to_string();
        assert!(
            message.contains("no engine named 'delta' is visible to 2vxsx-fae"),
            "{message}"
        );
        // Deleted engines are neither on offer nor matched.
        assert!(
            message.contains("Visible engines: alpha, beta, gamma"),
            "{message}"
        );
        let err = resolve(&registry(), "deleted").await.unwrap_err();
        assert!(
            matches!(err, ResolveEngineError::NotVisible { .. }),
            "{err}"
        );
    }

    #[tokio::test]
    async fn an_identity_with_no_engines_is_told_so() {
        let registry = FakeRegistry {
            engines: vec![],
            metadata: vec![],
            absent: false,
        };
        let message = resolve(&registry, "alpha").await.unwrap_err().to_string();
        assert!(
            message.contains("This identity sees no engines"),
            "{message}"
        );
    }

    #[tokio::test]
    async fn an_engine_without_a_subnet_is_refused() {
        let err = resolve(&registry(), "gamma").await.unwrap_err();
        assert!(matches!(err, ResolveEngineError::NoSubnet { .. }), "{err}");
        assert!(
            err.to_string()
                .contains("'gamma' (eng-3) has no subnet yet"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_shared_name_is_refused_and_each_candidate_named() {
        let mut registry = registry();
        registry.engines.push(engine("eng-5", Some(SUBNET_B)));
        registry.metadata.push(("eng-5", metadata("alpha", "five")));

        let err = resolve(&registry, "alpha").await.unwrap_err();
        assert!(matches!(err, ResolveEngineError::Ambiguous { .. }), "{err}");
        let message = err.to_string();
        assert!(message.contains("alpha (slug one, id eng-1)"), "{message}");
        assert!(message.contains("alpha (slug five, id eng-5)"), "{message}");
    }

    #[tokio::test]
    async fn an_id_wins_over_an_engine_named_after_it() {
        let mut registry = registry();
        registry.engines.push(engine("eng-5", Some(SUBNET_B)));
        registry.metadata.push(("eng-5", metadata("eng-1", "five")));

        // `eng-1` is both an id and another engine's name; the id settles it.
        assert_eq!(
            resolve(&registry, "eng-1").await.unwrap(),
            principal(SUBNET_A)
        );
        // The engine called `eng-1` is still reachable by its own id.
        assert_eq!(
            resolve(&registry, "eng-5").await.unwrap(),
            principal(SUBNET_B)
        );
    }

    #[tokio::test]
    async fn an_engine_whose_metadata_is_withheld_is_skipped() {
        let mut registry = registry();
        registry.metadata.retain(|(id, _)| *id != "eng-1");

        // Still reachable by id; not by the name the registry would not give.
        assert_eq!(
            resolve(&registry, "eng-1").await.unwrap(),
            principal(SUBNET_A)
        );
        let err = resolve(&registry, "alpha").await.unwrap_err();
        assert!(
            matches!(err, ResolveEngineError::NotVisible { .. }),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_missing_registry_is_its_own_error() {
        let registry = FakeRegistry {
            engines: vec![],
            metadata: vec![],
            absent: true,
        };
        let err = resolve(&registry, "alpha").await.unwrap_err();
        assert!(
            matches!(err, ResolveEngineError::RegistryNotFound { .. }),
            "{err}"
        );
        assert!(err.to_string().contains("ENGINE_CANISTER_ID"), "{err}");
    }
}
