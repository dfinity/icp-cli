use candid::Principal;
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

use snafu::ResultExt;

use crate::calls::{
    CanisterCalls, DecodeSnafu, EncodeSnafu, RouteTo, TypedCallError, update_typed,
};

/// A management-canister call, acting on `target` — which is therefore what it
/// is routed to, since the management canister has no routing of its own.
async fn mgmt<A, R>(
    calls: &dyn CanisterCalls,
    method: &str,
    target: Option<Principal>,
    args: A,
    cycles: u128,
) -> Result<R, TypedCallError>
where
    A: candid::utils::ArgumentEncoder,
    R: for<'a> candid::utils::ArgumentDecoder<'a>,
{
    let route = match target {
        Some(t) => RouteTo::Canister(t),
        None => RouteTo::Callee,
    };
    update_typed(
        calls,
        Principal::management_canister(),
        method,
        args,
        route,
        cycles,
    )
    .await
}

pub async fn create_canister(
    calls: &dyn CanisterCalls,
    cycles: u128,
    args: CreateCanisterArgs,
) -> Result<CanisterIdRecord, TypedCallError> {
    let (result,): (CanisterIdRecord,) =
        mgmt(calls, "create_canister", None, (args,), cycles).await?;
    Ok(result)
}

pub async fn canister_status(
    calls: &dyn CanisterCalls,
    args: CanisterIdRecord,
) -> Result<CanisterStatusResult, TypedCallError> {
    let effective = args.canister_id;
    let (result,): (CanisterStatusResult,) =
        mgmt(calls, "canister_status", Some(effective), (args,), 0).await?;
    Ok(result)
}

pub async fn stop_canister(
    calls: &dyn CanisterCalls,
    args: StopCanisterArgs,
) -> Result<(), TypedCallError> {
    let effective = args.canister_id;
    mgmt::<_, ()>(calls, "stop_canister", Some(effective), (args,), 0).await
}

pub async fn start_canister(
    calls: &dyn CanisterCalls,
    args: StartCanisterArgs,
) -> Result<(), TypedCallError> {
    let effective = args.canister_id;
    mgmt::<_, ()>(calls, "start_canister", Some(effective), (args,), 0).await
}

pub async fn delete_canister(
    calls: &dyn CanisterCalls,
    args: DeleteCanisterArgs,
) -> Result<(), TypedCallError> {
    let effective = args.canister_id;
    mgmt::<_, ()>(calls, "delete_canister", Some(effective), (args,), 0).await
}

pub async fn update_settings(
    calls: &dyn CanisterCalls,
    args: UpdateSettingsArgs,
) -> Result<(), TypedCallError> {
    let effective = args.canister_id;
    mgmt::<_, ()>(calls, "update_settings", Some(effective), (args,), 0).await
}

pub async fn install_code(
    calls: &dyn CanisterCalls,
    args: InstallCodeArgs,
) -> Result<(), TypedCallError> {
    let effective = args.canister_id;
    mgmt::<_, ()>(calls, "install_code", Some(effective), (args,), 0).await
}

pub async fn install_chunked_code(
    calls: &dyn CanisterCalls,
    args: InstallChunkedCodeArgs,
) -> Result<(), TypedCallError> {
    let effective = args.target_canister;
    mgmt::<_, ()>(calls, "install_chunked_code", Some(effective), (args,), 0).await
}

pub async fn upload_chunk(
    calls: &dyn CanisterCalls,
    args: UploadChunkArgs,
) -> Result<UploadChunkResult, TypedCallError> {
    let effective = args.canister_id;
    let (result,): (UploadChunkResult,) =
        mgmt(calls, "upload_chunk", Some(effective), (args,), 0).await?;
    Ok(result)
}

pub async fn clear_chunk_store(
    calls: &dyn CanisterCalls,
    args: ClearChunkStoreArgs,
) -> Result<(), TypedCallError> {
    let effective = args.canister_id;
    mgmt::<_, ()>(calls, "clear_chunk_store", Some(effective), (args,), 0).await
}

