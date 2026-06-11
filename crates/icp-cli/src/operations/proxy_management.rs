use candid::Principal;
use ic_agent::Agent;
use ic_management_canister_types::{
    CanisterIdRecord, CanisterStatusResult, ClearChunkStoreArgs, CreateCanisterArgs,
    DeleteCanisterArgs, DeleteCanisterSnapshotArgs, FetchCanisterLogsArgs, FetchCanisterLogsResult,
    InstallChunkedCodeArgs, InstallCodeArgs, ListCanisterSnapshotsArgs,
    ListCanisterSnapshotsResult, LoadCanisterSnapshotArgs, ReadCanisterSnapshotDataArgs,
    ReadCanisterSnapshotDataResult, ReadCanisterSnapshotMetadataArgs,
    ReadCanisterSnapshotMetadataResult, StartCanisterArgs, StopCanisterArgs,
    TakeCanisterSnapshotArgs, TakeCanisterSnapshotResult, UpdateSettingsArgs,
    UploadCanisterSnapshotDataArgs, UploadCanisterSnapshotMetadataArgs,
    UploadCanisterSnapshotMetadataResult, UploadChunkArgs, UploadChunkResult,
};

use snafu::{ResultExt, Snafu};

use super::proxy::{UpdateOrProxyError, update_or_proxy};

pub async fn create_canister(
    agent: &Agent,
    proxy: Option<Principal>,
    cycles: u128,
    args: CreateCanisterArgs,
) -> Result<CanisterIdRecord, UpdateOrProxyError> {
    let (result,): (CanisterIdRecord,) = update_or_proxy(
        agent,
        Principal::management_canister(),
        "create_canister",
        (args,),
        proxy,
        None,
        cycles,
    )
    .await?;
    Ok(result)
}

