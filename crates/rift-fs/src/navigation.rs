use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use rift_core::{
    domain::{Location, LocationKind, NavigationSnapshot, trash_location_path},
    ports::{NavigationError, NavigationSource},
};

#[derive(Clone, Debug)]
pub struct SystemNavigationSource {
    home: PathBuf,
}

impl SystemNavigationSource {
    pub fn new() -> Result<Self, NavigationError> {
        let home = dirs::home_dir()
            .or_else(|| std::env::current_dir().ok())
            .ok_or_else(|| NavigationError::new("unable to determine an initial directory"))?;
        Ok(Self { home })
    }

    pub fn initial_directory(&self) -> &Path {
        &self.home
    }

    fn favorites(&self) -> Vec<Location> {
        let mut locations = Vec::new();
        let mut seen = BTreeSet::new();
        push_existing(
            &mut locations,
            &mut seen,
            self.home.clone(),
            "Home",
            LocationKind::Home,
        );
        push_optional(
            &mut locations,
            &mut seen,
            Some(self.home.join("Applications")),
            "Applications",
            LocationKind::Applications,
        );
        push_optional(
            &mut locations,
            &mut seen,
            dirs::desktop_dir(),
            "Desktop",
            LocationKind::Desktop,
        );
        push_optional(
            &mut locations,
            &mut seen,
            dirs::document_dir(),
            "Documents",
            LocationKind::Documents,
        );
        push_optional(
            &mut locations,
            &mut seen,
            dirs::download_dir(),
            "Downloads",
            LocationKind::Downloads,
        );
        push_optional(
            &mut locations,
            &mut seen,
            dirs::video_dir(),
            "Videos",
            LocationKind::Videos,
        );
        push_optional(
            &mut locations,
            &mut seen,
            dirs::audio_dir(),
            "Music",
            LocationKind::Music,
        );
        push_optional(
            &mut locations,
            &mut seen,
            dirs::picture_dir(),
            "Pictures",
            LocationKind::Pictures,
        );
        locations.push(Location::new(
            trash_location_path(),
            "Trash",
            LocationKind::Trash,
        ));
        locations
    }
}

impl NavigationSource for SystemNavigationSource {
    fn load(&self) -> Result<NavigationSnapshot, NavigationError> {
        Ok(NavigationSnapshot {
            locations: self.favorites(),
            devices: system_devices()?,
        })
    }
}

fn push_optional(
    locations: &mut Vec<Location>,
    seen: &mut BTreeSet<PathBuf>,
    path: Option<PathBuf>,
    name: &str,
    kind: LocationKind,
) {
    if let Some(path) = path {
        push_existing(locations, seen, path, name, kind);
    }
}

fn push_existing(
    locations: &mut Vec<Location>,
    seen: &mut BTreeSet<PathBuf>,
    path: PathBuf,
    name: &str,
    kind: LocationKind,
) {
    if path.is_dir() && seen.insert(path.clone()) {
        locations.push(Location::new(path, name, kind));
    }
}

#[cfg(target_os = "linux")]
fn system_devices() -> Result<Vec<Location>, NavigationError> {
    let contents = fs::read_to_string("/proc/self/mountinfo")
        .map_err(|error| NavigationError::new(format!("failed to read mount table: {error}")))?;
    let mounts = parse_linux_mounts(&contents);
    let mut devices = vec![Location::new(
        PathBuf::from("/"),
        "File System",
        LocationKind::FileSystem,
    )];
    let mut seen = BTreeSet::new();
    seen.insert(PathBuf::from("/"));

    for mount in mounts {
        if mount.mount_point == Path::new("/")
            || !mount.source.starts_with("/dev/")
            || !is_user_volume(&mount.mount_point)
            || !seen.insert(mount.mount_point.clone())
        {
            continue;
        }
        let removable = is_removable_device(&mount.source)
            || mount.mount_point.starts_with("/run/media")
            || mount.mount_point.starts_with("/media");
        let name = device_label(&mount.source).unwrap_or_else(|| {
            mount
                .mount_point
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| mount.source.to_string_lossy().into_owned())
        });
        devices.push(Location::new(
            mount.mount_point,
            name,
            LocationKind::Volume { removable },
        ));
    }
    devices[1..].sort_by(|left, right| left.name().cmp(right.name()));
    Ok(devices)
}

#[cfg(target_os = "linux")]
#[derive(Debug, Eq, PartialEq)]
struct LinuxMount {
    mount_point: PathBuf,
    source: PathBuf,
}

