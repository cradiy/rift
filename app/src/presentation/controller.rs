use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use gpui::{App, AppContext, Context, Entity, Global, Task};
use rift_core::{
    application::{BrowserEffect, BrowserMessage, BrowserState, LoadState},
    ports::{
        DirectoryWatch, FileOperation, FileOperationResult, FileSystem, FileSystemError,
        TransferKind, TransferRequest,
    },
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
pub(crate) struct SharedFileClipboard(Arc<Mutex<FileClipboard>>);

#[derive(Default)]
struct FileClipboard {
    paths: Vec<PathBuf>,
    cut: bool,
    generation: u64,
}

struct ClipboardUpdates(Entity<()>);
impl Global for ClipboardUpdates {}

impl SharedFileClipboard {
    fn read(&self) -> std::sync::MutexGuard<'_, FileClipboard> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn updates(cx: &mut App) -> Entity<()> {
        if let Some(updates) = cx.try_global::<ClipboardUpdates>() {
            return updates.0.clone();
        }
        let updates = cx.new(|_| ());
        cx.set_global(ClipboardUpdates(updates.clone()));
        updates
    }

    fn notify(cx: &mut App) {
        Self::updates(cx).update(cx, |_, cx| cx.notify());
    }

    fn set(&self, paths: Vec<PathBuf>, cut: bool, cx: &mut App) -> usize {
        if paths.is_empty() {
            return 0;
        }
        let count = paths.len();
        {
            let mut clipboard = self.read();
            clipboard.generation = clipboard.generation.wrapping_add(1);
            clipboard.paths = paths;
            clipboard.cut = cut;
        }
        Self::notify(cx);
        count
    }

    pub(crate) fn generation(&self) -> u64 {
        self.read().generation
    }

    pub(crate) fn complete_move(&self, generation: u64, paths: &[PathBuf], cx: &mut App) {
        if paths.is_empty() {
            return;
        }
        {
            let mut clipboard = self.read();
            if clipboard.generation != generation || !clipboard.cut {
                return;
            }
            clipboard.paths.retain(|path| !paths.contains(path));
        }
        Self::notify(cx);
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

    pub(crate) fn copy_selected(&mut self, cx: &mut App) -> usize {
        self.clipboard.set(self.selected_paths(), false, cx)
    }

    pub(crate) fn copy_selection(&mut self, fallback: PathBuf, cx: &mut App) -> usize {
        self.clipboard
            .set(self.selected_paths_or(fallback), false, cx)
    }

    pub(crate) fn cut_selected(&mut self, cx: &mut App) -> usize {
        if self.state.is_trash() {
            return 0;
        }
        self.clipboard.set(self.selected_paths(), true, cx)
    }

    pub(crate) fn cut_selection(&mut self, fallback: PathBuf, cx: &mut App) -> usize {
        if self.state.is_trash() {
            return 0;
        }
        self.clipboard
            .set(self.selected_paths_or(fallback), true, cx)
    }

    pub(crate) fn is_cut(&self, path: &std::path::Path) -> bool {
        let clipboard = self.clipboard.read();
        clipboard.cut && clipboard.paths.iter().any(|item| item == path)
    }

    pub(crate) fn clipboard_handle(&self) -> SharedFileClipboard {
        self.clipboard.clone()
    }

    pub(crate) fn paste_request(&self, directory: PathBuf) -> Option<TransferRequest> {
        let clipboard = self.clipboard.read();
        let sources = clipboard
            .paths
            .iter()
            .filter(|path| !clipboard.cut || path.parent() != Some(directory.as_path()))
            .cloned()
            .collect::<Vec<_>>();
        (!sources.is_empty()).then_some(TransferRequest {
            kind: if clipboard.cut {
                TransferKind::Move
            } else {
                TransferKind::Copy
            },
            sources,
            directory,
        })
    }

    pub(crate) fn clipboard(&self) -> Vec<PathBuf> {
        self.clipboard.read().paths.clone()
    }

    pub(crate) fn forget_clipboard_paths(&mut self, paths: &[PathBuf], cx: &mut App) {
        self.clipboard
            .read()
            .paths
            .retain(|path| !paths.contains(path));
        SharedFileClipboard::notify(cx);
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

    use super::{BrowserController, SharedFileClipboard};
    use gpui::{AppContext, TestAppContext};
    use rift_core::{application::BrowserState, ports::TransferKind};
    use rift_fs::LocalFileSystem;
    use std::sync::Arc;

    #[gpui::test]
    fn clipboard_mode_is_shared_and_an_old_move_cannot_clear_a_new_cut(cx: &mut TestAppContext) {
        let clipboard = SharedFileClipboard::default();
        let root = PathBuf::from("/tmp/source");
        let path = root.join("file.txt");
        let first = cx.new(|_| {
            BrowserController::with_clipboard(
                BrowserState::new(root.clone()),
                Arc::new(LocalFileSystem),
                clipboard.clone(),
            )
        });
        let second = cx.new(|_| {
            BrowserController::with_clipboard(
                BrowserState::new(PathBuf::from("/tmp/target")),
                Arc::new(LocalFileSystem),
                clipboard.clone(),
            )
        });
        cx.update(|cx| {
            first.update(cx, |controller, cx| {
                controller.cut_selection(path.clone(), cx)
            });
            assert!(second.read(cx).is_cut(&path));
            assert!(second.read(cx).paste_request(root.clone()).is_none());
            assert_eq!(
                second
                    .read(cx)
                    .paste_request(PathBuf::from("/tmp/target"))
                    .unwrap()
                    .kind,
                TransferKind::Move
            );
            let old_generation = clipboard.generation();
            second.update(cx, |controller, cx| {
                controller.copy_selection(path.clone(), cx)
            });
            assert!(!first.read(cx).is_cut(&path));
            assert_eq!(
                first.read(cx).paste_request(root.clone()).unwrap().kind,
                TransferKind::Copy
            );
            second.update(cx, |controller, cx| {
                controller.cut_selection(path.clone(), cx)
            });
            clipboard.complete_move(old_generation, std::slice::from_ref(&path), cx);
            assert!(first.read(cx).is_cut(&path));
            clipboard.complete_move(clipboard.generation(), std::slice::from_ref(&path), cx);
            assert!(second.read(cx).clipboard().is_empty());
        });
    }

    #[test]
    fn cloned_file_clipboards_share_the_same_paths() {
        let first = SharedFileClipboard::default();
        let second = first.clone();

        first.read().paths.push(PathBuf::from("/tmp/copied"));

        assert_eq!(
            second.read().paths.as_slice(),
            [PathBuf::from("/tmp/copied")]
        );
    }
}
