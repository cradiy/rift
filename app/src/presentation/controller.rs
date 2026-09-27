use std::sync::Arc;

use gpui::{AppContext, Context, Task};
use rift_core::{
    application::{BrowserEffect, BrowserMessage, BrowserState},
    ports::{FileOperation, FileOperationResult, FileSystem, FileSystemError},
};

use crate::config::AppConfig;

pub(crate) struct BrowserController {
    state: BrowserState,
    file_system: Arc<dyn FileSystem>,
    clipboard: Vec<std::path::PathBuf>,
}

impl BrowserController {
    pub(crate) fn new(state: BrowserState, file_system: Arc<dyn FileSystem>) -> Self {
        Self {
            state,
            file_system,
            clipboard: Vec::new(),
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
        self.clipboard = self.selected_paths();
        self.clipboard.len()
    }

    pub(crate) fn copy_selection(&mut self, fallback: std::path::PathBuf) -> usize {
        let selected = self.selected_paths();
        self.clipboard = if selected.contains(&fallback) && !selected.is_empty() {
            selected
        } else {
            vec![fallback]
        };
        self.clipboard.len()
    }

    pub(crate) fn clipboard(&self) -> &[std::path::PathBuf] {
        &self.clipboard
    }

    pub(crate) fn forget_clipboard_paths(&mut self, paths: &[std::path::PathBuf]) {
        self.clipboard.retain(|path| !paths.contains(path));
    }

    pub(crate) fn perform(
        &self,
        operation: FileOperation,
        cx: &Context<Self>,
    ) -> Task<Result<FileOperationResult, FileSystemError>> {
        let file_system = self.file_system.clone();
        cx.background_spawn(async move { file_system.perform(operation) })
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
