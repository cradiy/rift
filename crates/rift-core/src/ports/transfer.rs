use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

use super::{FileOperation, FileSystemError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferKind {
    Copy,
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    KeepBoth,
    Skip,
    Replace,
}

/// Metadata captured at the prompt, used to reject a stale replacement choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictEntry {
    pub byte_len: u64,
    pub modified: Option<SystemTime>,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub device: u64,
    pub inode: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferConflict {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub incoming: ConflictEntry,
    pub existing: ConflictEntry,
    pub replace_allowed: bool,
}

#[derive(Clone, Debug)]
pub struct ConflictDecision {
    pub conflict: TransferConflict,
    pub choice: ConflictChoice,
}

/// A transfer yields at a conflict instead of blocking a worker for UI input.
/// `decision` is one-shot; `policy` applies to subsequent conflicts in this task.
#[derive(Clone, Debug, Default)]
pub struct TransferOptions {
    pub decision: Option<ConflictDecision>,
    pub policy: Option<ConflictChoice>,
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
    pub skipped_items: usize,
    pub total_items: usize,
    /// Entries discovered recursively, including the selected roots.
    pub total_entries: usize,
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
    pub skipped: Vec<PathBuf>,
    pub failures: Vec<TransferFailure>,
    pub remaining: Vec<PathBuf>,
    pub cancelled: bool,
    pub conflict: Option<TransferConflict>,
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
