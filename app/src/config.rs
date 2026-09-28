use std::{
    sync::mpsc::{self, Sender, TryRecvError},
    thread::{self, JoinHandle},
};

use gpui::{App, BorrowAppContext, Global};
use rift_config::{
    BrowserConfig, BrowserSortDirection, BrowserSortField, BrowserViewMode, LoadedConfig,
};
use rift_core::application::{SortDirection, SortField, SortSpec, ViewMode};

pub(crate) struct AppConfig {
    loaded: LoadedConfig,
    writer: ConfigWriter,
}

impl Global for AppConfig {}

enum ConfigWriteMessage {
    Save(LoadedConfig),
    Shutdown,
}

struct ConfigWriter {
    sender: Option<Sender<ConfigWriteMessage>>,
    thread: Option<JoinHandle<()>>,
}

impl ConfigWriter {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("rift-config-writer".to_owned())
            .spawn(move || {
                while let Ok(message) = receiver.recv() {
                    let ConfigWriteMessage::Save(mut latest) = message else {
                        break;
                    };
                    let mut shutdown = false;

                    loop {
                        match receiver.try_recv() {
                            Ok(ConfigWriteMessage::Save(config)) => latest = config,
                            Ok(ConfigWriteMessage::Shutdown) => {
                                shutdown = true;
                                break;
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                shutdown = true;
                                break;
                            }
                        }
                    }

                    if let Err(error) = latest.save() {
                        log::error!(
                            "failed to save configuration {}: {error:#}",
                            latest.path.display()
                        );
                    }
                    if shutdown {
                        break;
                    }
                }
            })
            .expect("failed to start Rift configuration writer");
        Self {
            sender: Some(sender),
            thread: Some(thread),
        }
    }

    fn save(&self, config: LoadedConfig) {
        let Some(sender) = &self.sender else {
            return;
        };
        if sender.send(ConfigWriteMessage::Save(config)).is_err() {
            log::error!("Rift configuration writer is unavailable");
        }
    }
}

impl Drop for ConfigWriter {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(ConfigWriteMessage::Shutdown);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BrowserPreferences {
    pub(crate) view_mode: ViewMode,
    pub(crate) sort: SortSpec,
    pub(crate) show_hidden_files: bool,
    pub(crate) sidebar_visible: bool,
}

impl AppConfig {
    pub(crate) fn new(loaded: LoadedConfig) -> Self {
        Self {
            loaded,
            writer: ConfigWriter::new(),
        }
    }

    pub(crate) fn loaded(&self) -> &LoadedConfig {
        &self.loaded
    }

    pub(crate) fn browser_preferences(&self) -> BrowserPreferences {
        let browser = &self.loaded.config.browser;
        BrowserPreferences {
            view_mode: match browser.view_mode {
                BrowserViewMode::Grid => ViewMode::Grid,
                BrowserViewMode::List => ViewMode::List,
            },
            sort: SortSpec {
                field: match browser.sort_field {
                    BrowserSortField::Name => SortField::Name,
                    BrowserSortField::Modified => SortField::Modified,
                    BrowserSortField::Size => SortField::Size,
                    BrowserSortField::Kind => SortField::Kind,
                },
                direction: match browser.sort_direction {
                    BrowserSortDirection::Ascending => SortDirection::Ascending,
                    BrowserSortDirection::Descending => SortDirection::Descending,
                },
                directories_first: browser.directories_first,
            },
            show_hidden_files: browser.show_hidden_files,
            sidebar_visible: browser.sidebar_visible,
        }
    }

    pub(crate) fn update_browser_state(
        cx: &mut impl BorrowAppContext,
        view_mode: ViewMode,
        sort: SortSpec,
        show_hidden_files: bool,
    ) {
        Self::update_browser_config(cx, |browser| {
            browser.view_mode = match view_mode {
                ViewMode::Grid => BrowserViewMode::Grid,
                ViewMode::List => BrowserViewMode::List,
            };
            browser.sort_field = match sort.field {
                SortField::Name => BrowserSortField::Name,
                SortField::Modified => BrowserSortField::Modified,
                SortField::Size => BrowserSortField::Size,
                SortField::Kind => BrowserSortField::Kind,
            };
            browser.sort_direction = match sort.direction {
                SortDirection::Ascending => BrowserSortDirection::Ascending,
                SortDirection::Descending => BrowserSortDirection::Descending,
            };
            browser.directories_first = sort.directories_first;
            browser.show_hidden_files = show_hidden_files;
        });
    }

    pub(crate) fn update_sidebar_visible(cx: &mut impl BorrowAppContext, sidebar_visible: bool) {
        Self::update_browser_config(cx, |browser| {
            browser.sidebar_visible = sidebar_visible;
        });
    }

    pub(crate) fn vim_mode(cx: &App) -> bool {
        cx.global::<Self>().loaded.config.browser.vim_mode
    }

    pub(crate) fn update_vim_mode(cx: &mut impl BorrowAppContext, vim_mode: bool) {
        Self::update_browser_config(cx, |browser| {
            browser.vim_mode = vim_mode;
        });
    }

    fn update_browser_config(
        cx: &mut impl BorrowAppContext,
        update: impl FnOnce(&mut BrowserConfig),
    ) {
        cx.update_global::<Self, _>(|app_config, _| {
            let previous = app_config.loaded.config.browser.clone();
            update(&mut app_config.loaded.config.browser);
            if app_config.loaded.config.browser != previous {
                app_config.writer.save(app_config.loaded.clone());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use rift_config::{
        BrowserConfig, BrowserSortDirection, BrowserSortField, BrowserViewMode, RiftConfig,
    };

    use super::*;

    #[test]
    fn maps_persisted_browser_preferences_to_application_state() {
        let loaded = LoadedConfig {
            path: "config.toml".into(),
            config: RiftConfig {
                browser: BrowserConfig {
                    view_mode: BrowserViewMode::List,
                    sort_field: BrowserSortField::Size,
                    sort_direction: BrowserSortDirection::Descending,
                    directories_first: false,
                    show_hidden_files: true,
                    sidebar_visible: false,
                    vim_mode: true,
                },
                ..RiftConfig::default()
            },
        };

        assert_eq!(
            AppConfig::new(loaded).browser_preferences(),
            BrowserPreferences {
                view_mode: ViewMode::List,
                sort: SortSpec {
                    field: SortField::Size,
                    direction: SortDirection::Descending,
                    directories_first: false,
                },
                show_hidden_files: true,
                sidebar_visible: false,
            }
        );
    }

    #[test]
    fn background_writer_flushes_the_latest_snapshot_on_shutdown() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let mut first = LoadedConfig::load_or_create(path.clone()).unwrap();
        first.config.browser.view_mode = BrowserViewMode::List;
        let mut latest = first.clone();
        latest.config.browser.show_hidden_files = true;

        let writer = ConfigWriter::new();
        writer.save(first);
        writer.save(latest.clone());
        drop(writer);

        assert_eq!(
            LoadedConfig::load_or_create(path).unwrap().config,
            latest.config
        );
    }
}
