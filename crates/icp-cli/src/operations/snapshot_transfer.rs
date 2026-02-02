use std::{
    collections::{BTreeMap, HashSet},
    io::SeekFrom,
};

use backoff::{ExponentialBackoff, backoff::Backoff};
use futures::{StreamExt, stream::FuturesUnordered};
use ic_agent::{Agent, AgentError, export::Principal};
use ic_management_canister_types::{
    ChunkHash, ReadCanisterSnapshotDataArgs, ReadCanisterSnapshotMetadataArgs,
    ReadCanisterSnapshotMetadataResult, SnapshotDataKind, SnapshotDataOffset,
    UploadCanisterSnapshotDataArgs, UploadCanisterSnapshotMetadataArgs,
    UploadCanisterSnapshotMetadataResult,
};
use ic_utils::interfaces::ManagementCanister;
use icp::{
    fs::lock::{DirectoryStructureLock, LWrite, LockError, PathsAccess},
    prelude::*,
};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};
use tracing::debug;

/// Maximum chunk size for snapshot data transfers (2 MB, matching dfx).
pub const MAX_CHUNK_SIZE: u64 = 2_000_000;

/// Provides access to paths within a snapshot directory.
pub struct SnapshotPaths {
    dir: PathBuf,
}

impl SnapshotPaths {
    pub fn new(dir: PathBuf) -> Result<SnapshotDirectory, LockError> {
        DirectoryStructureLock::open_or_create(Self { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn metadata_path(&self) -> PathBuf {
        self.dir.join("metadata.json")
    }

    pub fn wasm_module_path(&self) -> PathBuf {
        self.dir.join("wasm_module.bin")
    }

    pub fn wasm_memory_path(&self) -> PathBuf {
        self.dir.join("wasm_memory.bin")
    }

    pub fn stable_memory_path(&self) -> PathBuf {
        self.dir.join("stable_memory.bin")
    }

    pub fn wasm_chunk_store_dir(&self) -> PathBuf {
        self.dir.join("wasm_chunk_store")
    }

    pub fn wasm_chunk_path(&self, hash: &[u8]) -> PathBuf {
        self.wasm_chunk_store_dir()
            .join(format!("{}.bin", hex::encode(hash)))
    }

    pub fn upload_progress_path(&self) -> PathBuf {
        self.dir.join(".upload_progress.json")
    }

    pub fn download_progress_path(&self) -> PathBuf {
        self.dir.join(".download_progress.json")
    }

    /// Ensure the directory and wasm chunk store subdirectory exist.
    pub fn ensure_dirs(&self) -> Result<(), icp::fs::IoError> {
        icp::fs::create_dir_all(&self.dir)?;
        icp::fs::create_dir_all(&self.wasm_chunk_store_dir())?;
        Ok(())
    }

    pub fn blob_path(&self, blob_type: BlobType) -> PathBuf {
        match blob_type {
            BlobType::WasmModule => self.wasm_module_path(),
            BlobType::WasmMemory => self.wasm_memory_path(),
            BlobType::StableMemory => self.stable_memory_path(),
        }
    }
}

impl PathsAccess for SnapshotPaths {
    fn lock_file(&self) -> PathBuf {
        self.dir.join(".lock")
    }
}

pub type SnapshotDirectory = DirectoryStructureLock<SnapshotPaths>;

#[derive(Debug, Snafu)]
pub enum SnapshotTransferError {
    #[snafu(display("Failed to read snapshot metadata for canister {canister_id}"))]
    ReadMetadata {
        canister_id: Principal,
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },

    #[snafu(display("Failed to read snapshot data chunk at offset {offset}"))]
    ReadDataChunk {
        offset: u64,
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },

    #[snafu(display("Failed to read WASM chunk with hash {hash}"))]
    ReadWasmChunk {
        hash: String,
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },

    #[snafu(display("Failed to upload snapshot metadata for canister {canister_id}"))]
    UploadMetadata {
        canister_id: Principal,
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },

    #[snafu(display("Failed to upload snapshot data chunk at offset {offset}"))]
    UploadDataChunk {
        offset: u64,
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },

    #[snafu(display("Failed to upload WASM chunk with hash {hash}"))]
    UploadWasmChunk {
        hash: String,
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },

    #[snafu(transparent)]
    FsIo { source: icp::fs::IoError },

    #[snafu(transparent)]
    FsRename { source: icp::fs::RenameError },

    #[snafu(transparent)]
    Json { source: icp::fs::json::Error },

    #[snafu(transparent)]
    Lock { source: LockError },

    #[snafu(display("Failed to open blob file for resume at {path}"))]
    OpenBlobForResume {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("Failed to create blob file at {path}"))]
    CreateBlobFile {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("Failed to seek in blob file at {path}"))]
    SeekBlobFile {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("Failed to write chunk to blob file at {path}"))]
    WriteBlobChunk {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("Failed to flush blob file at {path}"))]
    FlushBlobFile {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("Failed to open blob file for upload at {path}"))]
    OpenBlobForUpload {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("Failed to read chunk from blob file at {path}"))]
    ReadBlobChunk {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("Failed to get file size at {path}"))]
    GetBlobFileSize {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display(
        "Directory {path} is not empty. Use --resume to continue a previous download or choose an empty directory."
    ))]
    DirectoryNotEmpty { path: PathBuf },

    #[snafu(display("Cannot resume: no existing download found in {path}"))]
    NoExistingDownload { path: PathBuf },

    #[snafu(display("Cannot resume: no upload progress file found in {path}"))]
    NoUploadProgress { path: PathBuf },

    #[snafu(display(
        "Upload progress file references snapshot {expected} but resuming with {actual}"
    ))]
    SnapshotIdMismatch { expected: String, actual: String },

    #[snafu(display("Missing required file: {path}"))]
    MissingFile { path: PathBuf },

    #[snafu(display("Failed to create download progress file at {path}"))]
    CreateDownloadProgress {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("Failed to write download progress file at {path}"))]
    WriteDownloadProgress {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("Failed to sync download progress file at {path}"))]
    SyncDownloadProgress {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("Invalid snapshot metadata: {reason}"))]
    InvalidSnapshotMetadata { reason: String },
}

