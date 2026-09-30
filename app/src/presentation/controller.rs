use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use gpui::{AppContext, Context, Task};
use rift_core::{
    application::{BrowserEffect, BrowserMessage, BrowserState, LoadState},
    ports::{DirectoryWatch, FileOperation, FileOperationResult, FileSystem, FileSystemError},
};

use crate::config::AppConfig;

pub(crate) struct BrowserController {
    state: BrowserState,
    file_system: Arc<dyn FileSystem>,
    clipboard: SharedFileClipboard,
    directory_watch: Option<Box<dyn DirectoryWatch>>,
    watched_directory: Option<PathBuf>,
    directory_watch_generation: u64,
    auto_refresh_in_flight: bool,
}

const DIRECTORY_WATCH_POLL_INTERVAL: Duration = Duration::from_millis(220);

#[derive(Clone, Default)]
pub(crate) struct SharedFileClipboard(Arc<Mutex<Vec<PathBuf>>>);

impl SharedFileClipboard {
    fn read(&self) -> std::sync::MutexGuard<'_, Vec<PathBuf>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl BrowserController {
    pub(crate) fn with_clipboard(
        state: BrowserState,
        file_system: Arc<dyn FileSystem>,
        clipboard: SharedFileClipboard,
    ) -> Self {
        Self {
            state,
            file_system,
            clipboard,
            directory_watch: None,
            watched_directory: None,
            directory_watch_generation: 0,
            auto_refresh_in_flight: false,
        }
    }

    pub(crate) fn state(&self) -> &BrowserState {
        &self.state
    }

    pub(crate) fn file_system(&self) -> Arc<dyn FileSystem> {
        self.file_system.clone()
    }

