//! Miscellaneous utilities that don't belong to specific commands.

pub async fn fetch_canister_metadata(
    agent: &ic_agent::Agent,
    canister_id: candid::Principal,
    metadata: &str,
) -> Option<String> {
    Some(
        String::from_utf8_lossy(
            &agent
                .read_state_canister_metadata(canister_id, metadata)
                .await
                .ok()?,
        )
        .into(),
    )
}