/// Tracks upload progress for resumable uploads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadProgress {
    /// The snapshot ID being uploaded to.
    pub snapshot_id: String,
    /// Whether metadata has been uploaded.
    pub metadata_uploaded: bool,
    /// Byte offset for WASM module upload progress.
    pub wasm_module_offset: u64,
    /// Byte offset for WASM memory upload progress.
    pub wasm_memory_offset: u64,
    /// Byte offset for stable memory upload progress.
    pub stable_memory_offset: u64,
    /// Set of WASM chunk hashes that have been uploaded.
    pub wasm_chunks_uploaded: HashSet<String>,
}

impl UploadProgress {
    pub fn new(snapshot_id: String) -> Self {
        Self {
            snapshot_id,
            metadata_uploaded: false,
            wasm_module_offset: 0,
            wasm_memory_offset: 0,
            stable_memory_offset: 0,
            wasm_chunks_uploaded: HashSet::new(),
        }
    }
}

/// Tracks download progress for a single blob.
/// Uses a write frontier plus a set of chunks completed ahead of the frontier.
/// All chunks before the frontier are assumed complete. Only gaps (chunks ahead
/// of the frontier) are tracked, so memory usage is bounded.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlobDownloadProgress {
    /// The write frontier - all chunks with offset < frontier are complete.
    pub frontier: u64,
    /// Chunk offsets that completed ahead of the frontier (filled gaps).
    pub ahead: HashSet<u64>,
}

