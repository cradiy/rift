use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    path::{Path, PathBuf},
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
