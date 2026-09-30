use std::{cell::Cell, path::PathBuf, rc::Rc, sync::Arc};

use gpui::{
    App, DragAction, DragEnd, DragFailure, Entity, ExternalPaths, FontWeight, IntoElement, Render,
    SharedString, Window, div, prelude::*, px, rgba, svg,
};
use rift_core::{
    application::BrowserMessage,
    domain::EntryCategory,
    ports::{FileOperation, FileSystemError, TransferKind, TransferRequest},
};
use uic::{assets::LucideIcons, components::toast};

use crate::{
    presentation::{
        BrowserController, BrowserItem, ItemIcon, SharedFileClipboard, present_browser,
        start_browser_transfer,
    },
    ui::components::{FolderIcon, ImageThumbnail, ImageThumbnailLayout},
};

use super::file_actions::successor_after_removal;

#[derive(Clone)]
pub(crate) struct FileDrag {
    paths: Vec<PathBuf>,
    source_directory: PathBuf,
    source_controller: Entity<BrowserController>,
    visual: BrowserItem,
    native: Rc<Cell<bool>>,
    copy_requested: Rc<Cell<bool>>,
    clipboard: SharedFileClipboard,
    clipboard_generation: u64,
}

impl FileDrag {
    pub(crate) fn for_entry(
        entry: BrowserItem,
        controller: Entity<BrowserController>,
        cx: &App,
    ) -> Self {
        let source_directory = controller
            .read(cx)
            .state()
            .current_directory()
            .to_path_buf();
        let paths = controller.read(cx).selected_paths_or(entry.path.clone());
        let clipboard = controller.read(cx).clipboard_handle();
        let clipboard_generation = clipboard.generation();
        Self {
            paths,
            source_directory,
            source_controller: controller,
            visual: entry,
            native: Rc::new(Cell::new(false)),
            copy_requested: Rc::new(Cell::new(false)),
            clipboard,
            clipboard_generation,
        }
    }

    pub(crate) fn is_native(&self) -> bool {
        self.native.get()
    }

    pub(crate) fn mark_native(&self) {
        self.native.set(true);
    }

    pub(crate) fn update_copy_modifier(&self, control: bool) {
        self.copy_requested.set(control);
    }

    pub(crate) fn external_paths(&self) -> Arc<[PathBuf]> {
        self.paths.clone().into()
    }

    pub(crate) fn prepare_source_selection(&self, cx: &mut App) {
        if self.paths.len() != 1 {
            return;
        }
        let path = self.paths[0].clone();
        if self
            .source_controller
            .read(cx)
            .state()
            .selection()
            .contains(&path)
        {
            return;
        }
        self.source_controller.update(cx, |controller, cx| {
            controller.dispatch(
                BrowserMessage::Select {
                    path,
                    mode: rift_core::application::SelectionMode::Replace,
                },
                cx,
            );
        });
    }

    fn count(&self) -> usize {
        self.paths.len()
    }

    fn title(&self) -> SharedString {
        if self.paths.len() == 1 {
            self.paths[0]
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.paths[0].display().to_string())
                .into()
        } else {
            format!("{} items", self.paths.len()).into()
        }
    }
}

pub(crate) struct FileDragPreview {
    title: SharedString,
    count: usize,
    visual: BrowserItem,
}

pub(crate) fn file_drag_preview(drag: &FileDrag, cx: &mut App) -> Entity<FileDragPreview> {
    cx.new(|_| FileDragPreview {
        title: drag.title(),
        count: drag.count(),
        visual: drag.visual.clone(),
    })
}

