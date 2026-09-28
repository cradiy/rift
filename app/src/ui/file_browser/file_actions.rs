use std::path::{Component, Path, PathBuf};

use gpui::{
    App, AppContext, Entity, Focusable, FontWeight, IntoElement, Styled, Window, div, prelude::*,
    px, rgba, svg,
};
use rift_core::{
    application::BrowserMessage,
    ports::{FileOperation, FileOperationResult, FileSystemError},
};
use uic::{
    assets::LucideIcons,
    components::{
        input::{Input, TextInput},
        modal::{self, Modal},
        toast,
    },
};

use crate::{
    presentation::{BrowserController, BrowserItem, present_browser},
    ui::theme,
};

use super::FileBrowser;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NewItemKind {
    Directory,
    Text,
    Markdown,
}

fn parse_add_item_name(value: &str) -> Result<(String, NewItemKind), &'static str> {
    let value = value.trim();
    let (name, kind) = if let Some(name) = value.strip_suffix('/') {
        (name.trim(), NewItemKind::Directory)
    } else {
        (value, NewItemKind::Text)
    };
    validate_file_name(name)?;
    Ok((name.to_owned(), kind))
}

impl FileBrowser {
    pub(super) fn open_entry(
        controller: Entity<BrowserController>,
        path: PathBuf,
        is_directory: bool,
        cx: &mut App,
    ) {
        if is_directory {
            controller.update(cx, |controller, cx| {
                controller.dispatch(BrowserMessage::Navigate(path), cx);
            });
            return;
        }

        let path_for_open = path.clone();
        let task = cx.background_spawn(async move { rift_platform::open_path(&path_for_open) });
        cx.spawn(async move |cx| {
            let result = task.await;
            cx.update(|cx| {
                if let Err(error) = result {
                    log::warn!("{error}");
                    toast::error(format!("Could not open {}", path.display()), cx);
                }
            });
        })
        .detach();
    }

    pub(super) fn show_new_item_dialog(
        controller: Entity<BrowserController>,
        parent: PathBuf,
        kind: NewItemKind,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (title, initial, ok_label) = match kind {
            NewItemKind::Directory => ("New Folder", "untitled folder", "Create"),
            NewItemKind::Text => ("New Text File", "Untitled.txt", "Create"),
            NewItemKind::Markdown => ("New Markdown File", "Untitled.md", "Create"),
        };
        let input = cx.new(|cx| TextInput::new(cx).initial_value(initial));
        let input_for_content = input.clone();
        let input_for_submit = input.clone();
        let modal = Modal::new(move |_, _| Self::name_field(input_for_content.clone(), "Name"))
            .title_text(title)
            .ok_label(ok_label)
            .cancel_label("Cancel")
            .on_ok(move |_, cx| {
                let name = input_for_submit.read(cx).value().trim().to_owned();
                if let Err(message) = validate_file_name(&name) {
                    toast::error(message, cx);
                    return false;
                }
                let path = parent.join(name);
                let operation = match kind {
                    NewItemKind::Directory => FileOperation::CreateDirectory { path },
                    NewItemKind::Text | NewItemKind::Markdown => FileOperation::CreateFile { path },
                };
                Self::run_operation(controller.clone(), operation, "Item created", cx);
                true
            });
        modal::show(theme::style_modal(modal.w(px(430.))), window, cx);
        window.focus(&input.read(cx).focus_handle(cx), cx);
    }

