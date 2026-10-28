//! Whether a canister is serving queries, judged without controller rights.
//!
//! `canister_status` would answer that directly, but only to a controller or a
//! listed status viewer. An identity that is merely authorized by the canister
//! itself — a developer syncing a DAO-owned frontend, say — is neither. A query
//! is open to anyone, and the reject it draws says whether the canister is up.

use std::time::Duration;

use candid::Principal;
use snafu::Snafu;

use crate::calls::{Call, CallError, CanisterCalls};

/// No probe observed the canister serving queries within the wait's budget:
/// every replica that answered saw it stopped, stopping or absent, or none
/// answered at all.
#[derive(Debug, Snafu)]
#[snafu(display("canister {canister_id} did not serve queries within {seconds}s"))]
pub struct NotServingQueriesError {
    pub canister_id: Principal,
    pub seconds: u64,
}

/// A method name no real canister exports — used purely as a liveness probe.
/// Querying it is side-effect-free: the replica rejects an unknown method before
/// any canister code runs (no cycles, no logs, no state change), and the reject
/// reason tells us whether the canister is serving queries yet.
const READINESS_PROBE_METHOD: &str = "<icp-cli readiness probe>";

/// Wait until the canister's *query* path consistently observes it as Running.
///
/// After `start_canister` the canister is Running in the subnet's certified
/// state, but query calls are eventually-consistent reads: each is answered by a
/// single replica that may still lag the restart's commit height and would then
/// see the just-vacated Stopped state. The sync plugin's first calls are queries,
/// so without this wait sync can fail with a transient IC0508 right after a
/// restart.
///
/// We probe with a query for a method no canister exports and classify the result:
///
/// - a reject of "is stopped"/"is stopping" (IC0508/IC0509) means the replica is
///   still lagging behind the restart.
/// - any other reject (e.g. "no query method"), or a reply, means the replica got
///   far enough to answer for a non-status reason, so it sees the canister Running.
/// - a transport or timeout error is inconclusive.
///
/// We require a few consecutive ready observations, spaced out so they may land on
/// different replicas, to raise confidence the lagging set has drained. This is not
/// a hard guarantee — query reads are per-node and boundary nodes load-balance
/// across replicas — but it makes the post-restart race rare.
///
/// The same probe stands in for `canister_status` where the caller may not be
/// allowed to read the status: a canister that stays stopped never passes it.
pub async fn wait_until_serving_queries(
    calls: &dyn CanisterCalls,
    canister_id: Principal,
) -> Result<(), NotServingQueriesError> {
    const REQUIRED_CONSECUTIVE: u32 = 2;
    // Total wall-clock budget for the whole wait — the hard cap on the failure
    // path. PROBE_TIMEOUT below only bounds a single hung probe (so retries keep
    // flowing); this outer budget is what guarantees we give up promptly, rather
    // than attempts * (probe timeout + interval).
    const READINESS_BUDGET: Duration = Duration::from_secs(30);
    const POLL_INTERVAL: Duration = Duration::from_millis(500);
    const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

    let poll = async {
        let mut consecutive_ready: u32 = 0;
        loop {
            // Directly: what this waits for is what a single replica can see,
            // and an intermediary would answer from certified state instead —
            // as an update, which the probe timeout below has no room for.
            let probe =
                calls.query(Call::new(canister_id, READINESS_PROBE_METHOD, Vec::new()).direct());
            let ready = match tokio::time::timeout(PROBE_TIMEOUT, probe).await {
                Ok(Ok(_)) => true,                       // replied -> Running
                Ok(Err(err)) => is_serving_reject(&err), // non-stopped reject -> Running
                Err(_elapsed) => false,                  // probe timed out -> inconclusive
            };

            if ready {
                consecutive_ready += 1;
                if consecutive_ready >= REQUIRED_CONSECUTIVE {
                    return;
                }
            } else {
                consecutive_ready = 0;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    };

    match tokio::time::timeout(READINESS_BUDGET, poll).await {
        Ok(()) => Ok(()),
        Err(_elapsed) => NotServingQueriesSnafu {
            canister_id,
            seconds: READINESS_BUDGET.as_secs(),
        }
        .fail(),
    }
}

/// True when a query error is a *reject from the replica* that indicates the
/// canister is Running and serving — i.e. a positive readiness signal.
///
/// A rejection means the replica processed the request to a verdict (e.g. "no
/// such query method"), so the canister is up — unless the rejection says it is
/// stopped/stopping (IC0508/IC0509, with a message-substring fallback), which is
/// a replica still lagging behind the restart, or that it does not exist
/// (IC0301), which is either a replica lagging behind its creation or a stale
/// id for a canister since deleted. A call that reached no verdict at
/// all is inconclusive — not evidence the canister is serving — and returns
/// false so the caller retries rather than proceeding.
fn is_serving_reject(err: &CallError) -> bool {
    if !err.is_rejection() {
        return false;
    }
    let message = err.message().unwrap_or_default();
    let stopped = matches!(err.code(), Some("IC0508") | Some("IC0509"))
        || message.contains("is stopped")
        || message.contains("is stopping");
    !stopped && !err.is_canister_not_found()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reject(code: Option<&str>, message: &str) -> CallError {
        CallError::Rejected {
            canister: Principal::anonymous(),
            method: READINESS_PROBE_METHOD.to_owned(),
            code: code.map(String::from),
            message: message.to_owned(),
        }
    }

    #[test]
    fn stopped_rejects_are_not_a_readiness_signal() {
        // A replica still lagging behind the restart.
        assert!(!is_serving_reject(&reject(
            Some("IC0508"),
            "Canister abc is stopped"
        )));
        assert!(!is_serving_reject(&reject(
            None,
            "Canister abc is stopping"
        )));
    }

    #[test]
    fn not_found_rejects_are_not_a_readiness_signal() {
        // A replica lagging behind the creation, or a canister since deleted.
        assert!(!is_serving_reject(&reject(
            Some("IC0301"),
            "Canister abc not found"
        )));
        assert!(!is_serving_reject(&reject(None, "Canister abc not found")));
    }

    #[test]
    fn any_other_reject_means_the_canister_is_serving() {
        // The replica got far enough to answer for a non-status reason.
        assert!(is_serving_reject(&reject(
            Some("IC0536"),
            "Canister abc has no query method '<icp-cli readiness probe>'"
        )));
    }

    #[test]
    fn a_call_that_reached_no_verdict_is_inconclusive() {
        // Not evidence of anything; the caller must retry.
        assert!(!is_serving_reject(&CallError::unanswered(
            Principal::anonymous(),
            READINESS_PROBE_METHOD,
            std::io::Error::other("connection reset"),
        )));
    }
}