impl Render for FileDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let visual = if self.visual.is_directory {
            FolderIcon::new()
                .size(px(38.))
                .glyph(folder_glyph(self.visual.icon))
                .into_any_element()
        } else if self.visual.category == EntryCategory::Image {
            ImageThumbnail::new(
                self.visual.path.clone(),
                self.visual.modified_at,
                self.visual.byte_len,
                ImageThumbnailLayout::List,
            )
            .into_any_element()
        } else {
            svg()
                .path(LucideIcons::File)
                .size(px(31.))
                .text_color(category_accent(self.visual.category))
                .into_any_element()
        };
        div()
            .w(px(238.))
            .h(px(54.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded_xl()
            .border_1()
            .border_color(rgba(0x78cce85c))
            .bg(rgba(0x202632f2))
            .shadow_lg()
            .child(
                div()
                    .relative()
                    .w(px(40.))
                    .h(px(34.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(visual)
                    .when(self.count > 1, |icon| {
                        icon.child(
                            div()
                                .absolute()
                                .right(px(-2.))
                                .bottom(px(-3.))
                                .min_w(px(17.))
                                .h(px(17.))
                                .px(px(4.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .border_1()
                                .border_color(rgba(0x9ee3fa80))
                                .bg(rgba(0x167fa9f2))
                                .text_size(px(9.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgba(0xffffffff))
                                .child(self.count.to_string()),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgba(0xf3f5f8ed))
                            .child(self.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(rgba(0xaeb7c6a8))
                            .child(if self.count == 1 {
                                "Move · Ctrl to copy".to_owned()
                            } else {
                                format!("Move {} items · Ctrl to copy", self.count)
                            }),
                    ),
            )
    }
}

fn folder_glyph(icon: ItemIcon) -> Option<LucideIcons> {
    match icon {
        ItemIcon::Folder | ItemIcon::File => None,
        ItemIcon::Application => Some(LucideIcons::AppWindow),
        ItemIcon::Desktop => Some(LucideIcons::Monitor),
        ItemIcon::Document => Some(LucideIcons::FileText),
        ItemIcon::Download => Some(LucideIcons::CircleArrowDown),
        ItemIcon::Videos => Some(LucideIcons::Film),
        ItemIcon::Music => Some(LucideIcons::Music),
        ItemIcon::Picture => Some(LucideIcons::Image),
    }
}

fn category_accent(category: EntryCategory) -> gpui::Rgba {
    match category {
        EntryCategory::Folder => rgba(0x50c8eff0),
        EntryCategory::Application => rgba(0x8ca7fff0),
        EntryCategory::Document => rgba(0x8bcdf0e8),
        EntryCategory::Image => rgba(0x60d4c3ee),
        EntryCategory::Audio => rgba(0xc493f3ec),
        EntryCategory::Video => rgba(0x7faaf5ec),
        EntryCategory::Archive => rgba(0xe9b76cea),
        EntryCategory::Code => rgba(0x80c98eea),
        EntryCategory::Alias => rgba(0x99bce2e6),
        EntryCategory::Other => rgba(0xb8bbc5d6),
    }
}

pub(crate) fn drop_files_into(
    drag: &FileDrag,
    directory: PathBuf,
    target_controller: Entity<BrowserController>,
    window: &Window,
    cx: &mut App,
) {
    let copy = window.modifiers().control || drag.copy_requested.get();
    if let Some(message) = invalid_drop_message(drag, &directory, copy) {
        toast::error(message, cx);
        return;
    }

    start_browser_transfer(
        &drag.source_controller,
        TransferRequest {
            kind: if copy {
                TransferKind::Copy
            } else {
                TransferKind::Move
            },
            sources: drag.paths.clone(),
            directory,
        },
        vec![target_controller.downgrade()],
        cx,
    );
}

pub(crate) fn drop_external_files_into(
    paths: &ExternalPaths,
    directory: PathBuf,
    target_controller: Entity<BrowserController>,
    cx: &mut App,
) {
    start_browser_transfer(
        &target_controller,
        TransferRequest {
            kind: TransferKind::Copy,
            sources: paths.paths().to_vec(),
            directory,
        },
        Vec::new(),
        cx,
    );
}

pub(crate) fn finish_file_drag(outcome: DragEnd, drag: &FileDrag, cx: &mut App) {
    match outcome {
        DragEnd::ExternalDropped {
            action: DragAction::Move,
        } => {
            let ordered_paths = present_browser(drag.source_controller.read(cx).state())
                .into_iter()
                .map(|item| item.path)
                .collect::<Vec<_>>();
            let selection_after_move = successor_after_removal(&ordered_paths, &drag.paths);
            let moved_paths = drag.paths.clone();
            drag.clipboard
                .complete_move(drag.clipboard_generation, &moved_paths, cx);
            drag.source_controller.update(cx, |controller, cx| {
                let message = selection_after_move
                    .map_or(BrowserMessage::Refresh, BrowserMessage::RefreshSelecting);
                controller.dispatch(message, cx);
            });
            toast::success(item_count_message("Moved", drag.paths.len()), cx);
        }
        DragEnd::ExternalDropped {
            action: DragAction::Copy,
        } => toast::success(item_count_message("Copied", drag.paths.len()), cx),
        DragEnd::ExternalDropped {
            action: DragAction::Link,
        } => toast::success(item_count_message("Linked", drag.paths.len()), cx),
        DragEnd::Failed(failure) => toast::error(drag_failure_message(failure), cx),
        DragEnd::Dropped { .. } | DragEnd::Unaccepted | DragEnd::Cancelled => {}
    }
}

fn drag_failure_message(failure: DragFailure) -> &'static str {
    match failure {
        DragFailure::TimedOut => "File drag timed out",
        DragFailure::Transfer => "Unable to transfer the file list",
        DragFailure::Protocol => "The system file drag failed",
    }
}

fn item_count_message(action: &str, count: usize) -> String {
    if count == 1 {
        format!("{action} 1 item")
    } else {
        format!("{action} {count} items")
    }
}

pub(crate) fn trash_dragged_files(drag: &FileDrag, cx: &mut App) {
    let source_controller = drag.source_controller.clone();
    let count = drag.paths.len();
    let ordered_paths = present_browser(source_controller.read(cx).state())
        .into_iter()
        .map(|item| item.path)
        .collect::<Vec<_>>();
    let selection_after_removal = successor_after_removal(&ordered_paths, &drag.paths);
    let operation = FileOperation::Trash {
        paths: drag.paths.clone(),
    };
    let removed_paths = drag.paths.clone();
    let task = source_controller.update(cx, |controller, cx| controller.perform(operation, cx));

    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| match result {
            Ok(_) => {
                source_controller.update(cx, |controller, cx| {
                    controller.forget_clipboard_paths(&removed_paths, cx);
                    let message = selection_after_removal
                        .map_or(BrowserMessage::Refresh, BrowserMessage::RefreshSelecting);
                    controller.dispatch(message, cx);
                });
                toast::success(
                    if count == 1 {
                        "Moved 1 item to Trash".to_owned()
                    } else {
                        format!("Moved {count} items to Trash")
                    },
                    cx,
                );
            }
            Err(FileSystemError { message, .. }) => toast::error(message, cx),
        });
    })
    .detach();
}

fn invalid_drop_message(
    drag: &FileDrag,
    directory: &std::path::Path,
    copy: bool,
) -> Option<String> {
    invalid_drop_parts(&drag.paths, &drag.source_directory, directory, copy)
}

fn invalid_drop_parts(
    paths: &[PathBuf],
    source_directory: &std::path::Path,
    directory: &std::path::Path,
    copy: bool,
) -> Option<String> {
    if !copy && directory == source_directory {
        return Some("The items are already in this folder".to_owned());
    }
    if paths.iter().any(|path| directory.starts_with(path)) {
        return Some("A folder cannot be copied or moved into itself".to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(paths: &[&str], source_directory: &str) -> (Vec<PathBuf>, PathBuf) {
        (
            paths.iter().map(PathBuf::from).collect(),
            PathBuf::from(source_directory),
        )
    }

    #[test]
    fn rejects_default_move_to_the_same_directory() {
        let (paths, source_directory) = drag(&["/home/me/a.txt"], "/home/me");
        assert!(
            invalid_drop_parts(
                &paths,
                &source_directory,
                std::path::Path::new("/home/me"),
                false
            )
            .is_some()
        );
        assert!(
            invalid_drop_parts(
                &paths,
                &source_directory,
                std::path::Path::new("/home/me"),
                true
            )
            .is_none()
        );
    }

    #[test]
    fn rejects_a_directory_descendant_as_the_target() {
        let (paths, source_directory) = drag(&["/home/me/photos"], "/home/me");
        assert!(
            invalid_drop_parts(
                &paths,
                &source_directory,
                std::path::Path::new("/home/me/photos/2026"),
                false
            )
            .is_some()
        );
    }
}