#[cfg(target_os = "linux")]
fn parse_linux_mounts(contents: &str) -> Vec<LinuxMount> {
    contents
        .lines()
        .filter_map(|line| {
            let (mount, file_system) = line.split_once(" - ")?;
            let mount_point = mount.split_whitespace().nth(4)?;
            let source = file_system.split_whitespace().nth(1)?;
            Some(LinuxMount {
                mount_point: decode_mount_path(mount_point),
                source: decode_mount_path(source),
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn decode_mount_path(value: &str) -> PathBuf {
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 3 < bytes.len()
            && bytes[index + 1..index + 4]
                .iter()
                .all(|byte| matches!(byte, b'0'..=b'7'))
        {
            decoded.push(
                (bytes[index + 1] - b'0') * 64
                    + (bytes[index + 2] - b'0') * 8
                    + (bytes[index + 3] - b'0'),
            );
            index += 4;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    #[cfg(unix)]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        PathBuf::from(OsString::from_vec(decoded))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(&decoded).into_owned())
    }
}

#[cfg(target_os = "linux")]
fn is_user_volume(path: &Path) -> bool {
    path.starts_with("/run/media") || path.starts_with("/media") || path.starts_with("/mnt")
}

#[cfg(target_os = "linux")]
fn is_removable_device(source: &Path) -> bool {
    let Some(name) = source.file_name() else {
        return false;
    };
    let sys_path = Path::new("/sys/class/block").join(name);
    let direct = sys_path.join("removable");
    if fs::read_to_string(direct).is_ok_and(|value| value.trim() == "1") {
        return true;
    }
    fs::canonicalize(sys_path)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .and_then(|parent| fs::read_to_string(parent.join("removable")).ok())
        .is_some_and(|value| value.trim() == "1")
}

#[cfg(target_os = "linux")]
fn device_label(source: &Path) -> Option<String> {
    let target = fs::canonicalize(source).ok()?;
    let labels = fs::read_dir("/dev/disk/by-label").ok()?;
    labels.filter_map(Result::ok).find_map(|entry| {
        let matches = fs::canonicalize(entry.path()).is_ok_and(|path| path == target);
        matches.then(|| entry.file_name().to_string_lossy().into_owned())
    })
}

#[cfg(target_os = "macos")]
fn system_devices() -> Result<Vec<Location>, NavigationError> {
    let mut devices = vec![Location::new(
        PathBuf::from("/"),
        "File System",
        LocationKind::FileSystem,
    )];
    if let Ok(volumes) = fs::read_dir("/Volumes") {
        devices.extend(volumes.filter_map(Result::ok).filter_map(|entry| {
            let path = entry.path();
            path.is_dir().then(|| {
                Location::new(
                    path,
                    entry.file_name().to_string_lossy(),
                    LocationKind::Volume { removable: true },
                )
            })
        }));
    }
    Ok(devices)
}

#[cfg(target_os = "windows")]
fn system_devices() -> Result<Vec<Location>, NavigationError> {
    Ok((b'A'..=b'Z')
        .filter_map(|letter| {
            let path = PathBuf::from(format!("{}:\\", letter as char));
            path.is_dir().then(|| {
                Location::new(
                    path.clone(),
                    path.display().to_string(),
                    LocationKind::Volume { removable: false },
                )
            })
        })
        .collect())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn system_devices() -> Result<Vec<Location>, NavigationError> {
    Ok(vec![Location::new(
        PathBuf::from("/"),
        "File System",
        LocationKind::FileSystem,
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_real_home_and_system_devices() {
        let source = SystemNavigationSource::new().unwrap();
        let snapshot = source.load().unwrap();

        assert!(snapshot.locations.iter().any(|item| {
            item.kind() == LocationKind::Home && item.path() == source.initial_directory()
        }));
        assert!(
            snapshot
                .locations
                .iter()
                .any(|item| item.kind() == LocationKind::Trash)
        );
        assert!(!snapshot.devices.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_escaped_linux_mount_paths() {
        let mounts =
            parse_linux_mounts("42 31 8:1 / /run/media/me/My\\040Disk rw - ext4 /dev/sda1 rw\n");

        assert_eq!(
            mounts,
            vec![LinuxMount {
                mount_point: PathBuf::from("/run/media/me/My Disk"),
                source: PathBuf::from("/dev/sda1"),
            }]
        );
    }
}
