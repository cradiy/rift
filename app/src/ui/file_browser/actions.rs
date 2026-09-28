use std::path::PathBuf;

use gpui::{App, KeyBinding, Window, actions, px};
use rift_core::{
    application::{BrowserMessage, SelectionMode, SortField, ViewMode},
    ports::FileOperation,
};
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
        RequestVimTrash,
        ConfirmVimTrash,
        CancelVimTrash,
        VimTrashImmediately,
        PermanentlyDeleteItems,
        RefreshDirectory,
        NewFolder,
        AddItem,
        StartCopyPrefix,
        StartGoPrefix,
        CopyFileNameText,
        CopyFilePathText,
        CopyParentDirectoryText,
        CancelWhichKey,
        SelectAllItems,
        ClearSelection,
        ToggleHiddenFiles,
        MoveSelectionLeft,
        MoveSelectionRight,
        MoveSelectionUp,
        MoveSelectionDown,
        ExtendSelectionLeft,
        ExtendSelectionRight,
        ExtendSelectionUp,
        ExtendSelectionDown,
        SelectFirstItem,
        SelectLastItem,
        SelectPreviousPage,
        SelectNextPage,
        GoToParentDirectory,
        GoBackDirectory,
        GoForwardDirectory,
    ]
);

#[derive(Clone, Copy)]
enum NavigationDirection {
    Left,
    Right,
    Up,
    Down,
    First,
    Last,
    PageUp,
    PageDown,
}

struct GridNavigationRow {
    list_index: usize,
    item_indices: Vec<usize>,
}

