use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rift_core::{
    domain::{Entry, EntryKind, is_trash_location, trash_location_path},
    ports::{
        DirectoryWatch, FileOperation, FileOperationResult, FileSystem, FileSystemError,
        FileSystemOperation,
    },
};

#[derive(Clone, Copy, Debug, Default)]
pub struct LocalFileSystem;

impl FileSystem for LocalFileSystem {
    fn read_directory(&self, path: &Path) -> Result<Vec<Entry>, FileSystemError> {
        if is_trash_location(path) {
            return read_trash_directory();
        }
        let directory = fs::read_dir(path).map_err(|error| {
            FileSystemError::new(FileSystemOperation::ReadDirectory, path, error.to_string())
        })?;
        let mut entries = Vec::new();

        for item in directory {
            let item = item.map_err(|error| {
                FileSystemError::new(FileSystemOperation::ReadDirectory, path, error.to_string())
            })?;
            let entry_path = item.path();
            let file_type = item.file_type().map_err(|error| {
                FileSystemError::new(
                    FileSystemOperation::ReadMetadata,
                    &entry_path,
                    error.to_string(),
                )
            })?;
            let metadata = item.metadata().map_err(|error| {
                FileSystemError::new(
                    FileSystemOperation::ReadMetadata,
                    &entry_path,
                    error.to_string(),
                )
            })?;
            let name = item.file_name();
            let kind = if file_type.is_dir() {
                EntryKind::Directory
            } else if file_type.is_file() {
                EntryKind::File
            } else if file_type.is_symlink() {
                EntryKind::Symlink
            } else {
                EntryKind::Other
            };
            let hidden = name.to_string_lossy().starts_with('.');

            entries.push(Entry::new(
                entry_path,
                name,
                kind,
                metadata.len(),
                metadata.modified().ok(),
                hidden,
            ));
        }

        entries.sort_by(|left, right| left.name().cmp(right.name()));
        Ok(entries)
    }

    fn count_directory_items(
        &self,
        path: &Path,
        include_hidden: bool,
    ) -> Result<usize, FileSystemError> {
        let directory = fs::read_dir(path).map_err(|error| {
            FileSystemError::new(
                FileSystemOperation::CountDirectoryItems,
                path,
                error.to_string(),
            )
        })?;
        let mut count = 0;
        for item in directory {
            let item = item.map_err(|error| {
                FileSystemError::new(
                    FileSystemOperation::CountDirectoryItems,
                    path,
                    error.to_string(),
                )
            })?;
            if include_hidden || !item.file_name().to_string_lossy().starts_with('.') {
                count += 1;
            }
        }
        Ok(count)
    }

