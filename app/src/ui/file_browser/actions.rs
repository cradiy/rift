use std::path::PathBuf;

use gpui::{App, KeyBinding, Window, actions};
use rift_core::{application::BrowserMessage, ports::FileOperation};
use uic::components::toast;

use crate::presentation::{BrowserItem, present_browser};

use super::{FileBrowser, file_actions::NewItemKind};

actions!(
    rift_file_browser,
    [
        OpenSelection,
        QuickLookSelection,
        RenameSelection,
        GetInfoSelection,
        CopyItems,
        PasteItems,
        TrashItems,
        PermanentlyDeleteItems,
        RefreshDirectory,
        NewFolder,
        SelectAllItems,
        ClearSelection,
        ToggleHiddenFiles,
    ]
);

pub(crate) fn init(cx: &mut App) {
    let context = Some("FileBrowser && !editing");
    cx.bind_keys([
        KeyBinding::new("enter", OpenSelection, context),
        KeyBinding::new("ctrl-o", OpenSelection, context),
        KeyBinding::new("space", QuickLookSelection, context),
        KeyBinding::new("f2", RenameSelection, context),
        KeyBinding::new("alt-enter", GetInfoSelection, context),
        KeyBinding::new("ctrl-c", CopyItems, context),
        KeyBinding::new("ctrl-v", PasteItems, context),
        KeyBinding::new("ctrl-r", RefreshDirectory, context),
        KeyBinding::new("f5", RefreshDirectory, context),
        KeyBinding::new("ctrl-shift-n", NewFolder, context),
        KeyBinding::new("ctrl-a", SelectAllItems, context),
        KeyBinding::new("ctrl-h", ToggleHiddenFiles, context),
        KeyBinding::new("escape", ClearSelection, context),
        KeyBinding::new("delete", TrashItems, context),
        KeyBinding::new("shift-delete", PermanentlyDeleteItems, context),
    ]);
}

impl FileBrowser {
    pub(super) fn open_selection(
        &mut self,
        _: &OpenSelection,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some((path, _, is_directory)) = self.primary_selection(cx) else {
            return;
        };
        Self::open_entry(self.controller.clone(), path, is_directory, cx);
    }

    pub(super) fn quick_look_selection(
        &mut self,
        _: &QuickLookSelection,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(entry) = self.primary_item(cx) else {
            return;
        };
        crate::ui::quick_look::show(entry, window, cx);
    }

    pub(super) fn rename_selection(
        &mut self,
        _: &RenameSelection,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.controller.read(cx).state().is_trash() {
            return;
        }
        let Some((path, name, _)) = self.primary_selection(cx) else {
            return;
        };
        self.begin_inline_rename(path, name, window, cx);
    }

    pub(super) fn get_info_selection(
        &mut self,
        _: &GetInfoSelection,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(entry) = self.primary_item(cx) else {
            return;
        };
        Self::show_info(entry, window, cx);
    }

    pub(super) fn copy_items(
        &mut self,
        _: &CopyItems,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let count = self
            .controller
            .update(cx, |controller, _| controller.copy_selected());
        if count > 0 {
            toast::success(
                if count == 1 {
                    "Copied 1 item".to_owned()
                } else {
                    format!("Copied {count} items")
                },
                cx,
            );
        }
    }

    pub(super) fn paste_items(
        &mut self,
        _: &PasteItems,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.controller.read(cx).state().is_trash() {
            return;
        }
        let (sources, directory) = {
            let controller = self.controller.read(cx);
            (
                controller.clipboard().to_vec(),
                controller.state().current_directory().to_path_buf(),
            )
        };
        if sources.is_empty() {
            return;
        }
        Self::run_operation(
            self.controller.clone(),
            FileOperation::CopyInto { sources, directory },
            "Pasted items",
            cx,
        );
    }

    pub(super) fn trash_items(
        &mut self,
        _: &TrashItems,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let paths = self.controller.read(cx).selected_paths();
        if paths.is_empty() {
            return;
        }
        if self.controller.read(cx).state().is_trash() {
            Self::show_permanent_delete_confirmation(self.controller.clone(), paths, window, cx);
            return;
        }
        Self::run_operation(
            self.controller.clone(),
            FileOperation::Trash { paths },
            "Moved to Trash",
            cx,
        );
    }

    pub(super) fn permanently_delete_items(
        &mut self,
        _: &PermanentlyDeleteItems,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.controller.read(cx).state().is_trash() {
            return;
        }
        let paths = self.controller.read(cx).selected_paths();
        if paths.is_empty() {
            return;
        }
        Self::show_permanent_delete_confirmation(self.controller.clone(), paths, window, cx);
    }

    pub(super) fn refresh_directory(
        &mut self,
        _: &RefreshDirectory,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.controller.update(cx, |controller, cx| {
            controller.dispatch(BrowserMessage::Refresh, cx);
        });
        self.navigation
            .update(cx, |controller, cx| controller.refresh(cx));
    }

    pub(super) fn new_folder(
        &mut self,
        _: &NewFolder,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.controller.read(cx).state().is_trash() {
            return;
        }
        let directory = self
            .controller
            .read(cx)
            .state()
            .current_directory()
            .to_path_buf();
        Self::show_new_item_dialog(
            self.controller.clone(),
            directory,
            NewItemKind::Directory,
            window,
            cx,
        );
    }

    pub(super) fn select_all_items(
        &mut self,
        _: &SelectAllItems,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.controller.update(cx, |controller, cx| {
            controller.dispatch(BrowserMessage::SelectAll, cx);
        });
    }

    pub(super) fn clear_selection(
        &mut self,
        _: &ClearSelection,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.controller.update(cx, |controller, cx| {
            controller.dispatch(BrowserMessage::ClearSelection, cx);
        });
    }

    pub(super) fn toggle_hidden_files(
        &mut self,
        _: &ToggleHiddenFiles,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let show = !self.controller.read(cx).state().show_hidden_files();
        self.controller.update(cx, |controller, cx| {
            controller.dispatch(BrowserMessage::SetShowHiddenFiles(show), cx);
        });
    }

    fn primary_selection(&self, cx: &App) -> Option<(PathBuf, String, bool)> {
        let entry = self.primary_item(cx)?;
        Some((entry.path, entry.name, entry.is_directory))
    }

    fn primary_item(&self, cx: &App) -> Option<BrowserItem> {
        let controller = self.controller.read(cx);
        let state = controller.state();
        let path = state.selection().iter().next()?;
        present_browser(state)
            .into_iter()
            .find(|entry| entry.path == *path)
    }
}
