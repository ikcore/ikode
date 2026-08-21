use crate::livec_adapter::TxnResponse;
use crate::{
    EventLogger, GraphAdapter, GraphError, GraphTxn, LivecGraph, RecoveryReport, WalOptions,
    WalStats,
};
use parking_lot::RwLock;
use std::{path::Path, sync::Arc};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckpointStats {
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub uncompressed_snapshot_bytes: u64,
    pub last_sequence: u64,
}

impl CheckpointStats {
    pub fn reclaimed_bytes(&self) -> u64 {
        self.before_bytes.saturating_sub(self.after_bytes)
    }

    pub fn compression_saved_bytes(&self) -> u64 {
        self.uncompressed_snapshot_bytes
            .saturating_sub(self.after_bytes)
    }
}

pub struct GraphService {
    graph: Arc<RwLock<GraphAdapter>>,
    wal: RwLock<EventLogger>,
    recovery_report: RecoveryReport,
}

impl GraphService {
    /// Open and recover the graph using strict, bounded WAL defaults.
    pub fn new_with_log<P: AsRef<Path>>(wal_path: P) -> Result<Self, GraphError> {
        Self::open_with_options(wal_path, WalOptions::default())
    }

    /// Open, exclusively lock, validate, repair a torn tail when configured, and
    /// recover the graph before returning it to callers.
    pub fn open_with_options<P: AsRef<Path>>(
        wal_path: P,
        options: WalOptions,
    ) -> Result<Self, GraphError> {
        let (wal, graph, recovery_report) = EventLogger::open(wal_path, options)?;
        Ok(Self {
            graph: Arc::new(RwLock::new(graph)),
            wal: RwLock::new(wal),
            recovery_report,
        })
    }

    pub fn recovery_report(&self) -> &RecoveryReport {
        &self.recovery_report
    }

    pub fn wal_stats(&self) -> WalStats {
        self.wal.read().stats()
    }

