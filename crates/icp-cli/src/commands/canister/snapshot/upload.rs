use byte_unit::{Byte, UnitType};
use clap::Args;
use icp::context::Context;
use icp::prelude::*;

use super::SnapshotId;
use crate::commands::args;
use crate::operations::misc::format_timestamp;
use crate::operations::snapshot_transfer::{
    BlobType, SnapshotPaths, SnapshotTransferError, UploadProgress, create_transfer_progress_bar,
    delete_upload_progress, load_metadata, load_upload_progress, save_upload_progress,
    upload_blob_from_file, upload_snapshot_metadata, upload_wasm_chunk,
};

#[derive(Debug, Args)]
pub(crate) struct UploadArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Input directory containing the snapshot files
    #[arg(long, short = 'i')]
    input: PathBuf,

    /// Replace an existing snapshot instead of creating a new one
    #[arg(long)]
    replace: Option<SnapshotId>,

    /// Resume a previously interrupted upload
    #[arg(long)]
    resume: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &UploadArgs) -> Result<(), anyhow::Error> {
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

    // Open the snapshot directory with a lock
    let snapshot_dir = SnapshotPaths::new(args.input.clone())?;

    let snapshot_id = snapshot_dir
        .with_write(async |paths| {
            // Load metadata
            let metadata = load_metadata(paths)?;

            ctx.term
                .write_line(&format!("Uploading snapshot to canister {name} ({cid})",))?;
            ctx.term.write_line(&format!(
                "  Original timestamp: {}",
                format_timestamp(metadata.taken_at_timestamp)
            ))?;

            let total_size =
                metadata.wasm_module_size + metadata.wasm_memory_size + metadata.stable_memory_size;
            ctx.term.write_line(&format!(
                "  Total size: {}",
                Byte::from_u64(total_size).get_appropriate_unit(UnitType::Binary)
            ))?;

            // Load or create upload progress
            let mut progress = if args.resume {
                match load_upload_progress(paths) {
                    Ok(progress) => {
                        ctx.term.write_line(&format!(
                            "Resuming upload to snapshot {}",
                            progress.snapshot_id
                        ))?;
                        progress
                    }
                    Err(SnapshotTransferError::NoUploadProgress { .. }) => {
                        return Err(SnapshotTransferError::NoUploadProgress {
                            path: args.input.clone(),
                        }
                        .into());
                    }
                    Err(e) => return Err(e.into()),
                }
            } else {
                // Upload metadata to create a new snapshot
                let replace_snapshot = args.replace.as_ref().map(|s| s.0.as_slice());
                let result =
                    upload_snapshot_metadata(&agent, cid, &metadata, replace_snapshot).await?;

                let snapshot_id_hex = hex::encode(&result.snapshot_id);
                ctx.term
                    .write_line(&format!("Created snapshot {} for upload", snapshot_id_hex))?;

                let mut progress = UploadProgress::new(snapshot_id_hex);
                progress.metadata_uploaded = true;
                save_upload_progress(&progress, paths)?;
                progress
            };

            let snapshot_id_bytes =
                hex::decode(&progress.snapshot_id).expect("invalid snapshot ID in progress file");

            // Upload WASM module
            if metadata.wasm_module_size > 0 {
                if progress.wasm_module_offset < metadata.wasm_module_size {
                    let pb = create_transfer_progress_bar(metadata.wasm_module_size, "WASM module");
                    upload_blob_from_file(
                        &agent,
                        cid,
                        &snapshot_id_bytes,
                        BlobType::WasmModule,
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

            // Upload WASM memory
            if metadata.wasm_memory_size > 0 {
                if progress.wasm_memory_offset < metadata.wasm_memory_size {
                    let pb = create_transfer_progress_bar(metadata.wasm_memory_size, "WASM memory");
                    upload_blob_from_file(
                        &agent,
                        cid,
                        &snapshot_id_bytes,
                        BlobType::WasmMemory,
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

            // Upload stable memory
            if metadata.stable_memory_size > 0 {
                if progress.stable_memory_offset < metadata.stable_memory_size {
                    let pb =
                        create_transfer_progress_bar(metadata.stable_memory_size, "Stable memory");
                    upload_blob_from_file(
                        &agent,
                        cid,
                        &snapshot_id_bytes,
                        BlobType::StableMemory,
                        paths,
                        &mut progress,
                        &pb,
                    )
                    .await?;
                    pb.finish_with_message("done");
                } else {
                    ctx.term.write_line("Stable memory: already complete")?;
                }
            }

            // Upload WASM chunk store
            if !metadata.wasm_chunk_store.is_empty() {
                ctx.term.write_line(&format!(
                    "Uploading {} WASM chunks...",
                    metadata.wasm_chunk_store.len()
                ))?;

                for chunk_hash in &metadata.wasm_chunk_store {
                    let hash_hex = hex::encode(&chunk_hash.hash);
                    if !progress.wasm_chunks_uploaded.contains(&hash_hex) {
                        upload_wasm_chunk(&agent, cid, &snapshot_id_bytes, &chunk_hash.hash, paths)
                            .await?;
                        progress.wasm_chunks_uploaded.insert(hash_hex);
                        save_upload_progress(&progress, paths)?;
                    }
                }
                ctx.term.write_line("WASM chunks: done")?;
            }

            // Clean up progress file on success
            delete_upload_progress(paths)?;

            ctx.term.write_line(&format!(
                "Snapshot {} uploaded successfully",
                progress.snapshot_id
            ))?;

            Ok::<_, anyhow::Error>(progress.snapshot_id)
        })
        .await??;

    ctx.term.write_line(&format!(
        "Use `icp canister snapshot restore {name} {snapshot_id}` to restore from this snapshot"
    ))?;

    Ok(())
}
