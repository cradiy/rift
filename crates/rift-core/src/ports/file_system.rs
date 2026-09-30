use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    path::{Path, PathBuf},
};

use super::{
    TransferCancellation, TransferFailure, TransferPhase, TransferProgress, TransferReport,
    TransferRequest, TransferredItem,
};
use crate::domain::Entry;

pub trait FileSystem: Send + Sync + 'static {
    fn read_directory(&self, path: &Path) -> Result<Vec<Entry>, FileSystemError>;

    fn count_directory_items(
        &self,
        path: &Path,
        include_hidden: bool,
    ) -> Result<usize, FileSystemError>;

    fn perform(&self, operation: FileOperation) -> Result<FileOperationResult, FileSystemError>;

    /// Adapters may override this to report byte progress and cancel within a
    /// file. The default implementation cancels between top-level items.
    fn transfer(
        &self,
        request: TransferRequest,
        cancel: &TransferCancellation,
        progress: &mut dyn FnMut(TransferProgress),
    ) -> TransferReport {
        let mut report = TransferReport::default();
        for (index, source) in request.sources.iter().enumerate() {
            if cancel.is_cancelled() {
                report.cancelled = true;
                report.remaining = request.sources[index..].to_vec();
                break;
            }
            progress(TransferProgress {
                phase: TransferPhase::Transferring,
                current_path: Some(source.clone()),
                completed_items: report.completed.len(),
                total_items: request.sources.len(),
                ..Default::default()
            });
            match self.perform(request.operation_for(vec![source.clone()])) {
                Ok(result) => {
                    report
                        .completed
                        .extend(result.affected_paths.into_iter().map(|destination| {
                            TransferredItem {
                                source: source.clone(),
                                destination,
                            }
                        }))
                }
                Err(error) => report.failures.push(TransferFailure {
                    source: source.clone(),
                    error,
                    retryable: true,
                }),
            }
        }
        report
    }

    fn watch_directory(&self, path: &Path) -> Result<Box<dyn DirectoryWatch>, FileSystemError> {
        Err(FileSystemError::new(
            FileSystemOperation::WatchDirectory,
            path,
            "directory watching is not supported",
        ))
    }
}

/// A lightweight change signal owned by the filesystem adapter.
///
/// Implementations update the monotonically increasing revision from their
/// platform callback. Consumers can sample it without blocking a UI or worker
/// thread.
pub trait DirectoryWatch: 'static {
    fn revision(&self) -> u64;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileOperation {
    Rename {
        source: PathBuf,
        destination: PathBuf,
    },
    CreateDirectory {
        path: PathBuf,
    },
    CreateFile {
        path: PathBuf,
    },
    CopyInto {
        sources: Vec<PathBuf>,
        directory: PathBuf,
    },
    MoveInto {
        sources: Vec<PathBuf>,
        directory: PathBuf,
    },
    Trash {
        paths: Vec<PathBuf>,
    },
    PurgeTrash {
        paths: Vec<PathBuf>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileOperationResult {
    pub affected_paths: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSystemOperation {
    ReadDirectory,
    WatchDirectory,
    CountDirectoryItems,
    ReadMetadata,
    Rename,
    CreateDirectory,
    CreateFile,
    Copy,
    Move,
    Trash,
    PurgeTrash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSystemError {
    pub operation: FileSystemOperation,
    pub path: PathBuf,
    pub message: String,
}

impl FileSystemError {
    pub fn new(
        operation: FileSystemOperation,
        path: impl Into<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            path: path.into(),
            message: message.into(),
        }
    }
}

impl Display for FileSystemError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} failed for {}: {}",
            self.operation,
            self.path.display(),
            self.message
        )
    }
}

impl Error for FileSystemError {}
