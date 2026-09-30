use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use rift_core::ports::{
    ConflictChoice, ConflictEntry, FileSystemError, FileSystemOperation, TransferCancellation,
    TransferConflict, TransferFailure, TransferKind, TransferOptions, TransferPhase,
    TransferProgress, TransferReport, TransferRequest, TransferredItem,
};

use crate::local::{
    available_copy_destination, ensure_directory, ensure_not_inside_source, remove_entry,
};

const COPY_CHUNK_SIZE: usize = 1024 * 1024;

enum TransferError {
    Cancelled,
    Skipped,
    Conflict(Box<TransferConflict>),
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
    options: Option<TransferOptions>,
    copied_sources: Vec<(PathBuf, ConflictEntry)>,
    retire_replacement: fn(&Path) -> std::io::Result<()>,
}

pub(super) fn run(
    request: TransferRequest,
    cancel: &TransferCancellation,
    publish: &mut dyn FnMut(TransferProgress),
) -> TransferReport {
    run_with_options(request, None, cancel, publish)
}

pub(super) fn run_with_options(
    request: TransferRequest,
    options: Option<TransferOptions>,
    cancel: &TransferCancellation,
    publish: &mut dyn FnMut(TransferProgress),
) -> TransferReport {
    run_with_disposal(request, options, cancel, publish, |path| {
        trash::delete(path).map_err(std::io::Error::other)
    })
}

