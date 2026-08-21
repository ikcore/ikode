pub mod livec_graph;
pub mod livec_string_interner;

pub mod livec_adapter;
pub use livec_adapter::{GraphAdapter, GraphEvent, GraphTxn};

pub use livec_graph::LivecGraph;
pub use livec_string_interner::StringInterner;

pub mod livec_types;
pub use livec_types::{EdgeId, GSize, LabelId, NodeId, PropKeyId, StrId};

pub mod livec_graph_service;
pub use livec_graph_service::{CheckpointStats, GraphService};

pub mod livec_values;
pub use livec_values::Value as LValue;

pub mod livec_event_logger;
pub use livec_event_logger::{Durability, EventLogger, RecoveryReport, WalOptions, WalStats};
use thiserror::Error;

pub mod livec_crypto;

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("txn violation: {0}")]
    AlreadyApplied(String),
    #[error("entity already exists: {0}")]
    AlreadyExists(String),
    #[error("entity not found: {0}")]
    NotFound(String),
    #[error("index required/not found: {0}")]
    MissingIndex(String),
    #[error("unique index violation on {0}")]
    UniqueViolation(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("WAL is already open by another process: {0}")]
    WalLocked(String),
    #[error("WAL corruption at byte {offset}: {reason}")]
    WalCorrupt { offset: u64, reason: String },
    #[error("WAL payload is {size} bytes; configured maximum is {max} bytes")]
    WalRecordTooLarge { size: usize, max: usize },
    #[error("graph snapshot is {size} bytes; configured maximum is {max} bytes")]
    SnapshotTooLarge { size: usize, max: usize },
    #[error("WAL recovery failed after a transaction error: {0}")]
    RecoveryAfterFailure(String),
}
