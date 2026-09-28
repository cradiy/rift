use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use gpui::{AppContext, Context, Task};
use rift_core::{
    application::{BrowserEffect, BrowserMessage, BrowserState},
    ports::{FileOperation, FileOperationResult, FileSystem, FileSystemError},
};

use crate::config::AppConfig;

pub(crate) struct BrowserController {
    state: BrowserState,
    file_system: Arc<dyn FileSystem>,
    clipboard: SharedFileClipboard,
}

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
        }
    }

    pub(crate) fn state(&self) -> &BrowserState {
        &self.state
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
        let preferences_changed = matches!(
            &message,
            BrowserMessage::SetViewMode(_)
                | BrowserMessage::SetSort(_)
                | BrowserMessage::SetShowHiddenFiles(_)
        );
        let effects = self.state.update(message);
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