    fn perform(&self, operation: FileOperation) -> Result<FileOperationResult, FileSystemError> {
        match operation {
            FileOperation::Rename {
                source,
                destination,
            } => {
                ensure_available(&destination, FileSystemOperation::Rename)?;
                fs::rename(&source, &destination).map_err(|error| {
                    FileSystemError::new(FileSystemOperation::Rename, &source, error.to_string())
                })?;
                Ok(result(destination))
            }
            FileOperation::CreateDirectory { path } => {
                fs::create_dir(&path).map_err(|error| {
                    FileSystemError::new(
                        FileSystemOperation::CreateDirectory,
                        &path,
                        error.to_string(),
                    )
                })?;
                Ok(result(path))
            }
            FileOperation::CreateFile { path } => {
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|error| {
                        FileSystemError::new(
                            FileSystemOperation::CreateFile,
                            &path,
                            error.to_string(),
                        )
                    })?;
                Ok(result(path))
            }
            FileOperation::CopyInto { sources, directory } => {
                ensure_directory(&directory, FileSystemOperation::Copy)?;
                let mut affected_paths = Vec::with_capacity(sources.len());
                for source in sources {
                    ensure_not_inside_source(&source, &directory, FileSystemOperation::Copy)?;
                    let destination = available_copy_destination(&source, &directory)?;
                    if let Err(error) = copy_entry(&source, &destination) {
                        let _ = remove_entry(&destination);
                        return Err(error);
                    }
                    affected_paths.push(destination);
                }
                Ok(FileOperationResult { affected_paths })
            }
            FileOperation::MoveInto { sources, directory } => {
                ensure_directory(&directory, FileSystemOperation::Move)?;
                let mut affected_paths = Vec::with_capacity(sources.len());
                for source in sources {
                    ensure_not_inside_source(&source, &directory, FileSystemOperation::Move)?;
                    let name = source.file_name().ok_or_else(|| {
                        FileSystemError::new(
                            FileSystemOperation::Move,
                            &source,
                            "the source has no file name",
                        )
                    })?;
                    let destination = directory.join(name);
                    ensure_available(&destination, FileSystemOperation::Move)?;
                    move_entry(&source, &destination)?;
                    affected_paths.push(destination);
                }
                Ok(FileOperationResult { affected_paths })
            }
            FileOperation::Trash { paths } => {
                if paths.is_empty() {
                    return Ok(FileOperationResult::default());
                }
                trash::delete_all(&paths).map_err(|error| {
                    FileSystemError::new(
                        FileSystemOperation::Trash,
                        paths[0].clone(),
                        error.to_string(),
                    )
                })?;
                Ok(FileOperationResult {
                    affected_paths: paths,
                })
            }
            FileOperation::PurgeTrash { paths } => purge_trash_items(paths),
        }
    }

    fn watch_directory(&self, path: &Path) -> Result<Box<dyn DirectoryWatch>, FileSystemError> {
        let revision = Arc::new(AtomicU64::new(0));
        let callback_revision = revision.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if !matches!(event, Ok(event) if matches!(event.kind, EventKind::Access(_))) {
                    callback_revision.fetch_add(1, Ordering::Relaxed);
                }
            })
            .map_err(|error| {
                FileSystemError::new(FileSystemOperation::WatchDirectory, path, error.to_string())
            })?;
        watcher
            .watch(path, RecursiveMode::NonRecursive)
            .map_err(|error| {
                FileSystemError::new(FileSystemOperation::WatchDirectory, path, error.to_string())
            })?;

        Ok(Box::new(NotifyDirectoryWatch {
            _watcher: watcher,
            revision,
        }))
    }
}

struct NotifyDirectoryWatch {
    _watcher: RecommendedWatcher,
    revision: Arc<AtomicU64>,
}

impl DirectoryWatch for NotifyDirectoryWatch {
    fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }
}

fn read_trash_directory() -> Result<Vec<Entry>, FileSystemError> {
    let items = trash::os_limited::list().map_err(|error| {
        FileSystemError::new(
            FileSystemOperation::ReadDirectory,
            trash_location_path(),
            error.to_string(),
        )
    })?;
    let mut entries = Vec::with_capacity(items.len());
    for item in items {
        let Some(path) = trash_item_storage_path(&item) else {
            continue;
        };
        let metadata = fs::symlink_metadata(&path).ok();
        let kind = metadata
            .as_ref()
            .map(|metadata| {
                let file_type = metadata.file_type();
                if file_type.is_dir() {
                    EntryKind::Directory
                } else if file_type.is_file() {
                    EntryKind::File
                } else if file_type.is_symlink() {
                    EntryKind::Symlink
                } else {
                    EntryKind::Other
                }
            })
            .unwrap_or(EntryKind::Other);
        let modified = (item.time_deleted >= 0)
            .then(|| SystemTime::UNIX_EPOCH + Duration::from_secs(item.time_deleted as u64));
        let hidden = item.name.to_string_lossy().starts_with('.');
        entries.push(Entry::new(
            path,
            item.name,
            kind,
            metadata.map_or(0, |metadata| metadata.len()),
            modified,
            hidden,
        ));
    }
    entries.sort_by(|left, right| left.name().cmp(right.name()));
    Ok(entries)
}