pub(crate) fn init(cx: &mut App) {
    let context = Some("FileBrowser && !editing && !trash_confirm && !which_key");
    cx.bind_keys([
        KeyBinding::new("enter", OpenSelection, context),
        KeyBinding::new("ctrl-o", OpenSelection, context),
        KeyBinding::new("alt-up", GoToParentDirectory, context),
        KeyBinding::new("alt-left", GoBackDirectory, context),
        KeyBinding::new("alt-right", GoForwardDirectory, context),
        KeyBinding::new("left", MoveSelectionLeft, context),
        KeyBinding::new("right", MoveSelectionRight, context),
        KeyBinding::new("up", MoveSelectionUp, context),
        KeyBinding::new("down", MoveSelectionDown, context),
        KeyBinding::new("shift-left", ExtendSelectionLeft, context),
        KeyBinding::new("shift-right", ExtendSelectionRight, context),
        KeyBinding::new("shift-up", ExtendSelectionUp, context),
        KeyBinding::new("shift-down", ExtendSelectionDown, context),
        KeyBinding::new("home", SelectFirstItem, context),
        KeyBinding::new("end", SelectLastItem, context),
        KeyBinding::new("pageup", SelectPreviousPage, context),
        KeyBinding::new("pagedown", SelectNextPage, context),
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

    let vim_context = Some("FileBrowser && vim && !editing && !trash_confirm && !which_key");
    cx.bind_keys([
        KeyBinding::new("o", OpenSelection, vim_context),
        KeyBinding::new("u", GoToParentDirectory, vim_context),
        KeyBinding::new("-", GoToParentDirectory, vim_context),
        KeyBinding::new("h", MoveSelectionLeft, vim_context),
        KeyBinding::new("l", MoveSelectionRight, vim_context),
        KeyBinding::new("k", MoveSelectionUp, vim_context),
        KeyBinding::new("j", MoveSelectionDown, vim_context),
        KeyBinding::new("g", StartGoPrefix, vim_context),
        KeyBinding::new("shift-g", SelectLastItem, vim_context),
        KeyBinding::new("ctrl-u", SelectPreviousPage, vim_context),
        KeyBinding::new("ctrl-d", SelectNextPage, vim_context),
        KeyBinding::new("a", AddItem, vim_context),
        KeyBinding::new("c", StartCopyPrefix, vim_context),
        KeyBinding::new("d", RequestVimTrash, vim_context),
        KeyBinding::new("shift-d", VimTrashImmediately, vim_context),
    ]);

    let copy_context = Some("FileBrowser && vim && copy_prefix && !editing && !trash_confirm");
    cx.bind_keys([
        KeyBinding::new("f", CopyFileNameText, copy_context),
        KeyBinding::new("c", CopyFilePathText, copy_context),
        KeyBinding::new("d", CopyParentDirectoryText, copy_context),
        KeyBinding::new("escape", CancelWhichKey, copy_context),
    ]);

    let go_context = Some("FileBrowser && vim && which_key && go_prefix && !editing");
    cx.bind_keys([
        KeyBinding::new("g", SelectFirstItem, go_context),
        KeyBinding::new("escape", CancelWhichKey, go_context),
    ]);

    let confirmation_context = Some("FileBrowser && vim && trash_confirm && !editing");
    cx.bind_keys([
        KeyBinding::new("y", ConfirmVimTrash, confirmation_context),
        KeyBinding::new("n", CancelVimTrash, confirmation_context),
        KeyBinding::new("escape", CancelVimTrash, confirmation_context),
    ]);
}

impl FileBrowser {
    pub(super) fn navigable_items(&self, cx: &App) -> Vec<BrowserItem> {
        let items = present_browser(self.controller.read(cx).state());
        let query = self.search_input.read(cx).value().trim().to_lowercase();
        if query.is_empty() {
            items
        } else {
            items
                .into_iter()
                .filter(|item| item.name.to_lowercase().contains(&query))
                .collect()
        }
    }

    fn grid_navigation_rows(
        items: &[BrowserItem],
        grouped: bool,
        columns: usize,
    ) -> Vec<GridNavigationRow> {
        let columns = columns.max(1);
        let mut rows = Vec::new();
        let mut list_index = 0;
        let mut start = 0;

        while start < items.len() {
            let end = if grouped {
                let category = items[start].category;
                items[start..]
                    .iter()
                    .position(|item| item.category != category)
                    .map_or(items.len(), |offset| start + offset)
            } else {
                items.len()
            };
            if grouped {
                list_index += 1;
            }
            for chunk_start in (start..end).step_by(columns) {
                let chunk_end = (chunk_start + columns).min(end);
                rows.push(GridNavigationRow {
                    list_index,
                    item_indices: (chunk_start..chunk_end).collect(),
                });
                list_index += 1;
            }
            start = end;
        }
        rows
    }

    fn grid_target(
        rows: &[GridNavigationRow],
        current: usize,
        direction: NavigationDirection,
        page_rows: usize,
    ) -> usize {
        let Some((row_index, column)) = rows.iter().enumerate().find_map(|(row, entries)| {
            entries
                .item_indices
                .iter()
                .position(|index| *index == current)
                .map(|column| (row, column))
        }) else {
            return current;
        };

        match direction {
            NavigationDirection::Left => rows[row_index]
                .item_indices
                .get(column.saturating_sub(1))
                .copied()
                .unwrap_or(current),
            NavigationDirection::Right => rows[row_index]
                .item_indices
                .get(column + 1)
                .copied()
                .unwrap_or(current),
            NavigationDirection::Up
            | NavigationDirection::Down
            | NavigationDirection::PageUp
            | NavigationDirection::PageDown => {
                let row_delta = match direction {
                    NavigationDirection::Up => -1,
                    NavigationDirection::Down => 1,
                    NavigationDirection::PageUp => -(page_rows.max(1) as isize),
                    NavigationDirection::PageDown => page_rows.max(1) as isize,
                    _ => unreachable!(),
                };
                let target_row = (row_index as isize + row_delta)
                    .clamp(0, rows.len().saturating_sub(1) as isize)
                    as usize;
                let entries = &rows[target_row].item_indices;
                entries[column.min(entries.len().saturating_sub(1))]
            }
            NavigationDirection::First => 0,
            NavigationDirection::Last => rows
                .last()
                .and_then(|row| row.item_indices.last())
                .copied()
                .unwrap_or(current),
        }
    }

    fn move_selection(
        &mut self,
        direction: NavigationDirection,
        extend: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        let items = self.navigable_items(cx);
        if items.is_empty() {
            return;
        }
        let (view_mode, active, anchor, group_by_kind) = {
            let controller = self.controller.read(cx);
            let state = controller.state();
            (
                state.view_mode(),
                state.active_selection().map(PathBuf::from),
                state.selection_anchor().map(PathBuf::from),
                state.sort().field == SortField::Kind,
            )
        };
        let current = active
            .as_ref()
            .and_then(|path| items.iter().position(|item| item.path == *path))
            .or_else(|| items.iter().position(|item| item.selected));
        let target = match (current, direction, view_mode) {
            (_, NavigationDirection::First, _) => 0,
            (_, NavigationDirection::Last, _) => items.len() - 1,
            (None, _, _) => 0,
            (
                Some(current),
                NavigationDirection::Left | NavigationDirection::Right,
                ViewMode::List,
            ) => current,
            (Some(current), direction, ViewMode::List) => {
                let page = ((self.list_scroll.viewport_bounds().size.height / px(46.)).floor()
                    as usize)
                    .max(1);
                match direction {
                    NavigationDirection::Up => current.saturating_sub(1),
                    NavigationDirection::Down => (current + 1).min(items.len() - 1),
                    NavigationDirection::PageUp => current.saturating_sub(page),
                    NavigationDirection::PageDown => (current + page).min(items.len() - 1),
                    NavigationDirection::First => 0,
                    NavigationDirection::Last => items.len() - 1,
                    NavigationDirection::Left | NavigationDirection::Right => current,
                }
            }
            (Some(current), direction, ViewMode::Grid) => {
                let rows = Self::grid_navigation_rows(&items, group_by_kind, self.grid_columns);
                let page_rows =
                    ((self.grid_scroll.viewport_bounds().size.height / px(142.)).floor() as usize)
                        .max(1);
                Self::grid_target(&rows, current, direction, page_rows)
            }
        };

        if current == Some(target)
            && !matches!(
                direction,
                NavigationDirection::First | NavigationDirection::Last
            )
        {
            return;
        }

        let target_path = items[target].path.clone();
        self.rendered_active_selection = Some(target_path.clone());
        if extend {
            let anchor = anchor
                .and_then(|path| items.iter().position(|item| item.path == path))
                .or(current)
                .unwrap_or(target);
            let (start, end) = if anchor <= target {
                (anchor, target)
            } else {
                (target, anchor)
            };
            let paths = items[start..=end]
                .iter()
                .map(|item| item.path.clone())
                .collect();
            let anchor_path = items[anchor].path.clone();
            self.controller.update(cx, |controller, cx| {
                controller.dispatch(
                    BrowserMessage::SetSelectionState {
                        paths,
                        active: target_path.clone(),
                        anchor: anchor_path,
                    },
                    cx,
                );
            });
        } else {
            self.controller.update(cx, |controller, cx| {
                controller.dispatch(
                    BrowserMessage::Select {
                        path: target_path.clone(),
                        mode: SelectionMode::Replace,
                    },
                    cx,
                );
            });
        }

        match view_mode {
            ViewMode::List => self.list_scroll.scroll_to_reveal_item(target),
            ViewMode::Grid => {
                let rows = Self::grid_navigation_rows(&items, group_by_kind, self.grid_columns);
                if let Some(row) = rows.iter().find(|row| row.item_indices.contains(&target)) {
                    self.grid_scroll.scroll_to_reveal_item(row.list_index);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn move_selection_left(
        &mut self,
        _: &MoveSelectionLeft,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.move_selection(NavigationDirection::Left, false, cx);
    }

    pub(super) fn move_selection_right(
        &mut self,
        _: &MoveSelectionRight,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.move_selection(NavigationDirection::Right, false, cx);
    }

    pub(super) fn move_selection_up(
        &mut self,
        _: &MoveSelectionUp,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.move_selection(NavigationDirection::Up, false, cx);
    }

    pub(super) fn move_selection_down(
        &mut self,
        _: &MoveSelectionDown,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.move_selection(NavigationDirection::Down, false, cx);
    }

    pub(super) fn extend_selection_left(
        &mut self,
        _: &ExtendSelectionLeft,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.move_selection(NavigationDirection::Left, true, cx);
    }

    pub(super) fn extend_selection_right(
        &mut self,
        _: &ExtendSelectionRight,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.move_selection(NavigationDirection::Right, true, cx);
    }

    pub(super) fn extend_selection_up(
        &mut self,
        _: &ExtendSelectionUp,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.move_selection(NavigationDirection::Up, true, cx);
    }

    pub(super) fn extend_selection_down(
        &mut self,
        _: &ExtendSelectionDown,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.move_selection(NavigationDirection::Down, true, cx);
    }

    pub(super) fn select_first_item(
        &mut self,
        _: &SelectFirstItem,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.cancel_which_key(cx);
        self.move_selection(NavigationDirection::First, false, cx);
    }

    pub(super) fn select_last_item(
        &mut self,
        _: &SelectLastItem,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.move_selection(NavigationDirection::Last, false, cx);
    }

    pub(super) fn select_previous_page(
        &mut self,
        _: &SelectPreviousPage,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.move_selection(NavigationDirection::PageUp, false, cx);
    }

    pub(super) fn select_next_page(
        &mut self,
        _: &SelectNextPage,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.move_selection(NavigationDirection::PageDown, false, cx);
    }

    pub(super) fn go_to_parent_directory(
        &mut self,
        _: &GoToParentDirectory,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.controller.update(cx, |controller, cx| {
            controller.dispatch(BrowserMessage::GoUp, cx);
        });
    }

    pub(super) fn go_back_directory(
        &mut self,
        _: &GoBackDirectory,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.controller.update(cx, |controller, cx| {
            controller.dispatch(BrowserMessage::GoBack, cx);
        });
    }

    pub(super) fn go_forward_directory(
        &mut self,
        _: &GoForwardDirectory,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.controller.update(cx, |controller, cx| {
            controller.dispatch(BrowserMessage::GoForward, cx);
        });
    }

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

    pub(super) fn request_vim_trash(
        &mut self,
        _: &RequestVimTrash,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.controller.read(cx).state().is_trash() {
            return;
        }
        let paths = self.controller.read(cx).selected_paths();
        if paths.is_empty() {
            return;
        }
        self.vim_trash_confirmation = Some(paths);
        cx.notify();
    }

    pub(super) fn confirm_vim_trash(
        &mut self,
        _: &ConfirmVimTrash,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(paths) = self.vim_trash_confirmation.take() else {
            return;
        };
        if self.controller.read(cx).state().is_trash() {
            cx.notify();
            return;
        }
        Self::run_operation(
            self.controller.clone(),
            FileOperation::Trash { paths },
            "Moved to Trash",
            cx,
        );
    }

    pub(super) fn cancel_vim_trash(
        &mut self,
        _: &CancelVimTrash,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.vim_trash_confirmation.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn vim_trash_immediately(
        &mut self,
        _: &VimTrashImmediately,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.controller.read(cx).state().is_trash() {
            return;
        }
        let paths = self.controller.read(cx).selected_paths();
        if paths.is_empty() {
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

    pub(super) fn add_item(
        &mut self,
        _: &AddItem,
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
        Self::show_add_item_dialog(self.controller.clone(), directory, window, cx);
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

    pub(super) fn primary_item(&self, cx: &App) -> Option<BrowserItem> {
        let controller = self.controller.read(cx);
        let state = controller.state();
        let items = present_browser(state);
        state
            .active_selection()
            .and_then(|path| items.iter().find(|entry| entry.path == path).cloned())
            .or_else(|| items.into_iter().find(|entry| entry.selected))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rift_core::domain::EntryCategory;

    use super::{FileBrowser, NavigationDirection};
    use crate::presentation::{BrowserItem, ItemIcon};

    fn item(index: usize, category: EntryCategory) -> BrowserItem {
        BrowserItem {
            path: PathBuf::from(format!("/{index}")),
            name: index.to_string(),
            detail: String::new(),
            modified: String::new(),
            size: String::new(),
            byte_len: 0,
            modified_at: None,
            kind: String::new(),
            category,
            icon: ItemIcon::File,
            is_directory: false,
            alias: false,
            selected: false,
        }
    }

    #[test]
    fn grouped_grid_navigation_preserves_columns_and_scroll_rows() {
        let items = vec![
            item(0, EntryCategory::Folder),
            item(1, EntryCategory::Folder),
            item(2, EntryCategory::Folder),
            item(3, EntryCategory::Document),
            item(4, EntryCategory::Document),
        ];
        let rows = FileBrowser::grid_navigation_rows(&items, true, 2);

        assert_eq!(
            rows.iter().map(|row| row.list_index).collect::<Vec<_>>(),
            [1, 2, 4]
        );
        assert_eq!(
            FileBrowser::grid_target(&rows, 1, NavigationDirection::Down, 1),
            2
        );
        assert_eq!(
            FileBrowser::grid_target(&rows, 2, NavigationDirection::Down, 1),
            3
        );
        assert_eq!(
            FileBrowser::grid_target(&rows, 3, NavigationDirection::Up, 1),
            2
        );
    }
}
