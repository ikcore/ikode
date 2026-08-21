use crate::{GraphAdapter, GraphError, GraphEvent, GraphTxn};
use crc32fast::Hasher;
use fs2::FileExt;
use lz4_flex::{compress_prepend_size, decompress_size_prepended};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 4] = b"IKWL";
const FORMAT_VERSION: u8 = 1;
const HEADER_LEN: usize = 24;
const FLAG_COMPRESSED: u8 = 1;
const KIND_TXN: u8 = 1;
const KIND_COMMIT: u8 = 2;
const KIND_SNAPSHOT: u8 = 3;

/// How a committed WAL transaction is pushed toward durable storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    /// Flush file data before acknowledging a transaction. This is the default.
    SyncData,
    /// Flush file data and metadata before acknowledging a transaction.
    SyncAll,
    /// Only flush userspace buffers. Intended for explicitly rebuildable caches.
    Relaxed,
}

/// Byte and durability limits for the WAL.
#[derive(Debug, Clone)]
pub struct WalOptions {
    pub durability: Durability,
    /// Maximum uncompressed transaction payload accepted by a single record.
    pub max_record_bytes: usize,
    /// Maximum uncompressed graph snapshot accepted during checkpoint/recovery.
    pub max_snapshot_bytes: usize,
    /// Payloads at or above this size are compressed when that saves bytes.
    pub compression_threshold_bytes: usize,
    /// Checkpoint before the next transaction once this many delta bytes accumulated.
    /// Set to zero to disable automatic size-based checkpoints.
    pub checkpoint_after_bytes: u64,
    /// Repair an incomplete final record by truncating to the last commit boundary.
    pub repair_trailing_bytes: bool,
}

