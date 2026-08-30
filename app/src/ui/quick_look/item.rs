use std::path::PathBuf;

use rift_core::domain::EntryCategory;

use crate::presentation::BrowserItem;

/// Stable input shared by every Quick Look provider.
///
/// Keeping this independent from the file-browser selection state lets future
/// providers own loading state without coupling them to a particular view.
#[derive(Clone, Debug)]
pub(crate) struct QuickLookItem {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) size: String,
    pub(crate) modified: String,
    pub(crate) byte_len: u64,
    pub(crate) category: EntryCategory,
    pub(crate) is_directory: bool,
}

impl From<BrowserItem> for QuickLookItem {
    fn from(entry: BrowserItem) -> Self {
        Self {
            path: entry.path,
            name: entry.name,
            kind: entry.kind,
            size: entry.size,
            modified: entry.modified,
            byte_len: entry.byte_len,
            category: entry.category,
            is_directory: entry.is_directory,
        }
    }
}
