use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryCategory {
    Folder,
    Application,
    Document,
    Image,
    Audio,
    Video,
    Archive,
    Code,
    Alias,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    path: PathBuf,
    name: OsString,
    kind: EntryKind,
    size: u64,
    modified: Option<SystemTime>,
    hidden: bool,
}

impl Entry {
    pub fn new(
        path: PathBuf,
        name: OsString,
        kind: EntryKind,
        size: u64,
        modified: Option<SystemTime>,
        hidden: bool,
    ) -> Self {
        Self {
            path,
            name,
            kind,
            size,
            modified,
            hidden,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> &OsString {
        &self.name
    }

    pub fn kind(&self) -> EntryKind {
        self.kind
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn modified(&self) -> Option<SystemTime> {
        self.modified
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    pub fn is_directory(&self) -> bool {
        self.kind == EntryKind::Directory
    }

    pub fn category(&self) -> EntryCategory {
        match self.kind {
            EntryKind::Directory => EntryCategory::Folder,
            EntryKind::Symlink => EntryCategory::Alias,
            EntryKind::Other => EntryCategory::Other,
            EntryKind::File => category_for_extension(
                Path::new(&self.name)
                    .extension()
                    .and_then(|extension| extension.to_str()),
            ),
        }
    }
}

fn category_for_extension(extension: Option<&str>) -> EntryCategory {
    let Some(extension) = extension else {
        return EntryCategory::Other;
    };
    match extension.to_ascii_lowercase().as_str() {
        "app" | "appimage" | "desktop" | "exe" | "msi" | "apk" | "deb" | "rpm" => {
            EntryCategory::Application
        }
        "txt" | "md" | "markdown" | "pdf" | "doc" | "docx" | "odt" | "rtf" | "xls" | "xlsx"
        | "ods" | "csv" | "ppt" | "pptx" | "odp" | "epub" => EntryCategory::Document,
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "svg" | "heic"
        | "avif" | "ico" | "raw" => EntryCategory::Image,
        "mp3" | "m4a" | "aac" | "flac" | "wav" | "ogg" | "opus" | "wma" | "aiff" => {
            EntryCategory::Audio
        }
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" | "wmv" | "flv" | "mpeg" | "mpg" => {
            EntryCategory::Video
        }
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "tgz" | "dmg" | "iso" => {
            EntryCategory::Archive
        }
        "rs" | "c" | "h" | "cpp" | "hpp" | "cc" | "py" | "js" | "jsx" | "ts" | "tsx" | "java"
        | "kt" | "kts" | "go" | "rb" | "php" | "swift" | "html" | "css" | "scss" | "sass"
        | "less" | "json" | "toml" | "yaml" | "yml" | "xml" | "sh" | "bash" | "zsh" | "fish"
        | "sql" | "lua" | "vim" => EntryCategory::Code,
        _ => EntryCategory::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> Entry {
        Entry::new(
            PathBuf::from(name),
            OsString::from(name),
            EntryKind::File,
            0,
            None,
            false,
        )
    }

    #[test]
    fn classifies_common_file_extensions_case_insensitively() {
        assert_eq!(file("photo.PNG").category(), EntryCategory::Image);
        assert_eq!(file("movie.mkv").category(), EntryCategory::Video);
        assert_eq!(file("notes.md").category(), EntryCategory::Document);
        assert_eq!(file("main.rs").category(), EntryCategory::Code);
        assert_eq!(file("unknown").category(), EntryCategory::Other);
    }
}