impl BlobDownloadProgress {
    /// Record a completed chunk and advance the frontier if possible.
    pub fn mark_complete(&mut self, offset: u64, total_size: u64) {
        if offset < self.frontier {
            // Already implicitly complete
            return;
        }

        if offset > self.frontier {
            // Completed ahead of frontier
            self.ahead.insert(offset);
            return;
        }

        // offset == frontier: advance it
        self.frontier += chunk_size_at(offset, total_size);

        // Advance further using any chunks that are now at the frontier
        while self.ahead.remove(&self.frontier) {
            self.frontier += chunk_size_at(self.frontier, total_size);
        }
    }

    /// Check if a chunk at the given offset needs to be downloaded.
    pub fn needs_download(&self, offset: u64) -> bool {
        offset >= self.frontier && !self.ahead.contains(&offset)
    }

    /// Check if download is complete for a blob of the given total size.
    pub fn is_complete(&self, total_size: u64) -> bool {
        self.frontier >= total_size
    }
}

/// Calculate chunk size at a given offset for a blob of total_size.
fn chunk_size_at(offset: u64, total_size: u64) -> u64 {
    std::cmp::min(MAX_CHUNK_SIZE, total_size.saturating_sub(offset))
}

/// Tracks download progress for resumable downloads.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DownloadProgress {
    pub wasm_module: BlobDownloadProgress,
    pub wasm_memory: BlobDownloadProgress,
    pub stable_memory: BlobDownloadProgress,
}

impl DownloadProgress {
    pub fn blob_progress(&self, blob_type: BlobType) -> &BlobDownloadProgress {
        match blob_type {
            BlobType::WasmModule => &self.wasm_module,
            BlobType::WasmMemory => &self.wasm_memory,
            BlobType::StableMemory => &self.stable_memory,
        }
    }

    pub fn blob_progress_mut(&mut self, blob_type: BlobType) -> &mut BlobDownloadProgress {
        match blob_type {
            BlobType::WasmModule => &mut self.wasm_module,
            BlobType::WasmMemory => &mut self.wasm_memory,
            BlobType::StableMemory => &mut self.stable_memory,
        }
    }
}

/// Identifies which type of blob is being transferred.
#[derive(Debug, Clone, Copy)]
pub enum BlobType {
    WasmModule,
    WasmMemory,
    StableMemory,
}

impl BlobType {
    pub fn make_read_kind(&self, offset: u64, size: u64) -> SnapshotDataKind {
        match self {
            BlobType::WasmModule => SnapshotDataKind::WasmModule { offset, size },
            BlobType::WasmMemory => SnapshotDataKind::WasmMemory { offset, size },
            BlobType::StableMemory => SnapshotDataKind::StableMemory { offset, size },
        }
    }

    pub fn make_upload_offset(&self, offset: u64) -> SnapshotDataOffset {
        match self {
            BlobType::WasmModule => SnapshotDataOffset::WasmModule { offset },
            BlobType::WasmMemory => SnapshotDataOffset::WasmMemory { offset },
            BlobType::StableMemory => SnapshotDataOffset::StableMemory { offset },
        }
    }
}

/// Check if an agent error is retryable.
fn is_retryable(error: &AgentError) -> bool {
    matches!(
        error,
        AgentError::TimeoutWaitingForResponse() | AgentError::TransportError(_)
    )
}

/// Execute an async operation with exponential backoff retry.
async fn with_retry<F, Fut, T>(operation: F) -> Result<T, AgentError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, AgentError>>,
{
    let mut backoff = ExponentialBackoff {
        max_elapsed_time: Some(std::time::Duration::from_secs(60)),
        ..ExponentialBackoff::default()
    };

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(err) if is_retryable(&err) => {
                if let Some(duration) = backoff.next_backoff() {
                    debug!("Retryable error, waiting {:?}: {}", duration, err);
                    tokio::time::sleep(duration).await;
                } else {
                    return Err(err);
                }
            }
            Err(err) => return Err(err),
        }
    }
}