fn purge_trash_items(paths: Vec<PathBuf>) -> Result<FileOperationResult, FileSystemError> {
    if paths.is_empty() {
        return Ok(FileOperationResult::default());
    }
    let requested: BTreeSet<_> = paths.iter().cloned().collect();
    let items = trash::os_limited::list().map_err(|error| {
        FileSystemError::new(
            FileSystemOperation::PurgeTrash,
            trash_location_path(),
            error.to_string(),
        )
    })?;
    let selected: Vec<_> = items
        .into_iter()
        .filter(|item| trash_item_storage_path(item).is_some_and(|path| requested.contains(&path)))
        .collect();
    if selected.len() != requested.len() {
        return Err(FileSystemError::new(
            FileSystemOperation::PurgeTrash,
            paths[0].clone(),
            "one or more selected items are no longer in Trash",
        ));
    }
    trash::os_limited::purge_all(selected).map_err(|error| {
        FileSystemError::new(
            FileSystemOperation::PurgeTrash,
            paths[0].clone(),
            error.to_string(),
        )
    })?;
    Ok(FileOperationResult {
        affected_paths: paths,
    })
}

fn trash_item_storage_path(item: &trash::TrashItem) -> Option<PathBuf> {
    let info_file = Path::new(&item.id);
    let trash_root = info_file.parent()?.parent()?;
    Some(trash_root.join("files").join(info_file.file_stem()?))
}

fn result(path: PathBuf) -> FileOperationResult {
    FileOperationResult {
        affected_paths: vec![path],
    }
}

fn ensure_available(path: &Path, operation: FileSystemOperation) -> Result<(), FileSystemError> {
    if path.exists() {
        return Err(FileSystemError::new(
            operation,
            path,
            "an item with this name already exists",
        ));
    }
    Ok(())
}

fn ensure_directory(path: &Path, operation: FileSystemOperation) -> Result<(), FileSystemError> {
    if !path.is_dir() {
        return Err(FileSystemError::new(
            operation,
            path,
            "the destination is not a directory",
        ));
    }
    Ok(())
}

fn ensure_not_inside_source(
    source: &Path,
    directory: &Path,
    operation: FileSystemOperation,
) -> Result<(), FileSystemError> {
    if source.is_dir() && directory.starts_with(source) {
        return Err(FileSystemError::new(
            operation,
            directory,
            "a folder cannot be copied or moved into itself",
        ));
    }
    Ok(())
}

fn available_copy_destination(source: &Path, directory: &Path) -> Result<PathBuf, FileSystemError> {
    let name = source.file_name().ok_or_else(|| {
        FileSystemError::new(
            FileSystemOperation::Copy,
            source,
            "the source has no file name",
        )
    })?;
    let direct = directory.join(name);
    if !direct.exists() {
        return Ok(direct);
    }

    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string_lossy().into_owned());
    let extension = source
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned());
    for index in 1.. {
        let suffix = if index == 1 {
            " copy".to_owned()
        } else {
            format!(" copy {index}")
        };
        let candidate_name = match extension.as_deref() {
            Some(extension) if source.is_file() => format!("{stem}{suffix}.{extension}"),
            _ => format!("{stem}{suffix}"),
        };
        let candidate = directory.join(candidate_name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    unreachable!()
}

fn copy_entry(source: &Path, destination: &Path) -> Result<(), FileSystemError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        FileSystemError::new(FileSystemOperation::Copy, source, error.to_string())
    })?;
    if metadata.file_type().is_symlink() {
        return Err(FileSystemError::new(
            FileSystemOperation::Copy,
            source,
            "copying symbolic links is not supported yet",
        ));
    }
    if metadata.is_dir() {
        fs::create_dir(destination).map_err(|error| {
            FileSystemError::new(FileSystemOperation::Copy, destination, error.to_string())
        })?;
        for entry in fs::read_dir(source).map_err(|error| {
            FileSystemError::new(FileSystemOperation::Copy, source, error.to_string())
        })? {
            let entry = entry.map_err(|error| {
                FileSystemError::new(FileSystemOperation::Copy, source, error.to_string())
            })?;
            copy_entry(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        fs::copy(source, destination).map_err(|error| {
            FileSystemError::new(FileSystemOperation::Copy, source, error.to_string())
        })?;
    } else {
        return Err(FileSystemError::new(
            FileSystemOperation::Copy,
            source,
            "this item type cannot be copied",
        ));
    }
    Ok(())
}

fn move_entry(source: &Path, destination: &Path) -> Result<(), FileSystemError> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            if let Err(error) = copy_entry(source, destination) {
                let _ = remove_entry(destination);
                return Err(error);
            }
            remove_entry(source).map_err(|error| {
                FileSystemError::new(FileSystemOperation::Move, source, error.to_string())
            })
        }
        Err(error) => Err(FileSystemError::new(
            FileSystemOperation::Move,
            source,
            error.to_string(),
        )),
    }
}