pub async fn canister_status(
    agent: &Agent,
    proxy: Option<Principal>,
    args: CanisterIdRecord,
) -> Result<CanisterStatusResult, UpdateOrProxyError> {
    let effective = args.canister_id;
    let (result,): (CanisterStatusResult,) = update_or_proxy(
        agent,
        Principal::management_canister(),
        "canister_status",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await?;
    Ok(result)
}

pub async fn stop_canister(
    agent: &Agent,
    proxy: Option<Principal>,
    args: StopCanisterArgs,
) -> Result<(), UpdateOrProxyError> {
    let effective = args.canister_id;
    update_or_proxy::<_, ()>(
        agent,
        Principal::management_canister(),
        "stop_canister",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await
}

pub async fn start_canister(
    agent: &Agent,
    proxy: Option<Principal>,
    args: StartCanisterArgs,
) -> Result<(), UpdateOrProxyError> {
    let effective = args.canister_id;
    update_or_proxy::<_, ()>(
        agent,
        Principal::management_canister(),
        "start_canister",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await
}

pub async fn delete_canister(
    agent: &Agent,
    proxy: Option<Principal>,
    args: DeleteCanisterArgs,
) -> Result<(), UpdateOrProxyError> {
    let effective = args.canister_id;
    update_or_proxy::<_, ()>(
        agent,
        Principal::management_canister(),
        "delete_canister",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await
}

pub async fn update_settings(
    agent: &Agent,
    proxy: Option<Principal>,
    args: UpdateSettingsArgs,
) -> Result<(), UpdateOrProxyError> {
    let effective = args.canister_id;
    update_or_proxy::<_, ()>(
        agent,
        Principal::management_canister(),
        "update_settings",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await
}

pub async fn install_code(
    agent: &Agent,
    proxy: Option<Principal>,
    args: InstallCodeArgs,
) -> Result<(), UpdateOrProxyError> {
    let effective = args.canister_id;
    update_or_proxy::<_, ()>(
        agent,
        Principal::management_canister(),
        "install_code",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await
}

pub async fn install_chunked_code(
    agent: &Agent,
    proxy: Option<Principal>,
    args: InstallChunkedCodeArgs,
) -> Result<(), UpdateOrProxyError> {
    let effective = args.target_canister;
    update_or_proxy::<_, ()>(
        agent,
        Principal::management_canister(),
        "install_chunked_code",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await
}

pub async fn upload_chunk(
    agent: &Agent,
    proxy: Option<Principal>,
    args: UploadChunkArgs,
) -> Result<UploadChunkResult, UpdateOrProxyError> {
    let effective = args.canister_id;
    let (result,): (UploadChunkResult,) = update_or_proxy(
        agent,
        Principal::management_canister(),
        "upload_chunk",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await?;
    Ok(result)
}

pub async fn clear_chunk_store(
    agent: &Agent,
    proxy: Option<Principal>,
    args: ClearChunkStoreArgs,
) -> Result<(), UpdateOrProxyError> {
    let effective = args.canister_id;
    update_or_proxy::<_, ()>(
        agent,
        Principal::management_canister(),
        "clear_chunk_store",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await
}

#[derive(Debug, Snafu)]
pub enum FetchCanisterLogsError {
    #[snafu(display("failed to encode call arguments: {source}"))]
    CandidEncode { source: candid::Error },

    #[snafu(display("failed to decode call response: {source}"))]
    CandidDecode { source: candid::Error },

    #[snafu(display("direct query call failed: {source}"))]
    DirectQueryCall { source: ic_agent::AgentError },

    #[snafu(display("proxied call failed: {source}"))]
    ProxiedCall { source: UpdateOrProxyError },
}

/// Fetches canister logs from the management canister.
///
/// Unlike other management canister methods, `fetch_canister_logs` is a
/// **query** call when made directly. When a proxy is provided, the call is
/// routed through the proxy canister as an update call instead.
pub async fn fetch_canister_logs(
    agent: &Agent,
    proxy: Option<Principal>,
    args: FetchCanisterLogsArgs,
) -> Result<FetchCanisterLogsResult, FetchCanisterLogsError> {
    let effective = args.canister_id;
    if proxy.is_some() {
        let (result,): (FetchCanisterLogsResult,) = update_or_proxy(
            agent,
            Principal::management_canister(),
            "fetch_canister_logs",
            (args,),
            proxy,
            Some(effective),
            0,
        )
        .await
        .context(ProxiedCallSnafu)?;
        Ok(result)
    } else {
        let arg = candid::encode_args((args,)).context(CandidEncodeSnafu)?;
        let res = agent
            .query(&Principal::management_canister(), "fetch_canister_logs")
            .with_arg(arg)
            .with_effective_canister_id(effective)
            .await
            .context(DirectQueryCallSnafu)?;
        let (result,): (FetchCanisterLogsResult,) =
            candid::decode_args(&res).context(CandidDecodeSnafu)?;
        Ok(result)
    }
}

pub async fn take_canister_snapshot(
    agent: &Agent,
    proxy: Option<Principal>,
    args: TakeCanisterSnapshotArgs,
) -> Result<TakeCanisterSnapshotResult, UpdateOrProxyError> {
    let effective = args.canister_id;
    let (result,): (TakeCanisterSnapshotResult,) = update_or_proxy(
        agent,
        Principal::management_canister(),
        "take_canister_snapshot",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await?;
    Ok(result)
}

pub async fn load_canister_snapshot(
    agent: &Agent,
    proxy: Option<Principal>,
    args: LoadCanisterSnapshotArgs,
) -> Result<(), UpdateOrProxyError> {
    let effective = args.canister_id;
    update_or_proxy::<_, ()>(
        agent,
        Principal::management_canister(),
        "load_canister_snapshot",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await
}

pub async fn list_canister_snapshots(
    agent: &Agent,
    proxy: Option<Principal>,
    args: ListCanisterSnapshotsArgs,
) -> Result<ListCanisterSnapshotsResult, UpdateOrProxyError> {
    let effective = args.canister_id;
    let (result,): (ListCanisterSnapshotsResult,) = update_or_proxy(
        agent,
        Principal::management_canister(),
        "list_canister_snapshots",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await?;
    Ok(result)
}

pub async fn delete_canister_snapshot(
    agent: &Agent,
    proxy: Option<Principal>,
    args: DeleteCanisterSnapshotArgs,
) -> Result<(), UpdateOrProxyError> {
    let effective = args.canister_id;
    update_or_proxy::<_, ()>(
        agent,
        Principal::management_canister(),
        "delete_canister_snapshot",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await
}

pub async fn read_canister_snapshot_metadata(
    agent: &Agent,
    proxy: Option<Principal>,
    args: ReadCanisterSnapshotMetadataArgs,
) -> Result<ReadCanisterSnapshotMetadataResult, UpdateOrProxyError> {
    let effective = args.canister_id;
    let (result,): (ReadCanisterSnapshotMetadataResult,) = update_or_proxy(
        agent,
        Principal::management_canister(),
        "read_canister_snapshot_metadata",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await?;
    Ok(result)
}

pub async fn upload_canister_snapshot_metadata(
    agent: &Agent,
    proxy: Option<Principal>,
    args: UploadCanisterSnapshotMetadataArgs,
) -> Result<UploadCanisterSnapshotMetadataResult, UpdateOrProxyError> {
    let effective = args.canister_id;
    let (result,): (UploadCanisterSnapshotMetadataResult,) = update_or_proxy(
        agent,
        Principal::management_canister(),
        "upload_canister_snapshot_metadata",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await?;
    Ok(result)
}

pub async fn read_canister_snapshot_data(
    agent: &Agent,
    proxy: Option<Principal>,
    args: ReadCanisterSnapshotDataArgs,
) -> Result<ReadCanisterSnapshotDataResult, UpdateOrProxyError> {
    let effective = args.canister_id;
    let (result,): (ReadCanisterSnapshotDataResult,) = update_or_proxy(
        agent,
        Principal::management_canister(),
        "read_canister_snapshot_data",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await?;
    Ok(result)
}

pub async fn upload_canister_snapshot_data(
    agent: &Agent,
    proxy: Option<Principal>,
    args: UploadCanisterSnapshotDataArgs,
) -> Result<(), UpdateOrProxyError> {
    let effective = args.canister_id;
    update_or_proxy::<_, ()>(
        agent,
        Principal::management_canister(),
        "upload_canister_snapshot_data",
        (args,),
        proxy,
        Some(effective),
        0,
    )
    .await
}