    /// Write-ahead transaction protocol:
    ///
    /// 1. append a framed transaction record;
    /// 2. apply while readers are excluded;
    /// 3. append a commit marker and durably sync both records;
    /// 4. acknowledge the caller.
    ///
    /// A failed application is rolled back by rebuilding from committed records.
    /// This keeps the fast path linear in the transaction size while preserving
    /// all-or-nothing semantics for complex graph batches.
    pub fn submit_txn(&self, txn: GraphTxn) -> Result<TxnResponse, GraphError> {
        let mut graph = self.graph.write();
        let mut wal = self.wal.write();

        // Bound delta growth before accepting another transaction. Checkpointing
        // here is failure-safe: if it fails, no part of `txn` has been written or
        // applied yet.
        if wal.needs_checkpoint() {
            wal.checkpoint(&graph)?;
        }

        if let Some(key) = &txn.idempotency_key {
            if graph.applied_ids.contains(key) {
                return Err(GraphError::AlreadyApplied(key.to_owned()));
            }
        }

        let sequence = wal.begin(&txn)?;
        let applied =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| graph.apply_txn(&txn)));
        let response = match applied {
            Ok(Ok(response)) => response,
            Ok(Err(apply_error)) => {
                let abort_error = wal.abort_pending().err();
                match wal.recover_current() {
                    Ok((restored, _)) => {
                        *graph = restored;
                        if let Some(abort_error) = abort_error {
                            return Err(GraphError::RecoveryAfterFailure(format!(
                                "transaction failed ({apply_error}); WAL truncation also failed: {abort_error}"
                            )));
                        }
                        return Err(apply_error);
                    }
                    Err(recovery_error) => {
                        return Err(GraphError::RecoveryAfterFailure(format!(
                            "transaction failed ({apply_error}); committed-state recovery failed: {recovery_error}"
                        )));
                    }
                }
            }
            Err(_) => {
                let _ = wal.abort_pending();
                match wal.recover_current() {
                    Ok((restored, _)) => {
                        *graph = restored;
                        return Err(GraphError::Conflict(
                            "transaction panicked while applying and was rolled back".into(),
                        ));
                    }
                    Err(recovery_error) => {
                        return Err(GraphError::RecoveryAfterFailure(format!(
                            "transaction panicked; committed-state recovery failed: {recovery_error}"
                        )));
                    }
                }
            }
        };

        if let Err(commit_error) = wal.commit(sequence) {
            match wal.recover_current() {
                Ok((restored, _)) => {
                    *graph = restored;
                    return Err(commit_error);
                }
                Err(recovery_error) => {
                    return Err(GraphError::RecoveryAfterFailure(format!(
                        "WAL commit failed ({commit_error}); committed-state recovery failed: {recovery_error}"
                    )));
                }
            }
        }
        Ok(response)
    }

    /// Replace WAL history with one compressed snapshot of the exact graph state.
    pub fn checkpoint(&self) -> Result<CheckpointStats, GraphError> {
        let graph = self.graph.write();
        let mut wal = self.wal.write();
        let (before_bytes, after_bytes, uncompressed_snapshot_bytes) = wal.checkpoint(&graph)?;
        Ok(CheckpointStats {
            before_bytes,
            after_bytes,
            uncompressed_snapshot_bytes,
            last_sequence: wal.stats().last_sequence,
        })
    }

    pub fn with_read<F: FnOnce(&LivecGraph)>(&self, f: F) {
        let graph = self.graph.read();
        graph.with_read(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::livec_adapter::{
        EdgeAction, IndexAction, IndexScope, IndexTarget, IndexType, LabelRef, LabelSingle,
        NodeAction, NodeRef, PropKey, Props, Value as GraphValue,
    };
    use crate::{Durability, GraphEvent, LValue};
    use std::fs::OpenOptions;
    use std::io::{Read, Seek, SeekFrom, Write};
    use tempfile::tempdir;

    const TEST_FRAME_HEADER_BYTES: u64 = 24;

    fn label(name: &str) -> LabelRef {
        LabelRef::Single(LabelSingle::Name(name.to_string()))
    }

    fn create_txn(idempotency_key: Option<&str>, name: &str, blob_bytes: usize) -> GraphTxn {
        GraphTxn {
            idempotency_key: idempotency_key.map(str::to_string),
            timestamp_ms: 1,
            nodes: Some(vec![NodeAction::Create {
                label: label("Item"),
                props: Props(vec![
                    (
                        PropKey::Name("name".into()),
                        Some(GraphValue::Str(name.to_string())),
                    ),
                    (
                        PropKey::Name("blob".into()),
                        Some(GraphValue::Bytes(vec![0x5a; blob_bytes])),
                    ),
                ]),
                alias: None,
            }]),
            edges: None,
            indexes: None,
        }
    }

    fn alive_nodes(service: &GraphService) -> usize {
        let mut count = 0;
        service.with_read(|graph| {
            count = graph.nodes.iter().filter(|node| node.alive).count();
        });
        count
    }

    #[test]
    fn committed_transaction_survives_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        {
            let service = GraphService::new_with_log(&path).unwrap();
            service
                .submit_txn(create_txn(Some("txn-1"), "one", 0))
                .unwrap();
            assert_eq!(alive_nodes(&service), 1);
            assert_eq!(service.wal_stats().last_sequence, 1);
        }

        let recovered = GraphService::new_with_log(&path).unwrap();
        assert_eq!(alive_nodes(&recovered), 1);
        assert_eq!(recovered.recovery_report().recovered_transactions, 1);
        assert_eq!(recovered.recovery_report().last_sequence, 1);
    }

    #[test]
    fn failed_multi_action_transaction_is_fully_rolled_back() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        let service = GraphService::new_with_log(&path).unwrap();
        let txn = GraphTxn {
            idempotency_key: Some("bad-batch".into()),
            timestamp_ms: 1,
            nodes: Some(vec![
                NodeAction::Create {
                    label: label("Item"),
                    props: Props(vec![]),
                    alias: None,
                },
                NodeAction::Create {
                    label: LabelRef::Many(vec![]),
                    props: Props(vec![]),
                    alias: None,
                },
            ]),
            edges: None,
            indexes: None,
        };

        assert!(service.submit_txn(txn).is_err());
        assert_eq!(alive_nodes(&service), 0);
        assert_eq!(service.wal_stats().file_bytes, 0);
        drop(service);
        assert_eq!(alive_nodes(&GraphService::new_with_log(&path).unwrap()), 0);
    }

    #[test]
    fn panicking_graph_operation_is_caught_and_rolled_back() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        let service = GraphService::new_with_log(&path).unwrap();
        let txn = GraphTxn {
            idempotency_key: Some("panic-batch".into()),
            timestamp_ms: 1,
            nodes: None,
            edges: Some(vec![EdgeAction::Create {
                from: NodeRef::ById(999),
                to: NodeRef::ById(1000),
                label: label("BROKEN"),
                props: Props(vec![]),
            }]),
            indexes: None,
        };

        assert!(matches!(
            service.submit_txn(txn),
            Err(GraphError::Conflict(message)) if message.contains("panicked")
        ));
        assert_eq!(alive_nodes(&service), 0);
        assert_eq!(service.wal_stats().file_bytes, 0);
    }

    #[test]
    fn incomplete_tail_is_repaired_to_last_commit() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        {
            let service = GraphService::new_with_log(&path).unwrap();
            service
                .submit_txn(create_txn(Some("txn-1"), "one", 0))
                .unwrap();
        }
        let committed_len = std::fs::metadata(&path).unwrap().len();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"IK")
            .unwrap();

        let recovered = GraphService::new_with_log(&path).unwrap();
        assert_eq!(recovered.recovery_report().repaired_bytes, 2);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), committed_len);
        assert_eq!(alive_nodes(&recovered), 1);
    }

    #[test]
    fn tail_repair_can_be_disabled() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        {
            let service = GraphService::new_with_log(&path).unwrap();
            service
                .submit_txn(create_txn(Some("txn-1"), "one", 0))
                .unwrap();
        }
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"IK")
            .unwrap();
        let options = WalOptions {
            repair_trailing_bytes: false,
            ..WalOptions::default()
        };
        assert!(matches!(
            GraphService::open_with_options(&path, options),
            Err(GraphError::WalCorrupt { .. })
        ));
    }

    #[test]
    fn complete_but_uncommitted_transaction_is_discarded() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        {
            let (mut wal, _, _) = EventLogger::open(&path, WalOptions::default()).unwrap();
            wal.begin(&create_txn(Some("pending"), "pending", 0))
                .unwrap();
        }
        assert!(std::fs::metadata(&path).unwrap().len() > 0);

        let recovered = GraphService::new_with_log(&path).unwrap();
        assert!(recovered.recovery_report().repaired_bytes > 0);
        assert_eq!(alive_nodes(&recovered), 0);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    }

    #[test]
    fn checksum_corruption_before_the_tail_is_fatal() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        {
            let service = GraphService::new_with_log(&path).unwrap();
            service
                .submit_txn(create_txn(Some("txn-1"), "one", 0))
                .unwrap();
            service
                .submit_txn(create_txn(Some("txn-2"), "two", 0))
                .unwrap();
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.seek(SeekFrom::Start(TEST_FRAME_HEADER_BYTES)).unwrap();
        let mut byte = [0_u8; 1];
        file.read_exact(&mut byte).unwrap();
        byte[0] ^= 0xff;
        file.seek(SeekFrom::Start(TEST_FRAME_HEADER_BYTES)).unwrap();
        file.write_all(&byte).unwrap();
        file.sync_all().unwrap();
        drop(file);

        assert!(matches!(
            GraphService::new_with_log(&path),
            Err(GraphError::WalCorrupt { .. })
        ));
    }

    #[test]
    fn checksum_corruption_in_final_frame_is_not_silently_repaired() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        {
            let service = GraphService::new_with_log(&path).unwrap();
            service
                .submit_txn(create_txn(Some("txn-1"), "one", 0))
                .unwrap();
        }
        let len = std::fs::metadata(&path).unwrap().len();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.seek(SeekFrom::Start(len - 1)).unwrap();
        let mut byte = [0_u8; 1];
        file.read_exact(&mut byte).unwrap();
        byte[0] ^= 0xff;
        file.seek(SeekFrom::Start(len - 1)).unwrap();
        file.write_all(&byte).unwrap();
        file.sync_all().unwrap();
        drop(file);

        assert!(matches!(
            GraphService::new_with_log(&path),
            Err(GraphError::WalCorrupt { .. })
        ));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), len);
    }

    #[test]
    fn checkpoint_preserves_exact_state_and_idempotency() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        let sequence;
        {
            let service = GraphService::new_with_log(&path).unwrap();
            let mut txn = create_txn(Some("txn-1"), "one", 4096);
            txn.indexes = Some(vec![IndexAction::CreateIndex {
                scope: IndexScope::Global,
                target: IndexTarget::Node(PropKey::Name("name".into())),
                idx_type: IndexType::Unique,
            }]);
            service.submit_txn(txn).unwrap();
            let checkpoint = service.checkpoint().unwrap();
            sequence = checkpoint.last_sequence;
            assert!(checkpoint.uncompressed_snapshot_bytes > 0);
            assert_eq!(alive_nodes(&service), 1);
        }

        let recovered = GraphService::new_with_log(&path).unwrap();
        assert_eq!(recovered.recovery_report().snapshots_loaded, 1);
        assert_eq!(recovered.recovery_report().last_sequence, sequence);
        assert_eq!(alive_nodes(&recovered), 1);
        recovered.with_read(|graph| {
            let label = graph.label_names.get_id("Item").unwrap();
            let key = graph.prop_keys.get_id("name").unwrap();
            let value = graph.strings.get_id("one").unwrap();
            assert_eq!(
                graph.find_nodes_eq(label, key, &LValue::Str(value)),
                vec![0]
            );
        });
        assert!(matches!(
            recovered.submit_txn(create_txn(Some("txn-1"), "duplicate", 0)),
            Err(GraphError::AlreadyApplied(_))
        ));
    }

    #[test]
    fn compression_and_byte_counters_reflect_large_payloads() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        let options = WalOptions {
            durability: Durability::Relaxed,
            compression_threshold_bytes: 16,
            ..WalOptions::default()
        };
        let service = GraphService::open_with_options(&path, options).unwrap();
        service
            .submit_txn(create_txn(Some("large"), "large", 128 * 1024))
            .unwrap();
        let stats = service.wal_stats();
        assert!(stats.original_payload_bytes > stats.stored_payload_bytes);
        assert!(stats.compression_saved_bytes() > 100_000);
        assert!(stats.file_bytes < stats.original_payload_bytes);
    }

    #[test]
    fn record_size_limit_rejects_before_mutation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        let options = WalOptions {
            max_record_bytes: 128,
            ..WalOptions::default()
        };
        let service = GraphService::open_with_options(&path, options).unwrap();
        assert!(matches!(
            service.submit_txn(create_txn(Some("too-large"), "large", 1024)),
            Err(GraphError::WalRecordTooLarge { .. })
        ));
        assert_eq!(alive_nodes(&service), 0);
        assert_eq!(service.wal_stats().file_bytes, 0);
    }

    #[test]
    fn snapshot_size_limit_leaves_committed_wal_intact() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        let options = WalOptions {
            max_snapshot_bytes: 128,
            ..WalOptions::default()
        };
        let service = GraphService::open_with_options(&path, options).unwrap();
        service
            .submit_txn(create_txn(Some("txn-1"), "one", 1024))
            .unwrap();
        let before = service.wal_stats().file_bytes;
        assert!(matches!(
            service.checkpoint(),
            Err(GraphError::SnapshotTooLarge { .. })
        ));
        assert_eq!(service.wal_stats().file_bytes, before);
        assert_eq!(alive_nodes(&service), 1);
        drop(service);
        assert_eq!(alive_nodes(&GraphService::new_with_log(&path).unwrap()), 1);
    }

    #[test]
    fn size_threshold_checkpoints_before_following_transaction() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        let options = WalOptions {
            checkpoint_after_bytes: 1,
            ..WalOptions::default()
        };
        {
            let service = GraphService::open_with_options(&path, options).unwrap();
            service
                .submit_txn(create_txn(Some("txn-1"), "one", 0))
                .unwrap();
            service
                .submit_txn(create_txn(Some("txn-2"), "two", 0))
                .unwrap();
            assert_eq!(service.wal_stats().snapshots, 1);
        }
        let recovered = GraphService::new_with_log(&path).unwrap();
        assert_eq!(recovered.recovery_report().snapshots_loaded, 1);
        assert_eq!(recovered.recovery_report().recovered_transactions, 1);
        assert_eq!(alive_nodes(&recovered), 2);
    }

    #[test]
    fn legacy_json_prefix_can_be_extended_with_framed_records() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        let legacy = GraphEvent::Txn(create_txn(Some("legacy"), "legacy", 0));
        let mut bytes = serde_json::to_vec(&legacy).unwrap();
        bytes.push(b'\n');
        std::fs::write(&path, bytes).unwrap();

        {
            let service = GraphService::new_with_log(&path).unwrap();
            assert_eq!(service.recovery_report().legacy_transactions, 1);
            service
                .submit_txn(create_txn(Some("framed"), "framed", 0))
                .unwrap();
        }
        let recovered = GraphService::new_with_log(&path).unwrap();
        assert_eq!(recovered.recovery_report().legacy_transactions, 1);
        assert_eq!(alive_nodes(&recovered), 2);
    }

    #[test]
    fn exclusive_lock_rejects_a_second_writer() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        let first = GraphService::new_with_log(&path).unwrap();
        assert!(matches!(
            GraphService::new_with_log(&path),
            Err(GraphError::WalLocked(_))
        ));
        drop(first);
        assert!(GraphService::new_with_log(&path).is_ok());
    }

    #[test]
    fn interrupted_checkpoint_restores_previous_generation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.log");
        {
            let service = GraphService::new_with_log(&path).unwrap();
            service
                .submit_txn(create_txn(Some("txn-1"), "one", 0))
                .unwrap();
        }
        let backup = dir.path().join("graph.log.previous");
        std::fs::rename(&path, &backup).unwrap();

        let recovered = GraphService::new_with_log(&path).unwrap();
        assert_eq!(alive_nodes(&recovered), 1);
        assert!(path.exists());
    }
}