/// Create a progress bar for byte transfers.
pub fn create_transfer_progress_bar(total_bytes: u64, label: &str) -> ProgressBar {
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{prefix} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .expect("invalid progress bar template")
            .progress_chars("#>-"),
    );
    pb.set_prefix(label.to_string());
    pb
}

/// Read snapshot metadata from a canister.
pub async fn read_snapshot_metadata(
    agent: &Agent,
    canister_id: Principal,
    snapshot_id: &[u8],
) -> Result<ReadCanisterSnapshotMetadataResult, SnapshotTransferError> {
    let mgmt = ManagementCanister::create(agent);

    let args = ReadCanisterSnapshotMetadataArgs {
        canister_id,
        snapshot_id: snapshot_id.to_vec(),
    };

    let (metadata,) = with_retry(|| async {
        mgmt.read_canister_snapshot_metadata(&canister_id, &args)
            .await
    })
    .await
    .context(ReadMetadataSnafu { canister_id })?;

    Ok(metadata)
}

/// Upload snapshot metadata to create a new snapshot.
pub async fn upload_snapshot_metadata(
    agent: &Agent,
    canister_id: Principal,
    metadata: &ReadCanisterSnapshotMetadataResult,
    replace_snapshot: Option<&[u8]>,
) -> Result<UploadCanisterSnapshotMetadataResult, SnapshotTransferError> {
    let mgmt = ManagementCanister::create(agent);

    // Convert Option<SnapshotMetadataGlobal> to SnapshotMetadataGlobal, failing on None
    let globals = metadata
        .globals
        .iter()
        .cloned()
        .collect::<Option<Vec<_>>>()
        .ok_or(SnapshotTransferError::InvalidSnapshotMetadata {
            reason: "snapshot metadata contains unparseable globals".to_string(),
        })?;

    let args = UploadCanisterSnapshotMetadataArgs {
        canister_id,
        replace_snapshot: replace_snapshot.map(|s| s.to_vec()),
        wasm_module_size: metadata.wasm_module_size,
        globals,
        wasm_memory_size: metadata.wasm_memory_size,
        stable_memory_size: metadata.stable_memory_size,
        certified_data: metadata.certified_data.clone(),
        global_timer: metadata.global_timer.clone(),
        on_low_wasm_memory_hook_status: metadata.on_low_wasm_memory_hook_status.clone(),
    };

    let (result,) = with_retry(|| async {
        mgmt.upload_canister_snapshot_metadata(&canister_id, &args)
            .await
    })
    .await
    .context(UploadMetadataSnafu { canister_id })?;

    Ok(result)
}

