use byte_unit::{Byte, UnitType};
use clap::Args;
use icp::context::Context;
use icp::prelude::*;

use super::SnapshotId;
use crate::commands::args;
use crate::operations::misc::format_timestamp;
use crate::operations::snapshot_transfer::{
    BlobType, SnapshotPaths, SnapshotTransferError, create_transfer_progress_bar,
    delete_download_progress, download_blob_to_file, download_wasm_chunk, load_download_progress,
    load_metadata, read_snapshot_metadata, save_metadata,
};

/// Download a snapshot to local disk
#[derive(Debug, Args)]
pub(crate) struct DownloadArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// The snapshot ID to download (hex-encoded)
    snapshot_id: SnapshotId,

    /// Output directory for the snapshot files
    #[arg(long, short = 'o')]
    output: PathBuf,

    /// Resume a previously interrupted download
    #[arg(long)]
    resume: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &DownloadArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();

    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;
    let cid = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;

    let name = &args.cmd_args.canister;
    let snapshot_id = &args.snapshot_id.0;

    // Open or create the snapshot directory with a lock
    let snapshot_dir = SnapshotPaths::new(args.output.clone())?;

    snapshot_dir
        .with_write(async |paths| {
            // Ensure directories exist
            paths.ensure_dirs()?;

            // Check if we should resume or start fresh
            let metadata = if args.resume && paths.metadata_path().exists() {
                ctx.term.write_line("Resuming previous download...")?;
                load_metadata(paths)?
            } else if !args.resume {
                // Check if directory has existing files (besides lock)
                let has_files = paths.metadata_path().exists()
                    || paths.wasm_module_path().exists()
                    || paths.wasm_memory_path().exists()
                    || paths.stable_memory_path().exists();

                if has_files {
                    return Err(SnapshotTransferError::DirectoryNotEmpty {
                        path: args.output.clone(),
                    }
                    .into());
                }

                // Fetch metadata from canister
                ctx.term.write_line(&format!(
                    "Downloading snapshot {id} from canister {name} ({cid})",
                    id = hex::encode(snapshot_id),
                ))?;

                let metadata = read_snapshot_metadata(&agent, cid, snapshot_id).await?;

                ctx.term.write_line(&format!(
                    "  Timestamp: {}",
                    format_timestamp(metadata.taken_at_timestamp)
                ))?;

                let total_size = metadata.wasm_module_size
                    + metadata.wasm_memory_size
                    + metadata.stable_memory_size;
                ctx.term.write_line(&format!(
                    "  Total size: {}",
                    Byte::from_u64(total_size).get_appropriate_unit(UnitType::Binary)
                ))?;

                // Save metadata
                save_metadata(&metadata, paths)?;

                metadata
            } else {
                return Err(SnapshotTransferError::NoExistingDownload {
                    path: args.output.clone(),
                }
                .into());
            };

            // Load download progress (handles gaps from previous interrupted downloads)
            let mut progress = load_download_progress(paths)?;

            // Download WASM module
            if metadata.wasm_module_size > 0 {
                if !progress.wasm_module.is_complete(metadata.wasm_module_size) {
                    let pb = create_transfer_progress_bar(metadata.wasm_module_size, "WASM module");
                    download_blob_to_file(
                        &agent,
                        cid,
                        snapshot_id,
                        BlobType::WasmModule,
                        metadata.wasm_module_size,
                        paths,
                        &mut progress,
                        &pb,
                    )
                    .await?;
                    pb.finish_with_message("done");
                } else {
                    ctx.term.write_line("WASM module: already complete")?;
                }
            }

            // Download WASM memory
            if metadata.wasm_memory_size > 0 {
                if !progress.wasm_memory.is_complete(metadata.wasm_memory_size) {
                    let pb = create_transfer_progress_bar(metadata.wasm_memory_size, "WASM memory");
                    download_blob_to_file(
                        &agent,
                        cid,
                        snapshot_id,
                        BlobType::WasmMemory,
                        metadata.wasm_memory_size,
                        paths,
                        &mut progress,
                        &pb,
                    )
                    .await?;
                    pb.finish_with_message("done");
                } else {
                    ctx.term.write_line("WASM memory: already complete")?;
                }
            }

            // Download stable memory
            if metadata.stable_memory_size > 0 {
                if !progress
                    .stable_memory
                    .is_complete(metadata.stable_memory_size)
                {
                    let pb =
                        create_transfer_progress_bar(metadata.stable_memory_size, "Stable memory");
                    download_blob_to_file(
                        &agent,
                        cid,
                        snapshot_id,
                        BlobType::StableMemory,
                        metadata.stable_memory_size,
                        paths,
                        &mut progress,
                        &pb,
                    )
                    .await?;
                    pb.finish_with_message("done");
                } else {
                    ctx.term.write_line("Stable memory: already complete")?;
                }
            } else {
                // Create empty stable memory file
                icp::fs::write(&paths.stable_memory_path(), &[])?;
            }

            // Download WASM chunk store
            if !metadata.wasm_chunk_store.is_empty() {
                ctx.term.write_line(&format!(
                    "Downloading {} WASM chunks...",
                    metadata.wasm_chunk_store.len()
                ))?;

                for chunk_hash in &metadata.wasm_chunk_store {
                    let chunk_path = paths.wasm_chunk_path(&chunk_hash.hash);
                    if !chunk_path.exists() {
                        download_wasm_chunk(&agent, cid, snapshot_id, chunk_hash, paths).await?;
                    }
                }
                ctx.term.write_line("WASM chunks: done")?;
            }

            // Clean up progress file on success
            delete_download_progress(paths)?;

            ctx.term
                .write_line(&format!("Snapshot downloaded to {}", args.output))?;

            Ok::<_, anyhow::Error>(())
        })
        .await??;

    Ok(())
}