fn run_with_disposal(
    request: TransferRequest,
    options: Option<TransferOptions>,
    cancel: &TransferCancellation,
    publish: &mut dyn FnMut(TransferProgress),
    retire_replacement: fn(&Path) -> std::io::Result<()>,
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
        options,
        copied_sources: Vec::new(),
        retire_replacement,
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
        let size = runner.prepare(source, &request.directory);
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
            Err(TransferError::Skipped | TransferError::Conflict(_)) => {
                unreachable!("preparation cannot resolve conflicts")
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
            Err(TransferError::Skipped) => {
                report.skipped.push(source.clone());
                runner.progress.skipped_items = report.skipped.len();
                runner.progress.total_bytes = runner
                    .progress
                    .total_bytes
                    .map(|total| total.saturating_sub(*bytes));
            }
            Err(TransferError::Conflict(conflict)) => {
                report.conflict = Some(*conflict);
                report
                    .remaining
                    .extend(prepared[index..].iter().map(|(source, _)| source.clone()));
                break;
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

    fn prepare(&mut self, source: &Path, directory: &Path) -> Result<u64, TransferError> {
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
        self.measure(source)
    }

    fn measure(&mut self, source: &Path) -> Result<u64, TransferError> {
        self.check_cancel()?;
        self.progress.current_path = Some(source.to_path_buf());
        let metadata = fs::symlink_metadata(source).map_err(|error| self.error(source, error))?;
        self.progress.total_entries += 1;
        self.publish();
        if metadata.is_file() {
            return Ok(metadata.len());
        }
        if metadata.file_type().is_symlink() {
            return Ok(0);
        }
        if !metadata.is_dir() {
            return Err(self.error(source, "This item type cannot be copied"));
        }
        let mut bytes = 0u64;
        for entry in fs::read_dir(source).map_err(|error| self.error(source, error))? {
            let entry = entry.map_err(|error| self.error(source, error))?;
            bytes = bytes.saturating_add(self.measure(&entry.path())?);
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
        self.copied_sources.clear();
        let direct = directory.join(
            source
                .file_name()
                .ok_or_else(|| self.error(source, "The source has no file name"))?,
        );
        let existing = match fs::symlink_metadata(&direct) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(self.error(&direct, error)),
        };
        // Copying into the source's own directory is an intentional duplicate.
        // Moving there is a no-op, never a self-replacement.
        let incoming = fs::symlink_metadata(source).map_err(|error| self.error(source, error))?;
        let same_item = source == direct
            || existing.as_ref().is_some_and(|existing| {
                let incoming = fingerprint(&incoming);
                let existing = fingerprint(existing);
                incoming.device != 0
                    && incoming.device == existing.device
                    && incoming.inode == existing.inode
            });
        if same_item && kind == TransferKind::Move {
            return Err(TransferError::Skipped);
        }
        let choice = if let Some(existing) = existing.as_ref() {
            if same_item {
                Some(ConflictChoice::KeepBoth)
            } else if self.options.is_some() {
                let conflict = TransferConflict {
                    source: source.to_path_buf(),
                    destination: direct.clone(),
                    replace_allowed: replacement_is_safe(source, &direct, &incoming, existing),
                    incoming: fingerprint(&incoming),
                    existing: fingerprint(existing),
                };
                let options = self.options.as_mut().unwrap();
                let decision = options.decision.take();
                let stale = decision.as_ref().is_some_and(|decision| {
                    decision.conflict.source == source && decision.conflict != conflict
                });
                let matching_decision = decision.filter(|decision| decision.conflict == conflict);
                // If the target changed after prompting, ask again even when
                // this task has an apply-to-all policy.
                let choice = if stale {
                    None
                } else {
                    matching_decision
                        .map(|decision| decision.choice)
                        .or(options.policy)
                };
                let Some(choice) = choice else {
                    return Err(TransferError::Conflict(Box::new(conflict)));
                };
                if choice == ConflictChoice::Replace {
                    if !conflict.replace_allowed {
                        return Err(TransferError::Conflict(Box::new(conflict)));
                    }
                    return self.replace_item(source, &direct, kind, &conflict);
                }
                Some(choice)
            } else if kind == TransferKind::Copy {
                Some(ConflictChoice::KeepBoth)
            } else {
                None
            }
        } else {
            None
        };
        if choice == Some(ConflictChoice::Skip) {
            return Err(TransferError::Skipped);
        }
        let destination = if choice == Some(ConflictChoice::KeepBoth) {
            available_copy_destination(source, directory).map_err(|error| {
                TransferError::Failed {
                    error,
                    retryable: true,
                }
            })?
        } else {
            direct
        };
        if kind == TransferKind::Move {
            match rename_without_replacing(source, &destination) {
                Ok(()) => return Ok(destination),
                Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {}
                Err(error) => return Err(self.error(source, error)),
            }
        }
        self.copy_owned(source, &destination)?;
        if let Err(error) = sync_directory(directory) {
            let failure = self.error(&destination, error);
            remove_entry(&destination).map_err(|error| self.cleanup_error(&destination, error))?;
            return Err(failure);
        }
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
            if let Err(error) = self.validate_copied_sources() {
                remove_entry(&destination)
                    .map_err(|error| self.cleanup_error(&destination, error))?;
                return Err(error);
            }
            remove_entry(source).map_err(|error| TransferError::Failed {
                error: FileSystemError::new(self.operation, source,
                    format!("Copied to {} but could not fully remove the source: {error}. Check both locations before retrying.", destination.display())),
                retryable: false,
            })?;
        }
        Ok(destination)
    }

    fn replace_item(
        &mut self,
        source: &Path,
        destination: &Path,
        kind: TransferKind,
        conflict: &TransferConflict,
    ) -> Result<PathBuf, TransferError> {
        // Build on the destination filesystem. Until the exchange, the old
        // target and source are untouched; keep the staging path on recovery
        // failure rather than allowing TempDir::drop to erase the backup.
        let parent = destination.parent().unwrap();
        let stage = tempfile::Builder::new()
            .prefix(".rift-transfer-")
            .tempdir_in(parent)
            .map_err(|error| self.error(parent, error))?
            .keep();
        let staged = stage.join(destination.file_name().unwrap());
        let prepared = (|| {
            self.copy_owned(source, &staged)?;
            sync_directory(&stage).map_err(|error| self.error(&stage, error))?;
            let existing = fs::symlink_metadata(destination)
                .map_err(|error| self.error(destination, error))?;
            if fingerprint(&existing) != conflict.existing {
                return Err(self.error(destination, "The destination changed while preparing the replacement. Retry to review it again."));
            }
            self.check_cancel()?;
            self.validate_copied_sources()?;
            exchange_entries(&staged, destination)
                .map_err(|error| self.error(destination, error))?;
            Ok(())
        })();
        if let Err(error) = prepared {
            remove_entry(&stage).map_err(|error| self.cleanup_error(&stage, error))?;
            return Err(error);
        }
        // The old target is now the staged entry. If committing or moving it
        // to Trash fails, restore it; never delete that backup during recovery.
        let commit = (|| {
            let exchanged = fs::symlink_metadata(&staged)?;
            if fingerprint(&exchanged) != conflict.existing {
                return Err(std::io::Error::other(
                    "The target changed immediately before replacement",
                ));
            }
            sync_directory(parent)?;
            (self.retire_replacement)(&staged)
        })();
        if let Err(error) = commit {
            if let Err(rollback) = exchange_entries(&staged, destination) {
                return Err(TransferError::Failed {
                    error: FileSystemError::new(
                        self.operation,
                        &stage,
                        format!(
                            "Replacement recovery failed: {error}; {rollback}. The backup remains here. Check both locations before retrying."
                        ),
                    ),
                    retryable: false,
                });
            }
            // Even a failed rollback sync must retain the new staged copy.
            sync_directory(parent).map_err(|error| self.cleanup_error(&stage, error))?;
            remove_entry(&stage).map_err(|error| self.cleanup_error(&stage, error))?;
            return Err(self.error(destination, format!("Replacement was rolled back: {error}")));
        }
        remove_entry(&stage).map_err(|error| self.cleanup_error(&stage, error))?;
        if kind == TransferKind::Move {
            // Replacement is already committed. Finish removing the source;
            // cancellation takes effect before the next top-level item.
            self.progress.phase = TransferPhase::Finishing;
            self.publish();
            if let Err(TransferError::Failed { error, .. }) = self.validate_copied_sources() {
                return Err(TransferError::Failed {
                    error,
                    retryable: false,
                });
            }
            remove_entry(source).map_err(|error| TransferError::Failed {
                error: FileSystemError::new(self.operation, source,
                    format!("Replaced {} but could not fully remove the source: {error}. Check both locations before retrying.", destination.display())),
                retryable: false,
            })?;
        }
        Ok(destination.to_path_buf())
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

    fn validate_copied_sources(&self) -> Result<(), TransferError> {
        for (path, expected) in &self.copied_sources {
            let current = fs::symlink_metadata(path).map_err(|error| self.error(path, error))?;
            if fingerprint(&current) != *expected {
                return Err(self.error(
                    path,
                    "The source changed while transferring. It was not removed.",
                ));
            }
        }
        Ok(())
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
        self.copied_sources
            .push((source.to_path_buf(), fingerprint(&metadata)));
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
            let opened = input
                .metadata()
                .map_err(|error| self.error(source, error))?;
            if fingerprint(&opened) != fingerprint(&metadata) {
                return Err(self.error(source, "The source was replaced before copying"));
            }
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
            let after = input
                .metadata()
                .map_err(|error| self.error(source, error))?;
            let current =
                fs::symlink_metadata(source).map_err(|error| self.error(source, error))?;
            if fingerprint(&after) != fingerprint(&metadata)
                || fingerprint(&current) != fingerprint(&metadata)
            {
                return Err(self.error(
                    source,
                    "The source changed while copying. Its original was not removed.",
                ));
            }
            // Report a full disk or delayed write failure before source removal.
            fs::set_permissions(destination, metadata.permissions())
                .map_err(|error| self.error(destination, error))?;
            output
                .sync_all()
                .map_err(|error| self.error(destination, error))?;
            return self.check_cancel();
        } else if metadata.file_type().is_symlink() {
            #[cfg(unix)]
            {
                let target = fs::read_link(source).map_err(|error| self.error(source, error))?;
                std::os::unix::fs::symlink(target, destination)
                    .map_err(|error| self.error(destination, error))?;
                *owned = true;
                return self.check_cancel();
            }
            #[cfg(not(unix))]
            return Err(self.error(source, "Copying links is not supported on this platform"));
        } else {
            return Err(self.error(source, "This item type cannot be copied"));
        }
        fs::set_permissions(destination, metadata.permissions())
            .map_err(|error| self.error(destination, error))?;
        sync_directory(destination).map_err(|error| self.error(destination, error))?;
        self.check_cancel()
    }
}

fn fingerprint(metadata: &Metadata) -> ConflictEntry {
    #[cfg(unix)]
    let (device, inode) = {
        use std::os::unix::fs::MetadataExt;
        (metadata.dev(), metadata.ino())
    };
    #[cfg(not(unix))]
    let (device, inode) = (0, 0);
    ConflictEntry {
        byte_len: metadata.len(),
        modified: metadata.modified().ok(),
        is_directory: metadata.is_dir(),
        is_symlink: metadata.file_type().is_symlink(),
        device,
        inode,
    }
}

fn replacement_is_safe(
    source: &Path,
    destination: &Path,
    incoming: &Metadata,
    existing: &Metadata,
) -> bool {
    let incoming = fingerprint(incoming);
    let existing_entry = fingerprint(existing);
    if incoming.device != 0
        && incoming.device == existing_entry.device
        && incoming.inode == existing_entry.inode
    {
        return false;
    }
    if existing.is_dir()
        && let (Ok(source), Ok(destination)) =
            (fs::canonicalize(source), fs::canonicalize(destination))
        && source.starts_with(destination)
    {
        return false;
    }
    // Atomic exchange is currently provided by the Linux filesystem adapter.
    cfg!(target_os = "linux")
}

fn sync_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn exchange_entries(first: &Path, second: &Path) -> std::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        first,
        rustix::fs::CWD,
        second,
        rustix::fs::RenameFlags::EXCHANGE,
    )
    .map_err(Into::into)
}