    pub(crate) fn refresh_silently(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state.load_state(), LoadState::Idle) {
            self.refresh_from_directory_watch(self.directory_watch_generation, cx);
        }
    }

    pub(crate) fn selected_paths(&self) -> Vec<std::path::PathBuf> {
        self.state.selection().iter().cloned().collect()
    }

    pub(crate) fn selected_paths_or(
        &self,
        fallback: std::path::PathBuf,
    ) -> Vec<std::path::PathBuf> {
        let selected = self.selected_paths();
        if selected.contains(&fallback) && !selected.is_empty() {
            selected
        } else {
            vec![fallback]
        }
    }

    pub(crate) fn copy_selected(&mut self) -> usize {
        let selected = self.selected_paths();
        let count = selected.len();
        *self.clipboard.read() = selected;
        count
    }

    pub(crate) fn copy_selection(&mut self, fallback: std::path::PathBuf) -> usize {
        let selected = self.selected_paths();
        let clipboard = if selected.contains(&fallback) && !selected.is_empty() {
            selected
        } else {
            vec![fallback]
        };
        let count = clipboard.len();
        *self.clipboard.read() = clipboard;
        count
    }

    pub(crate) fn clipboard(&self) -> Vec<PathBuf> {
        self.clipboard.read().clone()
    }

    pub(crate) fn forget_clipboard_paths(&mut self, paths: &[std::path::PathBuf]) {
        self.clipboard.read().retain(|path| !paths.contains(path));
    }

    pub(crate) fn perform(
        &self,
        operation: FileOperation,
        cx: &Context<Self>,
    ) -> Task<Result<FileOperationResult, FileSystemError>> {
        let file_system = self.file_system.clone();
        cx.background_spawn(async move { file_system.perform(operation) })
    }

    pub(crate) fn count_directory_items(
        &self,
        path: std::path::PathBuf,
        include_hidden: bool,
        cx: &Context<Self>,
    ) -> Task<Result<usize, FileSystemError>> {
        let file_system = self.file_system.clone();
        cx.background_spawn(async move { file_system.count_directory_items(&path, include_hidden) })
    }

    pub(crate) fn dispatch(&mut self, message: BrowserMessage, cx: &mut Context<Self>) {
        let directory_before = self.state.current_directory().to_path_buf();
        let retries_directory_watch = matches!(&message, BrowserMessage::Refresh);
        let starts_directory_read = matches!(
            &message,
            BrowserMessage::Navigate(_)
                | BrowserMessage::GoBack
                | BrowserMessage::GoForward
                | BrowserMessage::GoUp
                | BrowserMessage::Refresh
                | BrowserMessage::RefreshSelecting(_)
        );
        let preferences_changed = matches!(
            &message,
            BrowserMessage::SetViewMode(_)
                | BrowserMessage::SetSort(_)
                | BrowserMessage::SetShowHiddenFiles(_)
        );
        let effects = self.state.update(message);
        let directory_changed = self.state.current_directory() != directory_before;
        if directory_changed
            || (starts_directory_read && self.watched_directory.is_none())
            || (retries_directory_watch && self.directory_watch.is_none() && !self.state.is_trash())
        {
            self.restart_directory_watch(cx);
        }
        if preferences_changed {
            AppConfig::update_browser_state(
                cx,
                self.state.view_mode(),
                self.state.sort(),
                self.state.show_hidden_files(),
            );
        }
        cx.notify();

        for effect in effects {
            self.execute(effect, cx);
        }
    }

    fn execute(&self, effect: BrowserEffect, cx: &mut Context<Self>) {
        let BrowserEffect::ReadDirectory { path, request_id } = effect;
        let file_system = self.file_system.clone();
        let path_for_read = path.clone();
        let read = cx.background_spawn(async move { file_system.read_directory(&path_for_read) });

        cx.spawn(async move |this, cx| {
            let result = read.await;
            let Some(this) = this.upgrade() else {
                return;
            };
            this.update(cx, |controller, cx| {
                let message = match result {
                    Ok(entries) => BrowserMessage::DirectoryLoaded {
                        path,
                        request_id,
                        entries,
                    },
                    Err(error) => BrowserMessage::DirectoryLoadFailed {
                        path,
                        request_id,
                        error,
                    },
                };
                controller.dispatch(message, cx);
            });
        })
        .detach();
    }

    fn restart_directory_watch(&mut self, cx: &mut Context<Self>) {
        self.directory_watch_generation = self.directory_watch_generation.wrapping_add(1);
        self.auto_refresh_in_flight = false;
        let generation = self.directory_watch_generation;
        let path = self.state.current_directory().to_path_buf();
        self.watched_directory = Some(path.clone());
        self.directory_watch = if self.state.is_trash() {
            None
        } else {
            match self.file_system.watch_directory(&path) {
                Ok(watch) => Some(watch),
                Err(error) => {
                    log::warn!("unable to watch {}: {error}", path.display());
                    None
                }
            }
        };
        let mut observed_revision = self
            .directory_watch
            .as_ref()
            .map_or(0, |watch| watch.revision());
        let mut pending_revision = None;

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(DIRECTORY_WATCH_POLL_INTERVAL)
                    .await;
                let Some(this) = this.upgrade() else {
                    return;
                };
                let mut keep_watching = true;
                this.update(cx, |controller, cx| {
                    if controller.directory_watch_generation != generation {
                        keep_watching = false;
                        return;
                    }
                    let Some(revision) = controller
                        .directory_watch
                        .as_ref()
                        .map(|watch| watch.revision())
                    else {
                        keep_watching = false;
                        return;
                    };
                    if revision == observed_revision {
                        pending_revision = None;
                        return;
                    }
                    if pending_revision != Some(revision) {
                        pending_revision = Some(revision);
                        return;
                    }
                    if !matches!(controller.state.load_state(), LoadState::Idle)
                        || controller.auto_refresh_in_flight
                    {
                        return;
                    }
                    observed_revision = revision;
                    pending_revision = None;
                    controller.refresh_from_directory_watch(generation, cx);
                });
                if !keep_watching {
                    return;
                }
            }
        })
        .detach();
    }

    fn refresh_from_directory_watch(&mut self, generation: u64, cx: &mut Context<Self>) {
        if self.auto_refresh_in_flight {
            return;
        }
        self.auto_refresh_in_flight = true;
        let path = self.state.current_directory().to_path_buf();
        let file_system = self.file_system.clone();
        let path_for_read = path.clone();
        let read = cx.background_spawn(async move { file_system.read_directory(&path_for_read) });

        cx.spawn(async move |this, cx| {
            let result = read.await;
            let Some(this) = this.upgrade() else {
                return;
            };
            this.update(cx, |controller, cx| {
                if controller.directory_watch_generation != generation {
                    return;
                }
                controller.auto_refresh_in_flight = false;
                match result {
                    Ok(entries) => {
                        controller.dispatch(BrowserMessage::DirectoryChanged { path, entries }, cx)
                    }
                    Err(error) => log::debug!("automatic directory refresh failed: {error}"),
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::SharedFileClipboard;

    #[test]
    fn cloned_file_clipboards_share_the_same_paths() {
        let first = SharedFileClipboard::default();
        let second = first.clone();

        first.read().push(PathBuf::from("/tmp/copied"));

        assert_eq!(second.read().as_slice(), [PathBuf::from("/tmp/copied")]);
    }
}
