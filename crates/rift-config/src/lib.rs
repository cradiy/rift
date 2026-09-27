use std::{
    env,
    ffi::OsStr,
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const CONFIG_ENV: &str = "RIFT_CONFIG";

const DEFAULT_CONFIG: &str = include_str!("../../../config.toml.example");

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RiftConfig {
    pub fonts: FontsConfig,
    pub logging: LoggingConfig,
    pub browser: BrowserConfig,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FontsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
    Off,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Off => "off",
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    pub level: LogLevel,
    pub directory: PathBuf,
    pub module_filters: String,
    pub max_file_size_bytes: u64,
    pub retained_files: usize,
    pub console: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrowserConfig {
    pub view_mode: BrowserViewMode,
    pub sort_field: BrowserSortField,
    pub sort_direction: BrowserSortDirection,
    pub directories_first: bool,
    pub show_hidden_files: bool,
    pub sidebar_visible: bool,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            view_mode: BrowserViewMode::Grid,
            sort_field: BrowserSortField::Name,
            sort_direction: BrowserSortDirection::Ascending,
            directories_first: true,
            show_hidden_files: false,
            sidebar_visible: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserViewMode {
    #[default]
    Grid,
    List,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserSortField {
    #[default]
    Name,
    Modified,
    Size,
    Kind,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserSortDirection {
    #[default]
    Ascending,
    Descending,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            directory: PathBuf::from("~/.local/state/rift/logs"),
            module_filters: "gpui::window=off,wgpu=error,wgpu_hal=error,naga=error,zbus=error,sum_tree=error,tracing::span=error".to_owned(),
            max_file_size_bytes: 10 * 1024 * 1024,
            retained_files: 7,
            console: true,
        }
    }
}

impl LoggingConfig {
    pub fn filter_spec(&self) -> String {
        let module_filters = self.module_filters.trim();
        if module_filters.is_empty() {
            self.level.to_string()
        } else {
            format!("{},{module_filters}", self.level)
        }
    }
}

#[derive(Clone, Debug)]
pub struct LoadedConfig {
    pub path: PathBuf,
    pub config: RiftConfig,
}

impl LoadedConfig {
    pub fn load_default() -> Result<Self> {
        Self::load_or_create(default_config_path()?)
    }

    pub fn load_or_create(path: PathBuf) -> Result<Self> {
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_default_config(&path)?;
                fs::read_to_string(&path)
                    .with_context(|| format!("failed to read configuration {}", path.display()))?
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read configuration {}", path.display()));
            }
        };
        let config: RiftConfig = toml::from_str(&contents)
            .with_context(|| format!("invalid configuration {}", path.display()))?;
        config.validate()?;
        Ok(Self { path, config })
    }

    pub fn log_directory(&self) -> Result<PathBuf> {
        let directory = expand_home(&self.config.logging.directory)?;
        if directory.is_absolute() {
            return Ok(directory);
        }
        let parent = self
            .path
            .parent()
            .context("configuration path has no parent directory")?;
        Ok(parent.join(directory))
    }

    pub fn save(&self) -> Result<()> {
        self.config.validate()?;
        let contents = toml::to_string_pretty(&self.config)
            .context("failed to serialize Rift configuration")?;
        let parent = self
            .path
            .parent()
            .context("configuration path has no parent directory")?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create configuration directory {}",
                parent.display()
            )
        })?;

        let mut temporary = tempfile::NamedTempFile::new_in(parent).with_context(|| {
            format!(
                "failed to create temporary configuration in {}",
                parent.display()
            )
        })?;
        temporary.write_all(contents.as_bytes()).with_context(|| {
            format!(
                "failed to write temporary configuration for {}",
                self.path.display()
            )
        })?;
        temporary.flush().with_context(|| {
            format!(
                "failed to flush temporary configuration for {}",
                self.path.display()
            )
        })?;
        temporary.as_file().sync_all().with_context(|| {
            format!(
                "failed to sync temporary configuration for {}",
                self.path.display()
            )
        })?;
        temporary
            .persist(&self.path)
            .map_err(|error| error.error)
            .with_context(|| format!("failed to replace configuration {}", self.path.display()))?;
        Ok(())
    }
}