#[cfg(not(target_os = "linux"))]
fn exchange_entries(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Atomic replacement is not available on this platform",
    ))
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

    fn conflict_fixture() -> (tempfile::TempDir, TransferRequest) {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        for name in ["one.txt", "two.txt"] {
            fs::write(source.join(name), "incoming").unwrap();
            fs::write(target.join(name), "existing").unwrap();
        }
        let request = TransferRequest {
            kind: TransferKind::Move,
            sources: vec![source.join("one.txt"), source.join("two.txt")],
            directory: target,
        };
        (root, request)
    }

    #[test]
    fn conflicts_yield_without_mutation_and_keep_both_resumes_only_unfinished_items() {
        let (_root, mut request) = conflict_fixture();
        let first = run_with_options(
            request.clone(),
            Some(TransferOptions::default()),
            &TransferCancellation::default(),
            &mut |_| {},
        );
        assert!(first.completed.is_empty());
        assert_eq!(first.remaining, request.sources);
        assert_eq!(
            fs::read(request.directory.join("one.txt")).unwrap(),
            b"existing"
        );
        let conflict = first.conflict.unwrap();
        request.sources = first.remaining;
        let next = run_with_options(
            request.clone(),
            Some(TransferOptions {
                decision: Some(rift_core::ports::ConflictDecision {
                    conflict,
                    choice: ConflictChoice::KeepBoth,
                }),
                policy: Some(ConflictChoice::Skip),
            }),
            &TransferCancellation::default(),
            &mut |_| {},
        );
        assert_eq!(next.completed.len(), 1);
        assert_eq!(next.skipped, [request.sources[1].clone()]);
        assert_eq!(
            fs::read(request.directory.join("one copy.txt")).unwrap(),
            b"incoming"
        );
        assert!(!request.sources[0].exists());
        assert!(request.sources[1].exists());
        assert_eq!(
            fs::read(request.directory.join("one.txt")).unwrap(),
            b"existing"
        );
        assert!(next.conflict.is_none());
    }

    #[test]
    fn a_changed_target_invalidates_the_replacement_decision_even_with_apply_to_all() {
        let (_root, request) = conflict_fixture();
        let first = run_with_options(
            request.clone(),
            Some(TransferOptions::default()),
            &TransferCancellation::default(),
            &mut |_| {},
        );
        fs::write(request.directory.join("one.txt"), "changed by someone else").unwrap();
        let report = run_with_options(
            request.clone(),
            Some(TransferOptions {
                decision: Some(rift_core::ports::ConflictDecision {
                    conflict: first.conflict.unwrap(),
                    choice: ConflictChoice::Replace,
                }),
                policy: Some(ConflictChoice::Replace),
            }),
            &TransferCancellation::default(),
            &mut |_| {},
        );
        assert!(report.conflict.is_some());
        assert!(report.completed.is_empty());
        assert_eq!(
            fs::read(request.directory.join("one.txt")).unwrap(),
            b"changed by someone else"
        );
        assert!(request.sources[0].exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn replacing_a_folder_swaps_the_whole_item_and_only_then_removes_the_source() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("folder");
        let target = root.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir_all(target.join("folder")).unwrap();
        fs::write(source.join("new.txt"), "new").unwrap();
        fs::write(target.join("folder/old.txt"), "old").unwrap();
        let report = run_with_disposal(
            TransferRequest {
                kind: TransferKind::Move,
                sources: vec![source.clone()],
                directory: target.clone(),
            },
            Some(TransferOptions {
                policy: Some(ConflictChoice::Replace),
                ..Default::default()
            }),
            &TransferCancellation::default(),
            &mut |_| {},
            remove_entry,
        );
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(report.completed.len(), 1);
        assert!(!source.exists());
        assert_eq!(fs::read(target.join("folder/new.txt")).unwrap(), b"new");
        assert!(!target.join("folder/old.txt").exists());
        assert_eq!(fs::read_dir(target).unwrap().count(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn replacement_disposal_failure_rolls_back_without_losing_either_original() {
        let (_root, mut request) = conflict_fixture();
        request.sources.truncate(1);
        let report = run_with_disposal(
            request.clone(),
            Some(TransferOptions {
                policy: Some(ConflictChoice::Replace),
                ..Default::default()
            }),
            &TransferCancellation::default(),
            &mut |_| {},
            |_| Err(std::io::Error::other("simulated Trash failure")),
        );
        assert_eq!(report.failures.len(), 1);
        assert!(report.completed.is_empty());
        assert_eq!(fs::read(&request.sources[0]).unwrap(), b"incoming");
        assert_eq!(
            fs::read(request.directory.join("one.txt")).unwrap(),
            b"existing"
        );
        assert_eq!(fs::read_dir(request.directory).unwrap().count(), 2);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cancelling_a_replacement_leaves_the_old_target_and_source_untouched() {
        let (_root, mut request) = conflict_fixture();
        request.sources.truncate(1);
        fs::write(&request.sources[0], vec![8; COPY_CHUNK_SIZE * 3]).unwrap();
        let cancel = TransferCancellation::default();
        let report = run_with_disposal(
            request.clone(),
            Some(TransferOptions {
                policy: Some(ConflictChoice::Replace),
                ..Default::default()
            }),
            &cancel,
            &mut |progress| {
                if progress.transferred_bytes > 0 {
                    cancel.cancel();
                }
            },
            remove_entry,
        );
        assert!(report.cancelled);
        assert_eq!(
            fs::metadata(&request.sources[0]).unwrap().len(),
            (COPY_CHUNK_SIZE * 3) as u64
        );
        assert_eq!(
            fs::read(request.directory.join("one.txt")).unwrap(),
            b"existing"
        );
        assert_eq!(fs::read_dir(request.directory).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn replacing_a_target_that_contains_the_source_is_disabled() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("folder/nested/folder");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("keep.txt"), "keep").unwrap();
        let report = run_with_options(
            TransferRequest {
                kind: TransferKind::Move,
                sources: vec![source.clone()],
                directory: root.path().to_path_buf(),
            },
            Some(TransferOptions {
                policy: Some(ConflictChoice::Replace),
                ..Default::default()
            }),
            &TransferCancellation::default(),
            &mut |_| {},
        );
        assert!(!report.conflict.unwrap().replace_allowed);
        assert_eq!(fs::read(source.join("keep.txt")).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn broken_symlinks_are_preserved_and_are_not_treated_as_free_names() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("link");
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        symlink("missing-incoming", &source).unwrap();
        symlink("missing-existing", target.join("link")).unwrap();
        let report = run_with_options(
            TransferRequest {
                kind: TransferKind::Copy,
                sources: vec![source.clone()],
                directory: target.clone(),
            },
            Some(TransferOptions {
                policy: Some(ConflictChoice::KeepBoth),
                ..Default::default()
            }),
            &TransferCancellation::default(),
            &mut |_| {},
        );
        assert!(report.failures.is_empty());
        assert_eq!(
            fs::read_link(target.join("link")).unwrap(),
            PathBuf::from("missing-existing")
        );
        assert_eq!(
            fs::read_link(target.join("link copy")).unwrap(),
            PathBuf::from("missing-incoming")
        );
        assert!(fs::symlink_metadata(source).is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_device_move_refuses_to_remove_a_source_changed_after_copying() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("folder");
        fs::create_dir(&source).unwrap();
        let file = source.join("file.txt");
        fs::write(&file, "original").unwrap();
        let target = tempfile::Builder::new()
            .prefix("rift-transfer-test-")
            .tempdir_in("/dev/shm")
            .unwrap();
        let report = run(
            TransferRequest {
                kind: TransferKind::Move,
                sources: vec![source.clone()],
                directory: target.path().to_path_buf(),
            },
            &TransferCancellation::default(),
            &mut |progress| {
                if progress.phase == TransferPhase::Finishing {
                    fs::write(&file, "a new version written by someone else").unwrap();
                }
            },
        );
        assert_eq!(report.failures.len(), 1);
        assert!(report.completed.is_empty());
        assert_eq!(
            fs::read(&file).unwrap(),
            b"a new version written by someone else"
        );
        assert!(!target.path().join("folder").exists());
    }

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
        assert_eq!(last.total_entries, 4);
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