/// Download a blob (wasm_module, wasm_memory, or stable_memory) to a file.
///
/// Writes chunks directly at their offset (no in-memory buffering).
/// Tracks progress so gaps can be filled on resume.
/// The agent handles rate limiting and semaphoring internally.
pub async fn download_blob_to_file(
    agent: &Agent,
    canister_id: Principal,
    snapshot_id: &[u8],
    blob_type: BlobType,
    total_size: u64,
    paths: LWrite<&SnapshotPaths>,
    progress: &mut DownloadProgress,
    progress_bar: &ProgressBar,
) -> Result<(), SnapshotTransferError> {
    let output_path = paths.blob_path(blob_type);

    if total_size == 0 {
        icp::fs::write(&output_path, &[])?;
        return Ok(());
    }

    let blob_progress = progress.blob_progress(blob_type);
    if blob_progress.is_complete(total_size) {
        return Ok(());
    }

    let mgmt = ManagementCanister::create(agent);

    // Create or open file for random-access writing
    let file = if output_path.exists() {
        File::options()
            .write(true)
            .open(&output_path)
            .await
            .context(OpenBlobForResumeSnafu { path: &output_path })?
    } else {
        // Pre-allocate file to total size
        let f = File::create(&output_path)
            .await
            .context(CreateBlobFileSnafu { path: &output_path })?;
        f.set_len(total_size)
            .await
            .context(CreateBlobFileSnafu { path: &output_path })?;
        f
    };

    // Set initial progress based on frontier
    let initial_bytes = progress.blob_progress(blob_type).frontier;
    progress_bar.set_position(initial_bytes);

    // Determine which chunks need downloading
    let snapshot_id_vec = snapshot_id.to_vec();
    let mut in_progress: FuturesUnordered<_> = FuturesUnordered::new();

    let mut offset = 0u64;
    while offset < total_size {
        let chunk_size = chunk_size_at(offset, total_size);

        if progress.blob_progress(blob_type).needs_download(offset) {
            let chunk_offset = offset;
            let args = ReadCanisterSnapshotDataArgs {
                canister_id,
                snapshot_id: snapshot_id_vec.clone(),
                kind: blob_type.make_read_kind(chunk_offset, chunk_size),
            };

            let mgmt = mgmt.clone();
            in_progress.push(async move {
                let result = with_retry(|| async {
                    mgmt.read_canister_snapshot_data(&canister_id, &args).await
                })
                .await
                .context(ReadDataChunkSnafu {
                    offset: chunk_offset,
                })?;
                Ok::<_, SnapshotTransferError>((chunk_offset, result.0.chunk))
            });
        }

        offset += chunk_size;
    }

    // Process completed chunks - write directly at offset
    use std::sync::Arc;
    use tokio::sync::Mutex;
    let file = Arc::new(Mutex::new(file));

    while let Some(result) = in_progress.next().await {
        let (chunk_offset, chunk) = result?;

        // Write chunk at its offset
        {
            let mut f = file.lock().await;
            f.seek(SeekFrom::Start(chunk_offset))
                .await
                .context(SeekBlobFileSnafu { path: &output_path })?;
            f.write_all(&chunk)
                .await
                .context(WriteBlobChunkSnafu { path: &output_path })?;
            f.sync_data()
                .await
                .context(FlushBlobFileSnafu { path: &output_path })?;
        }

        // Update progress
        progress
            .blob_progress_mut(blob_type)
            .mark_complete(chunk_offset, total_size);
        save_download_progress(progress, paths)?;

        // Update progress bar to show frontier position
        progress_bar.set_position(progress.blob_progress(blob_type).frontier);
    }

    Ok(())
}

/// Download a single WASM chunk by hash.
pub async fn download_wasm_chunk(
    agent: &Agent,
    canister_id: Principal,
    snapshot_id: &[u8],
    chunk_hash: &ChunkHash,
    paths: LWrite<&SnapshotPaths>,
) -> Result<(), SnapshotTransferError> {
    let mgmt = ManagementCanister::create(agent);

    let args = ReadCanisterSnapshotDataArgs {
        canister_id,
        snapshot_id: snapshot_id.to_vec(),
        kind: SnapshotDataKind::WasmChunk {
            hash: chunk_hash.hash.clone(),
        },
    };

    let hash_hex = hex::encode(&chunk_hash.hash);
    let output_path = paths.wasm_chunk_path(&chunk_hash.hash);

    let (result,) =
        with_retry(|| async { mgmt.read_canister_snapshot_data(&canister_id, &args).await })
            .await
            .context(ReadWasmChunkSnafu { hash: &hash_hex })?;

    icp::fs::write(&output_path, &result.chunk)?;

    Ok(())
}