/// Fetches canister logs from the management canister.
///
/// Asked for as a query, which is what it is. An implementation that has to
/// route the call through something that only accepts updates will do so; that
/// is its business, not this caller's.
pub async fn fetch_canister_logs(
    calls: &dyn CanisterCalls,
    args: FetchCanisterLogsArgs,
) -> Result<FetchCanisterLogsResult, TypedCallError> {
    let target = args.canister_id;
    let arg = candid::encode_args((args,)).context(EncodeSnafu {
        method: "fetch_canister_logs",
    })?;
    let reply = calls
        .query(
            crate::calls::Call::new(Principal::management_canister(), "fetch_canister_logs", arg)
                .with_route(crate::calls::RouteTo::Canister(target)),
        )
        .await?;
    let (result,): (FetchCanisterLogsResult,) =
        candid::decode_args(&reply).context(DecodeSnafu {
            method: "fetch_canister_logs",
        })?;
    Ok(result)
}

pub async fn take_canister_snapshot(
    calls: &dyn CanisterCalls,
    args: TakeCanisterSnapshotArgs,
) -> Result<TakeCanisterSnapshotResult, TypedCallError> {
    let effective = args.canister_id;
    let (result,): (TakeCanisterSnapshotResult,) =
        mgmt(calls, "take_canister_snapshot", Some(effective), (args,), 0).await?;
    Ok(result)
}

pub async fn load_canister_snapshot(
    calls: &dyn CanisterCalls,
    args: LoadCanisterSnapshotArgs,
) -> Result<(), TypedCallError> {
    let effective = args.canister_id;
    mgmt::<_, ()>(calls, "load_canister_snapshot", Some(effective), (args,), 0).await
}

pub async fn list_canister_snapshots(
    calls: &dyn CanisterCalls,
    args: ListCanisterSnapshotsArgs,
) -> Result<ListCanisterSnapshotsResult, TypedCallError> {
    let effective = args.canister_id;
    let (result,): (ListCanisterSnapshotsResult,) = mgmt(
        calls,
        "list_canister_snapshots",
        Some(effective),
        (args,),
        0,
    )
    .await?;
    Ok(result)
}

pub async fn delete_canister_snapshot(
    calls: &dyn CanisterCalls,
    args: DeleteCanisterSnapshotArgs,
) -> Result<(), TypedCallError> {
    let effective = args.canister_id;
    mgmt::<_, ()>(
        calls,
        "delete_canister_snapshot",
        Some(effective),
        (args,),
        0,
    )
    .await
}

pub async fn read_canister_snapshot_metadata(
    calls: &dyn CanisterCalls,
    args: ReadCanisterSnapshotMetadataArgs,
) -> Result<ReadCanisterSnapshotMetadataResult, TypedCallError> {
    let effective = args.canister_id;
    let (result,): (ReadCanisterSnapshotMetadataResult,) = mgmt(
        calls,
        "read_canister_snapshot_metadata",
        Some(effective),
        (args,),
        0,
    )
    .await?;
    Ok(result)
}

pub async fn upload_canister_snapshot_metadata(
    calls: &dyn CanisterCalls,
    args: UploadCanisterSnapshotMetadataArgs,
) -> Result<UploadCanisterSnapshotMetadataResult, TypedCallError> {
    let effective = args.canister_id;
    let (result,): (UploadCanisterSnapshotMetadataResult,) = mgmt(
        calls,
        "upload_canister_snapshot_metadata",
        Some(effective),
        (args,),
        0,
    )
    .await?;
    Ok(result)
}

pub async fn read_canister_snapshot_data(
    calls: &dyn CanisterCalls,
    args: ReadCanisterSnapshotDataArgs,
) -> Result<ReadCanisterSnapshotDataResult, TypedCallError> {
    let effective = args.canister_id;
    let (result,): (ReadCanisterSnapshotDataResult,) = mgmt(
        calls,
        "read_canister_snapshot_data",
        Some(effective),
        (args,),
        0,
    )
    .await?;
    Ok(result)
}

pub async fn upload_canister_snapshot_data(
    calls: &dyn CanisterCalls,
    args: UploadCanisterSnapshotDataArgs,
) -> Result<(), TypedCallError> {
    let effective = args.canister_id;
    mgmt::<_, ()>(
        calls,
        "upload_canister_snapshot_data",
        Some(effective),
        (args,),
        0,
    )
    .await
}