    pub(super) fn show_add_item_dialog(
        controller: Entity<BrowserController>,
        parent: PathBuf,
        window: &mut Window,
        cx: &mut App,
    ) {
        let input = cx.new(|cx| TextInput::new(cx).placeholder("name.txt or folder/"));
        let input_for_content = input.clone();
        let input_for_submit = input.clone();
        let modal = Modal::new(move |_, _| {
            div()
                .flex()
                .flex_col()
                .gap(px(9.))
                .child(Self::name_field(input_for_content.clone(), "Name"))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(rgba(0xb9bdc997))
                        .child("End the name with / to create a folder."),
                )
        })
        .title_text("Add Item")
        .ok_label("Create")
        .cancel_label("Cancel")
        .on_ok(move |_, cx| {
            let value = input_for_submit.read(cx).value();
            let (name, kind) = match parse_add_item_name(&value) {
                Ok(item) => item,
                Err(message) => {
                    toast::error(message, cx);
                    return false;
                }
            };
            let path = parent.join(name);
            let operation = match kind {
                NewItemKind::Directory => FileOperation::CreateDirectory { path },
                NewItemKind::Text | NewItemKind::Markdown => FileOperation::CreateFile { path },
            };
            Self::run_operation(controller.clone(), operation, "Item created", cx);
            true
        });
        modal::show(theme::style_modal(modal.w(px(430.))), window, cx);
        window.focus(&input.read(cx).focus_handle(cx), cx);
    }

    pub(super) fn show_move_dialog(
        controller: Entity<BrowserController>,
        fallback: PathBuf,
        window: &mut Window,
        cx: &mut App,
    ) {
        let input = cx.new(|cx| TextInput::new(cx).placeholder("/path/to/destination"));
        let input_for_content = input.clone();
        let input_for_submit = input.clone();
        let modal = Modal::new(move |_, _| {
            div()
                .flex()
                .flex_col()
                .gap(px(9.))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgba(0xc7c8d0b8))
                        .child("Destination folder"),
                )
                .child(Self::styled_input(&input_for_content))
        })
        .title_text("Move Items")
        .ok_label("Move")
        .cancel_label("Cancel")
        .on_ok(move |_, cx| {
            let value = input_for_submit.read(cx).value().trim().to_owned();
            if value.is_empty() {
                toast::error("Choose a destination folder", cx);
                return false;
            }
            let directory = PathBuf::from(value);
            let sources = {
                let controller = controller.read(cx);
                let selected = controller.selected_paths();
                if selected.contains(&fallback) && !selected.is_empty() {
                    selected
                } else {
                    vec![fallback.clone()]
                }
            };
            Self::run_operation(
                controller.clone(),
                FileOperation::MoveInto { sources, directory },
                "Items moved",
                cx,
            );
            true
        });
        modal::show(theme::style_modal(modal.w(px(500.))), window, cx);
        window.focus(&input.read(cx).focus_handle(cx), cx);
    }

    pub(super) fn show_info(entry: BrowserItem, window: &mut Window, cx: &mut App) {
        let icon = if entry.is_directory {
            LucideIcons::FolderOpen
        } else {
            LucideIcons::File
        };
        let title = format!("{} Info", entry.name);
        let modal = Modal::new(move |_, _| {
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(13.))
                        .child(svg().path(icon).size(px(42.)).text_color(rgba(0x48b8e8e8)))
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(entry.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(rgba(0xbfc2cc9e))
                                        .child(entry.kind.clone()),
                                ),
                        ),
                )
                .child(
                    div()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(rgba(0xffffff16))
                        .overflow_hidden()
                        .child(Self::info_row("Size", entry.size.clone(), false))
                        .child(Self::info_row("Modified", entry.modified.clone(), true))
                        .child(Self::info_row(
                            "Where",
                            entry.path.display().to_string(),
                            true,
                        )),
                )
        })
        .title_text(title)
        .ok_label("Done")
        .cancel_label("Cancel");
        modal::show(theme::style_modal(modal.w(px(480.))), window, cx);
    }

    pub(super) fn show_permanent_delete_confirmation(
        controller: Entity<BrowserController>,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if paths.is_empty() {
            return;
        }
        let count = paths.len();
        let message = if count == 1 {
            "This item will be permanently deleted. This action cannot be undone.".to_owned()
        } else {
            format!(
                "These {count} items will be permanently deleted. This action cannot be undone."
            )
        };
        let modal = Modal::new(move |_, _| {
            div()
                .flex()
                .items_start()
                .gap(px(13.))
                .child(
                    div()
                        .size(px(38.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(rgba(0xff453a24))
                        .child(
                            svg()
                                .path(LucideIcons::Trash2)
                                .size(px(20.))
                                .text_color(rgba(0xff777df2)),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .pt(px(2.))
                        .text_size(px(13.))
                        .text_color(rgba(0xe5e3e9d6))
                        .child(message.clone()),
                )
        })
        .title_text("Delete Permanently?")
        .ok_button(|_, _| {
            div()
                .h(px(34.))
                .px(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .bg(rgba(0xe5484ded))
                .hover(|button| button.bg(rgba(0xf2555aef)))
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgba(0xffffffff))
                .child("Delete")
        })
        .cancel_label("Cancel")
        .ok_on_enter(false)
        .on_ok(move |_, cx| {
            Self::run_operation(
                controller.clone(),
                FileOperation::PurgeTrash {
                    paths: paths.clone(),
                },
                "Permanently deleted",
                cx,
            );
            true
        });
        modal::show(theme::style_modal(modal.w(px(460.))), window, cx);
    }

    fn name_field(input: Entity<TextInput>, label: &'static str) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(9.))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgba(0xc7c8d0b8))
                    .child(label),
            )
            .child(Self::styled_input(&input))
    }

    fn styled_input(input: &Entity<TextInput>) -> Input {
        Input::new(input)
            .appearance(theme::input_appearance())
            .h(px(38.))
            .px(px(11.))
            .rounded(px(8.))
            .border_color(rgba(0xffffff24))
            .bg(rgba(0x121520d9))
            .text_size(px(13.))
            .text_color(rgba(0xf4f3f7ed))
    }

    fn info_row(label: &'static str, value: String, border: bool) -> impl IntoElement {
        div()
            .min_h(px(38.))
            .px(px(11.))
            .py(px(8.))
            .flex()
            .items_start()
            .when(border, |row| {
                row.border_t_1().border_color(rgba(0xffffff12))
            })
            .text_size(px(12.))
            .child(
                div()
                    .w(px(78.))
                    .flex_none()
                    .text_color(rgba(0xbfc2cc99))
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(rgba(0xeeeef2df))
                    .child(value),
            )
    }

    pub(super) fn run_operation(
        controller: Entity<BrowserController>,
        operation: FileOperation,
        success_message: &'static str,
        cx: &mut App,
    ) {
        let removed_paths = match &operation {
            FileOperation::Trash { paths } | FileOperation::PurgeTrash { paths } => {
                Some(paths.clone())
            }
            _ => None,
        };
        let selection_after_removal = removed_paths.as_deref().and_then(|removed_paths| {
            let ordered_paths = present_browser(controller.read(cx).state())
                .into_iter()
                .map(|item| item.path)
                .collect::<Vec<_>>();
            successor_after_removal(&ordered_paths, removed_paths)
        });
        let select_affected_path = matches!(
            &operation,
            FileOperation::Rename { .. }
                | FileOperation::CreateDirectory { .. }
                | FileOperation::CreateFile { .. }
        );
        let task = controller.update(cx, |controller, cx| controller.perform(operation, cx));
        cx.spawn(async move |cx| {
            let result = task.await;
            cx.update(|cx| match result {
                Ok(FileOperationResult { affected_paths }) => {
                    controller.update(cx, |controller, cx| {
                        if let Some(paths) = removed_paths.as_deref() {
                            controller.forget_clipboard_paths(paths);
                        }
                        let message = if let Some(path) = selection_after_removal {
                            BrowserMessage::RefreshSelecting(path)
                        } else if select_affected_path {
                            affected_paths
                                .into_iter()
                                .next()
                                .map_or(BrowserMessage::Refresh, BrowserMessage::RefreshSelecting)
                        } else {
                            BrowserMessage::Refresh
                        };
                        controller.dispatch(message, cx);
                    });
                    toast::success(success_message, cx);
                }
                Err(FileSystemError { message, .. }) => toast::error(message, cx),
            });
        })
        .detach();
    }
}

fn successor_after_removal(
    ordered_paths: &[PathBuf],
    removed_paths: &[PathBuf],
) -> Option<PathBuf> {
    let last_removed = ordered_paths
        .iter()
        .enumerate()
        .filter_map(|(index, path)| removed_paths.contains(path).then_some(index))
        .next_back()?;

    ordered_paths[last_removed + 1..]
        .iter()
        .find(|path| !removed_paths.contains(path))
        .or_else(|| {
            ordered_paths[..last_removed]
                .iter()
                .rev()
                .find(|path| !removed_paths.contains(path))
        })
        .cloned()
}

pub(super) fn validate_file_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("Name cannot be empty");
    }
    if name == "." || name == ".." {
        return Err("Choose a different name");
    }
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err("Name cannot contain a path separator");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{NewItemKind, parse_add_item_name, successor_after_removal};

    #[test]
    fn add_item_uses_a_trailing_slash_only_for_directories() {
        assert_eq!(
            parse_add_item_name("notes.md"),
            Ok(("notes.md".to_owned(), NewItemKind::Text))
        );
        assert_eq!(
            parse_add_item_name("Projects/"),
            Ok(("Projects".to_owned(), NewItemKind::Directory))
        );
        assert!(parse_add_item_name("nested/file/").is_err());
        assert!(parse_add_item_name("/").is_err());
    }

    #[test]
    fn deletion_selects_the_next_item_or_falls_back_to_the_previous_one() {
        let paths = ["a", "b", "c", "d"].map(PathBuf::from);

        assert_eq!(
            successor_after_removal(&paths, &[PathBuf::from("b")]),
            Some(PathBuf::from("c"))
        );
        assert_eq!(
            successor_after_removal(&paths, &[PathBuf::from("d")]),
            Some(PathBuf::from("c"))
        );
        assert_eq!(
            successor_after_removal(&paths, &[PathBuf::from("b"), PathBuf::from("c")]),
            Some(PathBuf::from("d"))
        );
    }
}
