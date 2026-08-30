use std::path::{Path, PathBuf};

/// Stable identifier for Rift's virtual, aggregate Linux Trash location.
///
/// It deliberately is not an absolute filesystem path: directory access is
/// routed through the filesystem port, which resolves all Freedesktop trash
/// folders (including mounted volumes).
pub const TRASH_LOCATION: &str = "rift-trash:";

pub fn trash_location_path() -> PathBuf {
    PathBuf::from(TRASH_LOCATION)
}

pub fn is_trash_location(path: &Path) -> bool {
    path == Path::new(TRASH_LOCATION)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocationKind {
    Home,
    Applications,
    Desktop,
    Documents,
    Downloads,
    Videos,
    Music,
    Pictures,
    Trash,
    FileSystem,
    Volume { removable: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Location {
    path: PathBuf,
    name: String,
    kind: LocationKind,
}

impl Location {
    pub fn new(path: PathBuf, name: impl Into<String>, kind: LocationKind) -> Self {
        Self {
            path,
            name: name.into(),
            kind,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> LocationKind {
        self.kind
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NavigationSnapshot {
    pub locations: Vec<Location>,
    pub devices: Vec<Location>,
}
