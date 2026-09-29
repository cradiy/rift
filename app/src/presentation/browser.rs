use std::path::PathBuf;
use std::time::SystemTime;

use chrono::{DateTime, Local};
use rift_core::{
    application::BrowserState,
    domain::{EntryCategory, EntryKind},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ItemIcon {
    Folder,
    File,
    Application,
    Desktop,
    Document,
    Download,
    Videos,
    Music,
    Picture,
}

#[derive(Clone, Debug)]
pub(crate) struct BrowserItem {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) detail: String,
    pub(crate) modified: String,
    pub(crate) size: String,
    pub(crate) byte_len: u64,
    pub(crate) modified_at: Option<SystemTime>,
    pub(crate) kind: String,
    pub(crate) category: EntryCategory,
    pub(crate) icon: ItemIcon,
    pub(crate) is_directory: bool,
    pub(crate) alias: bool,
    pub(crate) selected: bool,
}

pub(crate) fn present_browser(state: &BrowserState) -> Vec<BrowserItem> {
    state
        .visible_entries()
        .map(|entry| {
            let name = entry.name().to_string_lossy().into_owned();
            let is_directory = entry.is_directory();
            let category = entry.category();
            BrowserItem {
                path: entry.path().to_path_buf(),
                detail: if is_directory {
                    "Folder".to_owned()
                } else {
                    format_size(entry.size())
                },
                modified: entry
                    .modified()
                    .map(|modified| {
                        DateTime::<Local>::from(modified)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_else(|| "—".to_owned()),
                size: if is_directory {
                    "—".to_owned()
                } else {
                    format_size(entry.size())
                },
                byte_len: entry.size(),
                modified_at: entry.modified(),
                kind: category_item_label(category).to_owned(),
                category,
                icon: item_icon(&name, is_directory),
                is_directory,
                alias: entry.kind() == EntryKind::Symlink,
                selected: state.selection().contains(entry.path()),
                name,
            }
        })
        .collect()
}

fn category_item_label(category: EntryCategory) -> &'static str {
    match category {
        EntryCategory::Folder => "Folder",
        EntryCategory::Application => "Application",
        EntryCategory::Document => "Document",
        EntryCategory::Image => "Image",
        EntryCategory::Audio => "Audio",
        EntryCategory::Video => "Video",
        EntryCategory::Archive => "Archive",
        EntryCategory::Code => "Source Code",
        EntryCategory::Alias => "Symbolic Link",
        EntryCategory::Other => "Other",
    }
}

fn item_icon(name: &str, is_directory: bool) -> ItemIcon {
    if !is_directory {
        return ItemIcon::File;
    }
    match name {
        "Applications" => ItemIcon::Application,
        "Desktop" => ItemIcon::Desktop,
        "Documents" => ItemIcon::Document,
        "Downloads" => ItemIcon::Download,
        "Videos" => ItemIcon::Videos,
        "Music" => ItemIcon::Music,
        "Pictures" => ItemIcon::Picture,
        _ => ItemIcon::Folder,
    }
}

pub(crate) fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::format_size;

    #[test]
    fn formats_file_sizes_for_display() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
    }
}