impl Default for WalOptions {
    fn default() -> Self {
        Self {
            durability: Durability::SyncData,
            max_record_bytes: 64 * 1024 * 1024,
            max_snapshot_bytes: 1024 * 1024 * 1024,
            compression_threshold_bytes: 4 * 1024,
            checkpoint_after_bytes: 64 * 1024 * 1024,
            repair_trailing_bytes: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub file_bytes: u64,
    pub recovered_transactions: u64,
    pub legacy_transactions: u64,
    pub snapshots_loaded: u64,
    pub repaired_bytes: u64,
    pub stored_payload_bytes: u64,
    pub decoded_payload_bytes: u64,
    pub last_sequence: u64,
}

impl RecoveryReport {
    pub fn compression_saved_bytes(&self) -> u64 {
        self.decoded_payload_bytes
            .saturating_sub(self.stored_payload_bytes)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WalStats {
    pub file_bytes: u64,
    pub bytes_since_checkpoint: u64,
    pub last_sequence: u64,
    pub committed_transactions: u64,
    pub snapshots: u64,
    pub stored_payload_bytes: u64,
    pub original_payload_bytes: u64,
}

impl WalStats {
    pub fn compression_saved_bytes(&self) -> u64 {
        self.original_payload_bytes
            .saturating_sub(self.stored_payload_bytes)
    }
}

struct PendingWrite {
    sequence: u64,
    start_offset: u64,
    stored_payload_bytes: u64,
    original_payload_bytes: u64,
}

struct RecoveredWal {
    graph: GraphAdapter,
    report: RecoveryReport,
    committed_len: u64,
    bytes_since_checkpoint: u64,
}

struct DecodedFrame {
    kind: u8,
    sequence: u64,
    payload: Vec<u8>,
    stored_payload_bytes: u64,
    end_offset: u64,
}

enum FrameRead {
    Complete(DecodedFrame),
    IncompleteTail,
}

#[derive(Clone, Copy)]
enum PayloadLimit {
    Record,
    Snapshot,
}

struct BoundedBuffer {
    bytes: Vec<u8>,
    max: usize,
    attempted_size: usize,
    exceeded: bool,
}

impl BoundedBuffer {
    fn new(max: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(max.min(64 * 1024)),
            max,
            attempted_size: 0,
            exceeded: false,
        }
    }
}

impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.attempted_size = self.bytes.len().saturating_add(bytes.len());
        if self.attempted_size > self.max {
            self.exceeded = true;
            return Err(std::io::Error::other(
                "serialized WAL payload exceeds limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A single-writer, framed WAL with checksum validation and bounded decoding.
pub struct EventLogger {
    file: File,
    _lock_file: File,
    path: PathBuf,
    options: WalOptions,
    last_sequence: u64,
    committed_len: u64,
    bytes_since_checkpoint: u64,
    pending: Option<PendingWrite>,
    stats: WalStats,
}

impl EventLogger {
    /// Open, lock, recover, and position a WAL for appending.
    pub fn open<P: AsRef<Path>>(
        path: P,
        options: WalOptions,
    ) -> Result<(Self, GraphAdapter, RecoveryReport), GraphError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let lock_path = suffix_path(&path, ".lock");
        let lock_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        FileExt::try_lock_exclusive(&lock_file)
            .map_err(|_| GraphError::WalLocked(path.display().to_string()))?;

        let backup_path = suffix_path(&path, ".previous");
        if !path.exists() && backup_path.exists() {
            std::fs::rename(&backup_path, &path)?;
        }

        let mut file = open_active(&path)?;
        let recovered = match recover_file(&mut file, &options) {
            Ok(recovered) => {
                let _ = std::fs::remove_file(&backup_path);
                recovered
            }
            Err(primary_error) if backup_path.exists() => {
                drop(file);
                let corrupt_path = suffix_path(&path, ".corrupt");
                let _ = std::fs::remove_file(&corrupt_path);
                std::fs::rename(&path, &corrupt_path)?;
                std::fs::rename(&backup_path, &path)?;
                file = open_active(&path)?;
                match recover_file(&mut file, &options) {
                    Ok(recovered) => recovered,
                    Err(_) => return Err(primary_error),
                }
            }
            Err(error) => return Err(error),
        };

        if recovered.report.repaired_bytes > 0 {
            file.set_len(recovered.committed_len)?;
        }
        file.seek(SeekFrom::End(0))?;
        let _ = std::fs::remove_file(suffix_path(&path, ".next"));

        let stats = WalStats {
            file_bytes: recovered.committed_len,
            bytes_since_checkpoint: recovered.bytes_since_checkpoint,
            last_sequence: recovered.report.last_sequence,
            committed_transactions: recovered.report.recovered_transactions,
            snapshots: recovered.report.snapshots_loaded,
            stored_payload_bytes: recovered.report.stored_payload_bytes,
            original_payload_bytes: recovered.report.decoded_payload_bytes,
        };
        let report = recovered.report.clone();
        let graph = recovered.graph;
        Ok((
            Self {
                file,
                _lock_file: lock_file,
                path,
                options,
                last_sequence: report.last_sequence,
                committed_len: recovered.committed_len,
                bytes_since_checkpoint: recovered.bytes_since_checkpoint,
                pending: None,
                stats,
            },
            graph,
            report,
        ))
    }

    /// Backwards-compatible name. Opening now always includes recovery.
    pub fn open_append<P: AsRef<Path>>(path: P) -> Result<Self, GraphError> {
        let (logger, _, _) = Self::open(path, WalOptions::default())?;
        Ok(logger)
    }

    pub fn options(&self) -> &WalOptions {
        &self.options
    }

    pub fn needs_checkpoint(&self) -> bool {
        self.options.checkpoint_after_bytes > 0
            && self.bytes_since_checkpoint >= self.options.checkpoint_after_bytes
    }

    pub fn stats(&self) -> WalStats {
        let mut stats = self.stats.clone();
        stats.file_bytes = self
            .file
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(self.committed_len);
        stats.bytes_since_checkpoint = self.bytes_since_checkpoint;
        stats.last_sequence = self.last_sequence;
        stats
    }

    /// Append the transaction body before mutating the graph. The transaction is
    /// not committed until [`Self::commit`] writes its marker and syncs the file.
    pub fn begin(&mut self, txn: &GraphTxn) -> Result<u64, GraphError> {
        if self.pending.is_some() {
            return Err(GraphError::Conflict(
                "attempted to begin a WAL transaction while another is pending".into(),
            ));
        }
        let raw = encode_json_bounded(txn, self.options.max_record_bytes, PayloadLimit::Record)?;
        let sequence = self
            .last_sequence
            .checked_add(1)
            .ok_or_else(|| GraphError::Conflict("WAL sequence number exhausted".to_string()))?;
        let start_offset = self.file.seek(SeekFrom::End(0))?;
        let (stored, original) =
            match write_frame(&mut self.file, KIND_TXN, sequence, &raw, &self.options) {
                Ok(bytes) => bytes,
                Err(error) => {
                    let _ = self.file.set_len(self.committed_len);
                    let _ = self.file.seek(SeekFrom::End(0));
                    return Err(error);
                }
            };
        self.pending = Some(PendingWrite {
            sequence,
            start_offset,
            stored_payload_bytes: stored,
            original_payload_bytes: original,
        });
        Ok(sequence)
    }

    /// Commit and durably publish the pending transaction.
    pub fn commit(&mut self, sequence: u64) -> Result<(), GraphError> {
        let pending = self.pending.as_ref().ok_or_else(|| {
            GraphError::Conflict("attempted to commit without a pending WAL transaction".into())
        })?;
        if pending.sequence != sequence {
            return Err(GraphError::Conflict(format!(
                "WAL commit sequence {sequence} does not match pending sequence {}",
                pending.sequence
            )));
        }
        write_frame(&mut self.file, KIND_COMMIT, sequence, &[], &self.options)?;
        sync_file(&mut self.file, self.options.durability)?;

        let end = self.file.seek(SeekFrom::End(0))?;
        let pending = self.pending.take().expect("pending checked above");
        self.committed_len = end;
        self.last_sequence = sequence;
        self.bytes_since_checkpoint = self
            .bytes_since_checkpoint
            .saturating_add(end.saturating_sub(pending.start_offset));
        self.stats.file_bytes = end;
        self.stats.last_sequence = sequence;
        self.stats.committed_transactions = self.stats.committed_transactions.saturating_add(1);
        self.stats.stored_payload_bytes = self
            .stats
            .stored_payload_bytes
            .saturating_add(pending.stored_payload_bytes);
        self.stats.original_payload_bytes = self
            .stats
            .original_payload_bytes
            .saturating_add(pending.original_payload_bytes);
        Ok(())
    }

    /// Remove an uncommitted transaction record. Recovery would ignore it as well,
    /// but eager truncation keeps failed submissions from growing the file.
    pub fn abort_pending(&mut self) -> Result<(), GraphError> {
        if self.pending.take().is_some() {
            self.file.set_len(self.committed_len)?;
            self.file.seek(SeekFrom::End(0))?;
        }
        Ok(())
    }

    /// Re-read committed state after an application or durability failure.
    pub fn recover_current(&mut self) -> Result<(GraphAdapter, RecoveryReport), GraphError> {
        self.file.flush()?;
        let recovered = recover_file(&mut self.file, &self.options)?;
        if recovered.report.repaired_bytes > 0 {
            self.file.set_len(recovered.committed_len)?;
        }
        self.file.seek(SeekFrom::End(0))?;
        self.last_sequence = recovered.report.last_sequence;
        self.committed_len = recovered.committed_len;
        self.bytes_since_checkpoint = recovered.bytes_since_checkpoint;
        self.pending = None;
        self.stats.file_bytes = recovered.committed_len;
        self.stats.last_sequence = recovered.report.last_sequence;
        Ok((recovered.graph, recovered.report))
    }

    /// Replace all historical frames with one compressed, checksummed graph snapshot.
    pub fn checkpoint(&mut self, graph: &GraphAdapter) -> Result<(u64, u64, u64), GraphError> {
        if self.pending.is_some() {
            return Err(GraphError::Conflict(
                "cannot checkpoint while a WAL transaction is pending".into(),
            ));
        }
        let raw = encode_json_bounded(
            graph,
            self.options.max_snapshot_bytes,
            PayloadLimit::Snapshot,
        )?;

        let before = self.file.metadata()?.len();
        let next_path = suffix_path(&self.path, ".next");
        let backup_path = suffix_path(&self.path, ".previous");
        let _ = std::fs::remove_file(&next_path);
        let mut next = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&next_path)?;
        write_frame(
            &mut next,
            KIND_SNAPSHOT,
            self.last_sequence,
            &raw,
            &self.options,
        )?;
        next.sync_all()?;
        let after = next.metadata()?.len();
        drop(next);

        // The lock lives in a sidecar, so the active WAL handle can be closed while
        // performing a Windows-compatible recoverable replacement.
        let placeholder = self._lock_file.try_clone()?;
        let old = std::mem::replace(&mut self.file, placeholder);
        drop(old);
        let _ = std::fs::remove_file(&backup_path);
        if let Err(error) = std::fs::rename(&self.path, &backup_path) {
            self.file = open_active(&self.path)?;
            self.file.seek(SeekFrom::End(0))?;
            return Err(error.into());
        }
        if let Err(error) = std::fs::rename(&next_path, &self.path) {
            let _ = std::fs::rename(&backup_path, &self.path);
            self.file = open_active(&self.path)?;
            self.file.seek(SeekFrom::End(0))?;
            return Err(error.into());
        }
        self.file = open_active(&self.path)?;
        self.file.seek(SeekFrom::End(0))?;
        sync_parent(&self.path)?;
        let _ = std::fs::remove_file(&backup_path);

        self.committed_len = after;
        self.bytes_since_checkpoint = 0;
        self.stats.file_bytes = after;
        self.stats.bytes_since_checkpoint = 0;
        self.stats.snapshots = self.stats.snapshots.saturating_add(1);
        self.stats.stored_payload_bytes = after.saturating_sub(HEADER_LEN as u64);
        self.stats.original_payload_bytes = raw.len() as u64;
        Ok((before, after, raw.len() as u64))
    }
}

fn recover_file(file: &mut File, options: &WalOptions) -> Result<RecoveredWal, GraphError> {
    file.seek(SeekFrom::Start(0))?;
    let file_len = file.metadata()?.len();
    let mut reader = BufReader::new(file);
    let mut graph = GraphAdapter::new();
    let mut report = RecoveryReport {
        file_bytes: file_len,
        ..RecoveryReport::default()
    };
    let mut last_sequence = 0_u64;
    let mut last_committed_offset = 0_u64;
    let mut bytes_since_checkpoint = 0_u64;
    let mut pending: Option<(u64, GraphTxn, u64)> = None;
    let mut framed = false;
    let mut incomplete_tail = false;

    loop {
        let offset = reader.stream_position()?;
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }

        if available.len() >= MAGIC.len() && &available[..MAGIC.len()] == MAGIC {
            framed = true;
            match read_frame(&mut reader, file_len, options, offset)? {
                FrameRead::IncompleteTail => {
                    incomplete_tail = true;
                    break;
                }
                FrameRead::Complete(frame) => {
                    report.stored_payload_bytes = report
                        .stored_payload_bytes
                        .saturating_add(frame.stored_payload_bytes);
                    report.decoded_payload_bytes = report
                        .decoded_payload_bytes
                        .saturating_add(frame.payload.len() as u64);
                    match frame.kind {
                        KIND_TXN => {
                            if pending.is_some() {
                                return Err(corrupt(
                                    offset,
                                    "encountered a transaction before the previous one committed",
                                ));
                            }
                            if frame.sequence <= last_sequence {
                                return Err(corrupt(
                                    offset,
                                    format!(
                                        "transaction sequence {} is not greater than {last_sequence}",
                                        frame.sequence
                                    ),
                                ));
                            }
                            let txn = decode_json::<GraphTxn>(&frame.payload, offset)?;
                            pending = Some((frame.sequence, txn, offset));
                        }
                        KIND_COMMIT => {
                            if !frame.payload.is_empty() {
                                return Err(corrupt(offset, "commit record has a payload"));
                            }
                            let Some((sequence, txn, start)) = pending.take() else {
                                return Err(corrupt(offset, "commit has no pending transaction"));
                            };
                            if sequence != frame.sequence {
                                return Err(corrupt(
                                    offset,
                                    format!(
                                        "commit sequence {} does not match pending sequence {sequence}",
                                        frame.sequence
                                    ),
                                ));
                            }
                            graph.apply_txn(&txn).map_err(|error| {
                                corrupt(
                                    offset,
                                    format!("committed transaction is invalid: {error}"),
                                )
                            })?;
                            last_sequence = sequence;
                            last_committed_offset = frame.end_offset;
                            bytes_since_checkpoint = bytes_since_checkpoint
                                .saturating_add(frame.end_offset.saturating_sub(start));
                            report.recovered_transactions =
                                report.recovered_transactions.saturating_add(1);
                        }
                        KIND_SNAPSHOT => {
                            if pending.is_some() || offset != 0 {
                                return Err(corrupt(
                                    offset,
                                    "snapshot must be the first record and cannot interrupt a transaction",
                                ));
                            }
                            let mut restored = decode_json::<GraphAdapter>(&frame.payload, offset)?;
                            restored.finish_snapshot_restore();
                            graph = restored;
                            last_sequence = frame.sequence;
                            last_committed_offset = frame.end_offset;
                            bytes_since_checkpoint = 0;
                            report.snapshots_loaded = report.snapshots_loaded.saturating_add(1);
                        }
                        _ => return Err(corrupt(offset, "unknown WAL record kind")),
                    }
                }
            }
            continue;
        }

        if framed {
            if file_len.saturating_sub(offset) < HEADER_LEN as u64 {
                incomplete_tail = true;
                break;
            }
            return Err(corrupt(offset, "missing WAL frame magic"));
        }

        // Legacy JSONL prefix. Each complete legacy line was acknowledged by the
        // previous implementation and is therefore treated as committed.
        let mut line = Vec::new();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        if line.len() > options.max_record_bytes {
            return Err(GraphError::WalRecordTooLarge {
                size: line.len(),
                max: options.max_record_bytes,
            });
        }
        if !line.ends_with(b"\n") {
            incomplete_tail = true;
            break;
        }
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        if line.is_empty() {
            last_committed_offset = reader.stream_position()?;
            continue;
        }
        let event = decode_json::<GraphEvent>(&line, offset)?;
        match event {
            GraphEvent::Txn(txn) => {
                graph.apply_txn(&txn).map_err(|error| {
                    corrupt(offset, format!("legacy transaction is invalid: {error}"))
                })?;
                last_sequence = last_sequence.saturating_add(1);
                report.recovered_transactions = report.recovered_transactions.saturating_add(1);
                report.legacy_transactions = report.legacy_transactions.saturating_add(1);
            }
            GraphEvent::Snapshot {
                graph_version,
                data,
            } => {
                let mut restored = decode_json::<GraphAdapter>(&data, offset)?;
                restored.finish_snapshot_restore();
                graph = restored;
                last_sequence = graph_version;
                report.snapshots_loaded = report.snapshots_loaded.saturating_add(1);
                bytes_since_checkpoint = 0;
            }
        }
        let end = reader.stream_position()?;
        bytes_since_checkpoint = bytes_since_checkpoint.saturating_add(end - offset);
        last_committed_offset = end;
    }

    if let Some((_, _, start)) = pending {
        last_committed_offset = start;
        incomplete_tail = true;
    }
    if incomplete_tail || last_committed_offset < file_len {
        let repaired = file_len.saturating_sub(last_committed_offset);
        if repaired > 0 && !options.repair_trailing_bytes {
            return Err(corrupt(
                last_committed_offset,
                format!("incomplete trailing record ({repaired} bytes)"),
            ));
        }
        report.repaired_bytes = repaired;
    }
    report.last_sequence = last_sequence;
    Ok(RecoveredWal {
        graph,
        report,
        committed_len: last_committed_offset,
        bytes_since_checkpoint,
    })
}

fn read_frame<R: Read>(
    reader: &mut R,
    file_len: u64,
    options: &WalOptions,
    offset: u64,
) -> Result<FrameRead, GraphError> {
    let mut header = [0_u8; HEADER_LEN];
    if let Err(error) = reader.read_exact(&mut header) {
        return if error.kind() == std::io::ErrorKind::UnexpectedEof {
            Ok(FrameRead::IncompleteTail)
        } else {
            Err(error.into())
        };
    }
    if &header[..4] != MAGIC {
        return Err(corrupt(offset, "invalid WAL frame magic"));
    }
    if header[4] != FORMAT_VERSION {
        return Err(corrupt(
            offset,
            format!("unsupported WAL version {}", header[4]),
        ));
    }
    let kind = header[5];
    let flags = header[6];
    if flags & !FLAG_COMPRESSED != 0 {
        return Err(corrupt(offset, format!("unknown WAL flags {flags:#x}")));
    }
    let sequence = u64::from_le_bytes(header[8..16].try_into().expect("fixed header slice"));
    let payload_len =
        u32::from_le_bytes(header[16..20].try_into().expect("fixed header slice")) as usize;
    let stored_crc = u32::from_le_bytes(header[20..24].try_into().expect("fixed header slice"));
    let max_stored = options
        .max_snapshot_bytes
        .max(options.max_record_bytes)
        .saturating_add(4);
    if payload_len > max_stored {
        return Err(GraphError::WalRecordTooLarge {
            size: payload_len,
            max: max_stored,
        });
    }
    let end_offset = offset
        .saturating_add(HEADER_LEN as u64)
        .saturating_add(payload_len as u64);
    if end_offset > file_len {
        return Ok(FrameRead::IncompleteTail);
    }
    let mut stored = vec![0_u8; payload_len];
    reader.read_exact(&mut stored)?;
    let mut hasher = Hasher::new();
    hasher.update(&header[..20]);
    hasher.update(&stored);
    if hasher.finalize() != stored_crc {
        return Err(corrupt(offset, "WAL frame checksum mismatch"));
    }
    let max_decoded = if kind == KIND_SNAPSHOT {
        options.max_snapshot_bytes
    } else {
        options.max_record_bytes
    };
    let stored_payload_bytes = stored.len() as u64;
    let payload = if flags & FLAG_COMPRESSED != 0 {
        if stored.len() < 4 {
            return Err(corrupt(offset, "compressed payload has no size prefix"));
        }
        let decoded_len = u32::from_le_bytes(stored[..4].try_into().expect("size prefix")) as usize;
        if decoded_len > max_decoded {
            return Err(GraphError::WalRecordTooLarge {
                size: decoded_len,
                max: max_decoded,
            });
        }
        decompress_size_prepended(&stored)
            .map_err(|error| corrupt(offset, format!("invalid LZ4 payload: {error}")))?
    } else {
        if stored.len() > max_decoded {
            return Err(GraphError::WalRecordTooLarge {
                size: stored.len(),
                max: max_decoded,
            });
        }
        stored
    };
    Ok(FrameRead::Complete(DecodedFrame {
        kind,
        sequence,
        payload,
        stored_payload_bytes,
        end_offset,
    }))
}

fn write_frame<W: Write>(
    writer: &mut W,
    kind: u8,
    sequence: u64,
    raw: &[u8],
    options: &WalOptions,
) -> Result<(u64, u64), GraphError> {
    if raw.len() > u32::MAX as usize {
        return Err(GraphError::WalRecordTooLarge {
            size: raw.len(),
            max: u32::MAX as usize,
        });
    }
    let mut flags = 0_u8;
    let compressed;
    let payload = if !raw.is_empty() && raw.len() >= options.compression_threshold_bytes {
        compressed = compress_prepend_size(raw);
        if compressed.len() < raw.len() {
            flags |= FLAG_COMPRESSED;
            compressed.as_slice()
        } else {
            raw
        }
    } else {
        raw
    };
    let payload_len = u32::try_from(payload.len()).map_err(|_| GraphError::WalRecordTooLarge {
        size: payload.len(),
        max: u32::MAX as usize,
    })?;
    let mut header = [0_u8; HEADER_LEN];
    header[..4].copy_from_slice(MAGIC);
    header[4] = FORMAT_VERSION;
    header[5] = kind;
    header[6] = flags;
    header[8..16].copy_from_slice(&sequence.to_le_bytes());
    header[16..20].copy_from_slice(&payload_len.to_le_bytes());
    let mut hasher = Hasher::new();
    hasher.update(&header[..20]);
    hasher.update(payload);
    header[20..24].copy_from_slice(&hasher.finalize().to_le_bytes());
    writer.write_all(&header)?;
    writer.write_all(payload)?;
    Ok((payload.len() as u64, raw.len() as u64))
}

fn decode_json<T: DeserializeOwned>(bytes: &[u8], offset: u64) -> Result<T, GraphError> {
    serde_json::from_slice(bytes)
        .map_err(|error| corrupt(offset, format!("invalid JSON payload: {error}")))
}

fn encode_json_bounded<T: Serialize>(
    value: &T,
    max: usize,
    limit: PayloadLimit,
) -> Result<Vec<u8>, GraphError> {
    let mut writer = BoundedBuffer::new(max);
    match serde_json::to_writer(&mut writer, value) {
        Ok(()) => Ok(writer.bytes),
        Err(_) if writer.exceeded => {
            let size = writer.attempted_size.max(max.saturating_add(1));
            match limit {
                PayloadLimit::Record => Err(GraphError::WalRecordTooLarge { size, max }),
                PayloadLimit::Snapshot => Err(GraphError::SnapshotTooLarge { size, max }),
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn sync_file(file: &mut File, durability: Durability) -> Result<(), GraphError> {
    match durability {
        Durability::SyncData => file.sync_data()?,
        Durability::SyncAll => file.sync_all()?,
        Durability::Relaxed => file.flush()?,
    }
    Ok(())
}

fn open_active(path: &Path) -> Result<File, GraphError> {
    Ok(OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?)
}

fn suffix_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn corrupt(offset: u64, reason: impl Into<String>) -> GraphError {
    GraphError::WalCorrupt {
        offset,
        reason: reason.into(),
    }
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<(), GraphError> {
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<(), GraphError> {
    Ok(())
}