impl RiftConfig {
    fn validate(&self) -> Result<()> {
        if self
            .fonts
            .family
            .as_ref()
            .is_some_and(|family| family.trim().is_empty())
        {
            bail!("fonts.family must not be empty when specified");
        }
        if self.logging.directory.as_os_str().is_empty() {
            bail!("logging.directory must not be empty");
        }
        if self.logging.max_file_size_bytes == 0 {
            bail!("logging.max_file_size_bytes must be greater than zero");
        }
        if self.logging.retained_files == 0 {
            bail!("logging.retained_files must be greater than zero");
        }
        Ok(())
    }
}

pub fn default_config_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os(CONFIG_ENV) {
        if path.is_empty() {
            bail!("{CONFIG_ENV} must not be empty");
        }
        return Ok(PathBuf::from(path));
    }
    let directory =
        dirs::config_dir().context("unable to determine the configuration directory")?;
    Ok(directory.join("rift/config.toml"))
}

fn create_default_config(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .context("configuration path has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create configuration directory {}",
            parent.display()
        )
    })?;
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file
            .write_all(DEFAULT_CONFIG.as_bytes())
            .with_context(|| format!("failed to write default configuration {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to create configuration {}", path.display()))
        }
    }
}

fn expand_home(path: &Path) -> Result<PathBuf> {
    let mut components = path.components();
    let Some(first) = components.next() else {
        return Ok(path.to_owned());
    };
    if first.as_os_str() != OsStr::new("~") {
        return Ok(path.to_owned());
    }
    let home = dirs::home_dir().context("unable to determine the user's home directory")?;
    Ok(home.join(components.as_path()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_loads_default_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/config.toml");

        let loaded = LoadedConfig::load_or_create(path.clone()).unwrap();

        assert_eq!(loaded.path, path);
        assert_eq!(loaded.config.logging.level, LogLevel::Info);
        assert!(loaded.path.exists());
    }

    #[test]
    fn resolves_relative_log_directory_from_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(
            &path,
            "[logging]\ndirectory = \"logs\"\nmodule_filters = \"\"\n",
        )
        .unwrap();

        let loaded = LoadedConfig::load_or_create(path).unwrap();

        assert_eq!(
            loaded.log_directory().unwrap(),
            directory.path().join("logs")
        );
        assert_eq!(loaded.config.logging.filter_spec(), "info");
    }

    #[test]
    fn rejects_unknown_fields() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "unknown = true\n").unwrap();

        let error = LoadedConfig::load_or_create(path).unwrap_err();

        assert!(format!("{error:#}").contains("unknown field"));
    }

    #[test]
    fn combines_level_and_module_filters() {
        let logging = LoggingConfig {
            level: LogLevel::Debug,
            module_filters: "wgpu=error".to_owned(),
            ..Default::default()
        };

        assert_eq!(logging.filter_spec(), "debug,wgpu=error");
    }

    #[test]
    fn saves_and_reloads_browser_preferences_as_toml() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let mut loaded = LoadedConfig::load_or_create(path.clone()).unwrap();
        loaded.config.browser = BrowserConfig {
            view_mode: BrowserViewMode::List,
            sort_field: BrowserSortField::Modified,
            sort_direction: BrowserSortDirection::Descending,
            directories_first: false,
            show_hidden_files: true,
            sidebar_visible: false,
        };

        loaded.save().unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let reloaded = LoadedConfig::load_or_create(path).unwrap();
        assert!(contents.contains("[browser]"));
        assert_eq!(reloaded.config, loaded.config);
    }

    #[test]
    fn invalid_configuration_is_not_written() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let mut loaded = LoadedConfig::load_or_create(path.clone()).unwrap();
        let original = fs::read_to_string(&path).unwrap();
        loaded.config.fonts.family = Some("   ".to_owned());

        assert!(loaded.save().is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }
}