/// Upload a blob (wasm_module, wasm_memory, or stable_memory) from a file.
///
/// Uses parallel chunk uploading. The agent handles rate limiting internally.
/// Saves progress after each successful chunk for resume support.
/// Returns the final byte offset after all uploads complete.
pub async fn upload_blob_from_file(
    agent: &Agent,
    canister_id: Principal,
    snapshot_id: &[u8],
    blob_type: BlobType,
    paths: LWrite<&SnapshotPaths>,
    progress: &mut UploadProgress,
    progress_bar: &ProgressBar,
) -> Result<u64, SnapshotTransferError> {
    let input_path = paths.blob_path(blob_type);
    let file_size = std::fs::metadata(&input_path)
        .context(GetBlobFileSizeSnafu { path: &input_path })?
        .len();

    if file_size == 0 {
        return Ok(0);
    }

    let start_offset = match blob_type {
        BlobType::WasmModule => progress.wasm_module_offset,
        BlobType::WasmMemory => progress.wasm_memory_offset,
        BlobType::StableMemory => progress.stable_memory_offset,
    };

    let mgmt = ManagementCanister::create(agent);

    let mut file = File::open(&input_path)
        .await
        .context(OpenBlobForUploadSnafu { path: &input_path })?;

    if start_offset > 0 {
        file.seek(SeekFrom::Start(start_offset))
            .await
            .context(SeekBlobFileSnafu { path: &input_path })?;
    }

    progress_bar.set_position(start_offset);

    // Read all chunks and launch uploads concurrently
    let snapshot_id_vec = snapshot_id.to_vec();
    let mut in_progress: FuturesUnordered<_> = FuturesUnordered::new();

    let mut current_offset = start_offset;
    while current_offset < file_size {
        let chunk_size = std::cmp::min(MAX_CHUNK_SIZE, file_size - current_offset) as usize;
        let mut chunk = vec![0u8; chunk_size];
        file.read_exact(&mut chunk)
            .await
            .context(ReadBlobChunkSnafu { path: &input_path })?;

        let offset = current_offset;
        current_offset += chunk_size as u64;

        let args = UploadCanisterSnapshotDataArgs {
            canister_id,
            snapshot_id: snapshot_id_vec.clone(),
            kind: blob_type.make_upload_offset(offset),
            chunk,
        };

        let mgmt = mgmt.clone();
        in_progress.push(async move {
            with_retry(|| async {
                mgmt.upload_canister_snapshot_data(&canister_id, &args)
                    .await
            })
            .await
            .context(UploadDataChunkSnafu { offset })?;
            Ok::<_, SnapshotTransferError>((offset, args.chunk.len() as u64))
        });
    }

    // Track completed uploads for ordered progress reporting
    let mut completed: BTreeMap<u64, u64> = BTreeMap::new();
    let mut next_report_offset = start_offset;
    let mut first_error: Option<SnapshotTransferError> = None;

    while let Some(result) = in_progress.next().await {
        match result {
            Ok((offset, size)) => {
                completed.insert(offset, size);

                // Update progress in order and save after each advancement
                while let Some(&size) = completed.get(&next_report_offset) {
                    completed.remove(&next_report_offset);
                    next_report_offset += size;
                    progress_bar.set_position(next_report_offset);

                    // Update and save progress
                    match blob_type {
                        BlobType::WasmModule => progress.wasm_module_offset = next_report_offset,
                        BlobType::WasmMemory => progress.wasm_memory_offset = next_report_offset,
                        BlobType::StableMemory => {
                            progress.stable_memory_offset = next_report_offset
                        }
                    }
                    save_upload_progress(progress, paths)?;
                }
            }
            Err(e) => {
                // Record first error but continue processing to save any completed chunks
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }
    }

    // Return error if any chunk failed
    if let Some(e) = first_error {
        return Err(e);
    }

    Ok(next_report_offset)
}

/// Upload a single WASM chunk.
pub async fn upload_wasm_chunk(
    agent: &Agent,
    canister_id: Principal,
    snapshot_id: &[u8],
    chunk_hash: &[u8],
    paths: LWrite<&SnapshotPaths>,
) -> Result<(), SnapshotTransferError> {
    let mgmt = ManagementCanister::create(agent);

    let chunk_path = paths.wasm_chunk_path(chunk_hash);
    let chunk = icp::fs::read(&chunk_path)?;

    let args = UploadCanisterSnapshotDataArgs {
        canister_id,
        snapshot_id: snapshot_id.to_vec(),
        kind: SnapshotDataOffset::WasmChunk,
        chunk,
    };

    let hash_hex = hex::encode(chunk_hash);

    with_retry(|| async {
        mgmt.upload_canister_snapshot_data(&canister_id, &args)
            .await
    })
    .await
    .context(UploadWasmChunkSnafu { hash: hash_hex })?;

    Ok(())
}

/// Save upload progress to a file.
pub fn save_upload_progress(
    progress: &UploadProgress,
    paths: LWrite<&SnapshotPaths>,
) -> Result<(), SnapshotTransferError> {
    icp::fs::json::save(&paths.upload_progress_path(), progress)?;
    Ok(())
}

/// Load upload progress from a file.
pub fn load_upload_progress(
    paths: LWrite<&SnapshotPaths>,
) -> Result<UploadProgress, SnapshotTransferError> {
    let progress_path = paths.upload_progress_path();
    if !progress_path.exists() {
        return Err(SnapshotTransferError::NoUploadProgress {
            path: paths.dir().to_path_buf(),
        });
    }
    Ok(icp::fs::json::load(&progress_path)?)
}

/// Delete upload progress file.
pub fn delete_upload_progress(paths: LWrite<&SnapshotPaths>) -> Result<(), SnapshotTransferError> {
    let progress_path = paths.upload_progress_path();
    if progress_path.exists() {
        icp::fs::remove_file(&progress_path)?;
    }
    Ok(())
}

/// Save download progress to a file atomically.
pub fn save_download_progress(
    progress: &DownloadProgress,
    paths: LWrite<&SnapshotPaths>,
) -> Result<(), SnapshotTransferError> {
    use std::io::Write;

    let target_path = paths.download_progress_path();
    let tmp_path = paths.dir().join(".download_progress.json.tmp");

    // Write to temp file
    let contents =
        serde_json::to_string_pretty(progress).expect("DownloadProgress is always serializable");
    let mut file = std::fs::File::create(&tmp_path)
        .context(CreateDownloadProgressSnafu { path: &tmp_path })?;
    file.write_all(contents.as_bytes())
        .context(WriteDownloadProgressSnafu { path: &tmp_path })?;
    file.sync_all()
        .context(SyncDownloadProgressSnafu { path: &tmp_path })?;
    drop(file);

    // Atomic rename
    icp::fs::rename(&tmp_path, &target_path)?;

    Ok(())
}

/// Load download progress from a file, or return default if none exists.
pub fn load_download_progress(
    paths: LWrite<&SnapshotPaths>,
) -> Result<DownloadProgress, SnapshotTransferError> {
    Ok(icp::fs::json::load_or_default(
        &paths.download_progress_path(),
    )?)
}

/// Delete download progress file.
pub fn delete_download_progress(
    paths: LWrite<&SnapshotPaths>,
) -> Result<(), SnapshotTransferError> {
    let progress_path = paths.download_progress_path();
    if progress_path.exists() {
        icp::fs::remove_file(&progress_path)?;
    }
    Ok(())
}

/// Save metadata to the snapshot directory.
pub fn save_metadata(
    metadata: &ReadCanisterSnapshotMetadataResult,
    paths: LWrite<&SnapshotPaths>,
) -> Result<(), SnapshotTransferError> {
    icp::fs::json::save(&paths.metadata_path(), metadata)?;
    Ok(())
}

/// Load metadata from the snapshot directory.
pub fn load_metadata(
    paths: LWrite<&SnapshotPaths>,
) -> Result<ReadCanisterSnapshotMetadataResult, SnapshotTransferError> {
    let metadata_path = paths.metadata_path();
    if !metadata_path.exists() {
        return Err(SnapshotTransferError::MissingFile {
            path: metadata_path,
        });
    }
    Ok(icp::fs::json::load(&metadata_path)?)
}
