use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use rift_core::ports::{
    FileSystemError, FileSystemOperation, TransferCancellation, TransferFailure, TransferKind,
    TransferPhase, TransferProgress, TransferReport, TransferRequest, TransferredItem,
};

use crate::local::{
    available_copy_destination, ensure_directory, ensure_not_inside_source, remove_entry,
};

const COPY_CHUNK_SIZE: usize = 1024 * 1024;

enum TransferError {
    Cancelled,
    Failed {
        error: FileSystemError,
        retryable: bool,
    },
}

struct Runner<'a> {
    cancel: &'a TransferCancellation,
    publish: &'a mut dyn FnMut(TransferProgress),
    progress: TransferProgress,
    operation: FileSystemOperation,
    buffer: Vec<u8>,
}

pub(super) fn run(
    request: TransferRequest,
    cancel: &TransferCancellation,
    publish: &mut dyn FnMut(TransferProgress),
) -> TransferReport {
    let operation = match request.kind {
        TransferKind::Copy => FileSystemOperation::Copy,
        TransferKind::Move => FileSystemOperation::Move,
    };
    let mut runner = Runner {
        cancel,
        publish,
        progress: TransferProgress {
            total_items: request.sources.len(),
            ..Default::default()
        },
        operation,
        buffer: vec![0; COPY_CHUNK_SIZE],
    };
    runner.publish();
    let mut report = TransferReport::default();
    if let Err(error) = ensure_directory(&request.directory, operation) {
        for source in &request.sources {
            report.failures.push(TransferFailure {
                source: source.clone(),
                error: error.clone(),
                retryable: true,
            });
        }
        return report;
    }

    // Prepare independently: one unreadable source must not prevent the other
    // selected items from transferring. No destination is created in this phase.
    let mut prepared = Vec::new();
    let mut total = 0u64;
    for (index, source) in request.sources.iter().enumerate() {
        let size = runner.prepare(source, &request.directory, request.kind);
        match size {
            Ok(bytes) => {
                total = total.saturating_add(bytes);
                prepared.push((source.clone(), bytes));
            }
            Err(TransferError::Cancelled) => {
                report.cancelled = true;
                report
                    .remaining
                    .extend(prepared.into_iter().map(|(source, _)| source));
                report
                    .remaining
                    .extend_from_slice(&request.sources[index..]);
                return report;
            }
            Err(TransferError::Failed { error, retryable }) => {
                report.failures.push(TransferFailure {
                    source: source.clone(),
                    error,
                    retryable,
                })
            }
        }
    }
    runner.progress.total_bytes = Some(total);
    runner.progress.phase = TransferPhase::Transferring;
    runner.publish();

    for (index, (source, bytes)) in prepared.iter().enumerate() {
        let before = runner.progress.transferred_bytes;
        let result = runner.transfer_item(source, &request.directory, request.kind);
        match result {
            Ok(destination) => {
                runner.progress.transferred_bytes = before.saturating_add(*bytes);
                report.completed.push(TransferredItem {
                    source: source.clone(),
                    destination,
                });
                runner.progress.completed_items = report.completed.len();
            }
            Err(TransferError::Cancelled) => {
                runner.progress.transferred_bytes = before;
                runner.publish();
                report.cancelled = true;
                report
                    .remaining
                    .extend(prepared[index..].iter().map(|(source, _)| source.clone()));
                break;
            }
            Err(TransferError::Failed { error, retryable }) => {
                runner.progress.transferred_bytes = before;
                report.failures.push(TransferFailure {
                    source: source.clone(),
                    error,
                    retryable,
                });
            }
        }
        runner.publish();
    }
    report
}

