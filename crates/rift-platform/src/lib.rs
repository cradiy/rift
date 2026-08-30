use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    path::{Path, PathBuf},
};

pub fn is_macos() -> bool {
    cfg!(target_os = "macos")
}

pub fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

pub fn is_windows() -> bool {
    cfg!(target_os = "windows")
}

pub fn open_path(path: &Path) -> Result<(), OpenPathError> {
    open::that_detached(path).map_err(|error| OpenPathError {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenPathError {
    pub path: PathBuf,
    pub message: String,
}

impl Display for OpenPathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unable to open {}: {}",
            self.path.display(),
            self.message
        )
    }
}

impl Error for OpenPathError {}
