use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{FileOperation, FileSystemError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferKind {
    Copy,
    Move,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferRequest {
    pub kind: TransferKind,
    pub sources: Vec<PathBuf>,
    pub directory: PathBuf,
}

impl TransferRequest {
    pub fn from_operation(operation: &FileOperation) -> Option<Self> {
        let (kind, sources, directory) = match operation {
            FileOperation::CopyInto { sources, directory } => {
                (TransferKind::Copy, sources, directory)
            }
            FileOperation::MoveInto { sources, directory } => {
                (TransferKind::Move, sources, directory)
            }
            _ => return None,
        };
        Some(Self {
            kind,
            sources: sources.clone(),
            directory: directory.clone(),
        })
    }

    pub fn operation_for(&self, sources: Vec<PathBuf>) -> FileOperation {
        match self.kind {
            TransferKind::Copy => FileOperation::CopyInto {
                sources,
                directory: self.directory.clone(),
            },
            TransferKind::Move => FileOperation::MoveInto {
                sources,
                directory: self.directory.clone(),
            },
        }
    }
}

#[derive(Clone, Default)]
pub struct TransferCancellation(Arc<AtomicBool>);

impl TransferCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransferPhase {
    #[default]
    Preparing,
    Transferring,
    Finishing,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransferProgress {
    pub phase: TransferPhase,
    pub current_path: Option<PathBuf>,
    pub transferred_bytes: u64,
    pub total_bytes: Option<u64>,
    pub completed_items: usize,
    pub total_items: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferredItem {
    pub source: PathBuf,
    pub destination: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferFailure {
    pub source: PathBuf,
    pub error: FileSystemError,
    /// False when retrying could duplicate a cross-device move whose copy
    /// succeeded but whose source could not be completely removed.
    pub retryable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransferReport {
    pub completed: Vec<TransferredItem>,
    pub failures: Vec<TransferFailure>,
    pub remaining: Vec<PathBuf>,
    pub cancelled: bool,
}

impl TransferReport {
    pub fn retry_sources(&self) -> Vec<PathBuf> {
        self.failures
            .iter()
            .filter(|failure| failure.retryable)
            .map(|failure| failure.source.clone())
            .chain(self.remaining.iter().cloned())
            .collect()
    }
}