fn remove_entry(path: &Path) -> std::io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        fs,
        path::Path,
        thread,
        time::{Duration, Instant},
    };

    use rift_core::ports::{FileOperation, FileSystem};

    use super::{LocalFileSystem, trash_item_storage_path};

    #[test]
    fn resolves_the_storage_path_for_a_freedesktop_trash_item() {
        let item = trash::TrashItem {
            id: OsString::from("/mnt/disk/.Trash-1000/info/photo.png.2.trashinfo"),
            name: OsString::from("photo.png"),
            original_parent: "/home/me/Pictures".into(),
            time_deleted: 0,
        };

        assert_eq!(
            trash_item_storage_path(&item),
            Some("/mnt/disk/.Trash-1000/files/photo.png.2".into())
        );
    }

    #[test]
    fn reads_its_own_manifest_directory() {
        let entries = LocalFileSystem
            .read_directory(Path::new(env!("CARGO_MANIFEST_DIR")))
            .expect("manifest directory should be readable");

        assert!(
            entries
                .iter()
                .any(|entry| entry.name().to_string_lossy() == "Cargo.toml")
        );
    }

    #[test]
    fn counts_directory_items_with_the_requested_hidden_file_policy() {
        let root = tempfile::tempdir().expect("temporary directory");
        fs::write(root.path().join("visible.txt"), "visible").unwrap();
        fs::write(root.path().join(".hidden.txt"), "hidden").unwrap();
        fs::create_dir(root.path().join("folder")).unwrap();

        assert_eq!(
            LocalFileSystem
                .count_directory_items(root.path(), false)
                .unwrap(),
            2
        );
        assert_eq!(
            LocalFileSystem
                .count_directory_items(root.path(), true)
                .unwrap(),
            3
        );
    }

    #[test]
    fn watches_immediate_directory_changes() {
        let root = tempfile::tempdir().expect("temporary directory");
        let watch = LocalFileSystem
            .watch_directory(root.path())
            .expect("temporary directory should be watchable");
        let initial_revision = watch.revision();

        fs::write(root.path().join("created.txt"), "created").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while watch.revision() == initial_revision && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }

        assert!(watch.revision() > initial_revision);
    }

    #[test]
    fn creates_renames_copies_and_moves_items_without_overwriting() {
        let root = tempfile::tempdir().expect("temporary directory");
        let source_dir = root.path().join("source");
        let destination_dir = root.path().join("destination");
        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&destination_dir).unwrap();

        let file_system = LocalFileSystem;
        let note = source_dir.join("Note.txt");
        file_system
            .perform(FileOperation::CreateFile { path: note.clone() })
            .unwrap();
        let renamed = source_dir.join("Renamed.txt");
        file_system
            .perform(FileOperation::Rename {
                source: note,
                destination: renamed.clone(),
            })
            .unwrap();

        file_system
            .perform(FileOperation::CopyInto {
                sources: vec![renamed.clone()],
                directory: destination_dir.clone(),
            })
            .unwrap();
        let duplicate = file_system
            .perform(FileOperation::CopyInto {
                sources: vec![renamed.clone()],
                directory: destination_dir.clone(),
            })
            .unwrap();
        assert!(destination_dir.join("Renamed.txt").is_file());
        assert_eq!(
            duplicate.affected_paths,
            vec![destination_dir.join("Renamed copy.txt")]
        );

        file_system
            .perform(FileOperation::MoveInto {
                sources: vec![renamed.clone()],
                directory: root.path().to_path_buf(),
            })
            .unwrap();
        assert!(!renamed.exists());
        assert!(root.path().join("Renamed.txt").is_file());
    }

    #[test]
    fn rejects_copying_a_directory_into_itself() {
        let root = tempfile::tempdir().expect("temporary directory");
        let source = root.path().join("source");
        let child = source.join("child");
        fs::create_dir_all(&child).unwrap();

        let result = LocalFileSystem.perform(FileOperation::CopyInto {
            sources: vec![source],
            directory: child,
        });

        assert!(result.is_err());
    }
}