impl Runner<'_> {
    fn publish(&mut self) {
        (self.publish)(self.progress.clone());
    }

    fn check_cancel(&self) -> Result<(), TransferError> {
        if self.cancel.is_cancelled() {
            Err(TransferError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn error(&self, path: &Path, error: impl ToString) -> TransferError {
        TransferError::Failed {
            error: FileSystemError::new(self.operation, path, error.to_string()),
            retryable: true,
        }
    }

    fn prepare(
        &mut self,
        source: &Path,
        directory: &Path,
        kind: TransferKind,
    ) -> Result<u64, TransferError> {
        self.check_cancel()?;
        ensure_not_inside_source(source, directory, self.operation).map_err(|error| {
            TransferError::Failed {
                error,
                retryable: true,
            }
        })?;
        let metadata = fs::symlink_metadata(source).map_err(|error| self.error(source, error))?;
        if metadata.is_dir() {
            let canonical_source =
                fs::canonicalize(source).map_err(|error| self.error(source, error))?;
            let canonical_target =
                fs::canonicalize(directory).map_err(|error| self.error(directory, error))?;
            if canonical_target.starts_with(&canonical_source) {
                return Err(self.error(source, "A folder cannot be transferred into itself"));
            }
        }
        self.measure(source, kind)
    }

    fn measure(&mut self, source: &Path, kind: TransferKind) -> Result<u64, TransferError> {
        self.check_cancel()?;
        self.progress.current_path = Some(source.to_path_buf());
        self.publish();
        let metadata = fs::symlink_metadata(source).map_err(|error| self.error(source, error))?;
        if metadata.is_file() {
            return Ok(metadata.len());
        }
        if metadata.file_type().is_symlink() && kind == TransferKind::Move {
            return Ok(0);
        }
        if !metadata.is_dir() {
            return Err(self.error(source, "This item type cannot be copied"));
        }
        let mut bytes = 0u64;
        for entry in fs::read_dir(source).map_err(|error| self.error(source, error))? {
            let entry = entry.map_err(|error| self.error(source, error))?;
            bytes = bytes.saturating_add(self.measure(&entry.path(), kind)?);
        }
        Ok(bytes)
    }

    fn transfer_item(
        &mut self,
        source: &Path,
        directory: &Path,
        kind: TransferKind,
    ) -> Result<PathBuf, TransferError> {
        self.check_cancel()?;
        self.progress.phase = TransferPhase::Transferring;
        self.progress.current_path = Some(source.to_path_buf());
        self.publish();
        let destination = match kind {
            TransferKind::Copy => {
                available_copy_destination(source, directory).map_err(|error| {
                    TransferError::Failed {
                        error,
                        retryable: true,
                    }
                })?
            }
            TransferKind::Move => directory.join(
                source
                    .file_name()
                    .ok_or_else(|| self.error(source, "The source has no file name"))?,
            ),
        };
        if kind == TransferKind::Move {
            match rename_without_replacing(source, &destination) {
                Ok(()) => return Ok(destination),
                Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {}
                Err(error) => return Err(self.error(source, error)),
            }
        }
        self.copy_owned(source, &destination)?;
        if kind == TransferKind::Move {
            if self.cancel.is_cancelled() {
                return match remove_entry(&destination) {
                    Ok(()) => Err(TransferError::Cancelled),
                    Err(error) => Err(self.cleanup_error(&destination, error)),
                };
            }
            // Once source removal starts it finishes as one commit step. A
            // cancellation arriving here applies to the next top-level item.
            self.progress.phase = TransferPhase::Finishing;
            self.publish();
            remove_entry(source).map_err(|error| TransferError::Failed {
                error: FileSystemError::new(self.operation, source,
                    format!("Copied to {} but could not fully remove the source: {error}. Check both locations before retrying.", destination.display())),
                retryable: false,
            })?;
        }
        Ok(destination)
    }

    fn cleanup_error(&self, destination: &Path, error: std::io::Error) -> TransferError {
        TransferError::Failed {
            error: FileSystemError::new(
                self.operation,
                destination,
                format!(
                    "Could not remove the incomplete copy: {error}. Check this destination before retrying."
                ),
            ),
            retryable: false,
        }
    }

    fn copy_owned(&mut self, source: &Path, destination: &Path) -> Result<(), TransferError> {
        let mut owned = false;
        let result = self.copy_entry(source, destination, &mut owned);
        if result.is_err() && owned {
            remove_entry(destination).map_err(|error| self.cleanup_error(destination, error))?;
        }
        result
    }

    fn copy_entry(
        &mut self,
        source: &Path,
        destination: &Path,
        owned: &mut bool,
    ) -> Result<(), TransferError> {
        self.check_cancel()?;
        self.progress.current_path = Some(source.to_path_buf());
        self.publish();
        let metadata = fs::symlink_metadata(source).map_err(|error| self.error(source, error))?;
        if metadata.is_dir() {
            fs::create_dir(destination).map_err(|error| self.error(destination, error))?;
            *owned = true;
            for entry in fs::read_dir(source).map_err(|error| self.error(source, error))? {
                let entry = entry.map_err(|error| self.error(source, error))?;
                self.copy_entry(
                    &entry.path(),
                    &destination.join(entry.file_name()),
                    &mut false,
                )?;
            }
        } else if metadata.is_file() {
            let mut input = File::open(source).map_err(|error| self.error(source, error))?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(|error| self.error(destination, error))?;
            *owned = true;
            loop {
                self.check_cancel()?;
                let count = input
                    .read(&mut self.buffer)
                    .map_err(|error| self.error(source, error))?;
                if count == 0 {
                    break;
                }
                output
                    .write_all(&self.buffer[..count])
                    .map_err(|error| self.error(destination, error))?;
                self.progress.transferred_bytes =
                    self.progress.transferred_bytes.saturating_add(count as u64);
                self.publish();
            }
        } else {
            return Err(self.error(source, "This item type cannot be copied"));
        }
        fs::set_permissions(destination, metadata.permissions())
            .map_err(|error| self.error(destination, error))?;
        self.check_cancel()
    }
}

#[cfg(target_os = "linux")]
fn rename_without_replacing(source: &Path, destination: &Path) -> std::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(Into::into)
}

#[cfg(not(target_os = "linux"))]
fn rename_without_replacing(source: &Path, destination: &Path) -> std::io::Result<()> {
    if destination.symlink_metadata().is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Destination already exists",
        ));
    }
    fs::rename(source, destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_copy_reports_bytes_and_keeps_contents() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("first.txt"), b"first").unwrap();
        fs::write(source.join("nested/second.txt"), b"second").unwrap();
        let mut updates = Vec::new();
        let report = run(
            TransferRequest {
                kind: TransferKind::Copy,
                sources: vec![source.clone()],
                directory: target.clone(),
            },
            &TransferCancellation::default(),
            &mut |progress| updates.push(progress),
        );

        assert!(report.failures.is_empty());
        assert_eq!(report.completed.len(), 1);
        assert_eq!(
            fs::read(target.join("source/nested/second.txt")).unwrap(),
            b"second"
        );
        let last = updates.last().unwrap();
        assert_eq!(last.total_bytes, Some(11));
        assert_eq!(last.transferred_bytes, 11);
        assert_eq!(last.completed_items, 1);
        assert!(updates.iter().any(|update| update.current_path.as_deref()
            == Some(source.join("nested/second.txt").as_path())));
    }

    #[test]
    fn cancelling_within_a_file_removes_only_the_incomplete_copy() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.txt");
        let large = root.path().join("large.bin");
        let last = root.path().join("last.txt");
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(&first, "keep").unwrap();
        fs::write(&large, vec![7; COPY_CHUNK_SIZE * 3]).unwrap();
        fs::write(&last, "later").unwrap();
        let cancel = TransferCancellation::default();
        let report = run(
            TransferRequest {
                kind: TransferKind::Copy,
                sources: vec![first.clone(), large.clone(), last.clone()],
                directory: target.clone(),
            },
            &cancel,
            &mut |progress| {
                if progress.current_path.as_ref() == Some(&large) && progress.transferred_bytes > 4
                {
                    cancel.cancel();
                }
            },
        );

        assert!(report.cancelled);
        assert_eq!(report.completed.len(), 1);
        assert_eq!(report.retry_sources(), [large.clone(), last]);
        assert_eq!(fs::read(target.join("first.txt")).unwrap(), b"keep");
        assert!(!target.join("large.bin").exists());
        assert!(!target.join("last.txt").exists());
        assert_eq!(
            fs::metadata(large).unwrap().len(),
            (COPY_CHUNK_SIZE * 3) as u64
        );
    }

    #[test]
    fn cancellation_during_preparation_creates_no_destination() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("file.txt"), "data").unwrap();
        let cancel = TransferCancellation::default();
        let report = run(
            TransferRequest {
                kind: TransferKind::Copy,
                sources: vec![source.clone()],
                directory: target.clone(),
            },
            &cancel,
            &mut |progress| {
                if progress.current_path.is_some() {
                    cancel.cancel();
                }
            },
        );
        assert!(report.cancelled);
        assert_eq!(report.retry_sources(), [source]);
        assert_eq!(fs::read_dir(target).unwrap().count(), 0);
    }

    #[test]
    fn batch_failures_do_not_stop_other_items_and_retry_does_not_duplicate_successes() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.txt");
        let missing = root.path().join("missing.txt");
        let last = root.path().join("last.txt");
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(&first, "first").unwrap();
        fs::write(&last, "last").unwrap();
        let mut request = TransferRequest {
            kind: TransferKind::Copy,
            sources: vec![first, missing.clone(), last],
            directory: target.clone(),
        };
        let report = run(
            request.clone(),
            &TransferCancellation::default(),
            &mut |_| {},
        );
        assert_eq!(report.completed.len(), 2);
        assert_eq!(report.failures[0].source, missing);
        request.sources = report.retry_sources();
        fs::write(&missing, "now exists").unwrap();
        let retry = run(request, &TransferCancellation::default(), &mut |_| {});
        assert_eq!(retry.completed.len(), 1);
        assert!(retry.failures.is_empty());
        assert_eq!(fs::read_dir(target).unwrap().count(), 3);
    }

    #[test]
    fn moves_never_replace_an_existing_target_and_continue_other_items() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("existing.txt");
        let another = root.path().join("another.txt");
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(&source, "source").unwrap();
        fs::write(&another, "another").unwrap();
        fs::write(target.join("existing.txt"), "original").unwrap();
        let report = run(
            TransferRequest {
                kind: TransferKind::Move,
                sources: vec![source.clone(), another.clone()],
                directory: target.clone(),
            },
            &TransferCancellation::default(),
            &mut |_| {},
        );
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.completed.len(), 1);
        assert_eq!(fs::read(target.join("existing.txt")).unwrap(), b"original");
        assert!(source.exists());
        assert!(!another.exists());
    }

    #[test]
    fn a_destination_created_by_someone_else_is_never_cleaned_up() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("file.txt");
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(&source, "source").unwrap();
        let mut transferring_updates = 0;
        let report = run(
            TransferRequest {
                kind: TransferKind::Copy,
                sources: vec![source],
                directory: target.clone(),
            },
            &TransferCancellation::default(),
            &mut |progress| {
                if progress.phase == TransferPhase::Transferring {
                    transferring_updates += 1;
                    // after choosing the destination, but before create_new
                    if transferring_updates == 3 {
                        fs::write(target.join("file.txt"), "someone else's file").unwrap();
                    }
                }
            },
        );
        assert_eq!(report.failures.len(), 1);
        assert!(report.completed.is_empty());
        assert_eq!(
            fs::read(target.join("file.txt")).unwrap(),
            b"someone else's file"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_device_move_cancels_safely_and_can_be_retried() {
        use std::os::unix::fs::MetadataExt;
        let root = tempfile::tempdir().unwrap();
        let target = tempfile::Builder::new()
            .prefix("rift-transfer-test-")
            .tempdir_in("/dev/shm")
            .unwrap();
        assert_ne!(
            fs::metadata(root.path()).unwrap().dev(),
            fs::metadata(target.path()).unwrap().dev()
        );
        let source = root.path().join("large.bin");
        fs::write(&source, vec![5; COPY_CHUNK_SIZE * 3]).unwrap();
        let request = TransferRequest {
            kind: TransferKind::Move,
            sources: vec![source.clone()],
            directory: target.path().to_path_buf(),
        };
        let cancel = TransferCancellation::default();
        let report = run(request.clone(), &cancel, &mut |progress| {
            if progress.transferred_bytes > 0 {
                cancel.cancel();
            }
        });
        assert!(report.cancelled);
        assert!(source.exists());
        assert!(!target.path().join("large.bin").exists());
        let retried = run(request, &TransferCancellation::default(), &mut |_| {});
        assert_eq!(retried.completed.len(), 1);
        assert!(!source.exists());
        assert_eq!(
            fs::read(target.path().join("large.bin")).unwrap(),
            vec![5; COPY_CHUNK_SIZE * 3]
        );
    }
}
