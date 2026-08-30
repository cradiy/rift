use gpui::{
    AnyElement, App, Entity, FontWeight, IntoElement, Window, div, prelude::*, px, rgba, svg,
};
use gpui_effects::FrostedGlass;
use rift_core::{
    application::{BrowserMessage, SortDirection, SortField, SortSpec},
    ports::FileOperation,
};
use uic::{
    assets::LucideIcons,
    components::{
        context_menu::{
            ContextMenu, ContextMenuAppearance, ContextMenuItem, ContextMenuSurfaceState,
        },
        toast,
    },
};

use crate::{
    presentation::{BrowserController, BrowserItem, NavigationController},
    ui::{quick_look, theme},
};

use super::{FileBrowser, file_actions::NewItemKind};

impl FileBrowser {
    pub(super) fn sort_context_menu(
        controller: Entity<BrowserController>,
        sort: SortSpec,
        show_hidden_files: bool,
    ) -> ContextMenu {
        let folders_first_controller = controller.clone();
        let hidden_files_controller = controller.clone();
        Self::style_context_menu(
            ContextMenu::new()
                .item(Self::sort_field_item(
                    controller.clone(),
                    sort,
                    SortField::Name,
                    LucideIcons::Type,
                    "Name",
                ))
                .item(Self::sort_field_item(
                    controller.clone(),
                    sort,
                    SortField::Modified,
                    LucideIcons::Clock,
                    "Date Modified",
                ))
                .item(Self::sort_field_item(
                    controller.clone(),
                    sort,
                    SortField::Size,
                    LucideIcons::HardDrive,
                    "Size",
                ))
                .item(Self::sort_field_item(
                    controller.clone(),
                    sort,
                    SortField::Kind,
                    LucideIcons::FileType,
                    "Kind",
                ))
                .separator()
                .item(Self::sort_direction_item(
                    controller.clone(),
                    sort,
                    SortDirection::Ascending,
                    LucideIcons::ArrowUp,
                    "Ascending",
                ))
                .item(Self::sort_direction_item(
                    controller.clone(),
                    sort,
                    SortDirection::Descending,
                    LucideIcons::ArrowDown,
                    "Descending",
                ))
                .separator()
                .item(ContextMenuItem::action_with(
                    move |_, _| {
                        Self::checked_menu_label(
                            LucideIcons::Folder,
                            "Folders First",
                            sort.directories_first,
                        )
                    },
                    move |_, cx| {
                        folders_first_controller.update(cx, |controller, cx| {
                            controller.dispatch(
                                BrowserMessage::SetSort(SortSpec {
                                    directories_first: !sort.directories_first,
                                    ..sort
                                }),
                                cx,
                            );
                        });
                    },
                ))
                .separator()
                .item(
                    ContextMenuItem::action_with(
                        move |_, _| {
                            Self::checked_menu_label(
                                LucideIcons::Eye,
                                "Show Hidden Files",
                                show_hidden_files,
                            )
                        },
                        move |_, cx| {
                            hidden_files_controller.update(cx, |controller, cx| {
                                controller.dispatch(
                                    BrowserMessage::SetShowHiddenFiles(!show_hidden_files),
                                    cx,
                                );
                            });
                        },
                    )
                    .shortcut("Ctrl+H"),
                ),
        )
    }

    fn sort_field_item(
        controller: Entity<BrowserController>,
        sort: SortSpec,
        field: SortField,
        icon: LucideIcons,
        label: &'static str,
    ) -> ContextMenuItem {
        ContextMenuItem::action_with(
            move |_, _| Self::checked_menu_label(icon, label, sort.field == field),
            move |_, cx| {
                controller.update(cx, |controller, cx| {
                    controller.dispatch(BrowserMessage::SetSort(SortSpec { field, ..sort }), cx);
                });
            },
        )
    }

    fn sort_direction_item(
        controller: Entity<BrowserController>,
        sort: SortSpec,
        direction: SortDirection,
        icon: LucideIcons,
        label: &'static str,
    ) -> ContextMenuItem {
        ContextMenuItem::action_with(
            move |_, _| Self::checked_menu_label(icon, label, sort.direction == direction),
            move |_, cx| {
                controller.update(cx, |controller, cx| {
                    controller
                        .dispatch(BrowserMessage::SetSort(SortSpec { direction, ..sort }), cx);
                });
            },
        )
    }

    pub(super) fn blank_context_menu(
        controller: Entity<BrowserController>,
        navigation: Entity<NavigationController>,
        cx: &App,
    ) -> ContextMenu {
        let (directory, can_paste, is_trash) = {
            let controller = controller.read(cx);
            (
                controller.state().current_directory().to_path_buf(),
                !controller.clipboard().is_empty(),
                controller.state().is_trash(),
            )
        };

        if is_trash {
            let refresh_controller = controller.clone();
            return Self::style_context_menu(
                ContextMenu::new().item(
                    ContextMenuItem::action_with(
                        |_, _| Self::menu_label(LucideIcons::RefreshCw, "Refresh"),
                        move |_, cx| {
                            refresh_controller.update(cx, |controller, cx| {
                                controller.dispatch(BrowserMessage::Refresh, cx);
                            });
                            navigation.update(cx, |controller, cx| controller.refresh(cx));
                        },
                    )
                    .shortcut("F5"),
                ),
            );
        }

        let new_directory_controller = controller.clone();
        let new_directory_parent = directory.clone();
        let new_text_controller = controller.clone();
        let new_text_parent = directory.clone();
        let new_markdown_controller = controller.clone();
        let new_markdown_parent = directory.clone();
        let paste_controller = controller.clone();
        let paste_directory = directory.clone();
        let refresh_controller = controller.clone();
        let new_file_menu = ContextMenu::new()
            .item(ContextMenuItem::action_with(
                |_, _| Self::menu_label(LucideIcons::FileText, "Text File"),
                move |window, cx| {
                    Self::show_new_item_dialog(
                        new_text_controller.clone(),
                        new_text_parent.clone(),
                        NewItemKind::Text,
                        window,
                        cx,
                    );
                },
            ))
            .item(ContextMenuItem::action_with(
                |_, _| Self::menu_label(LucideIcons::FileText, "Markdown File"),
                move |window, cx| {
                    Self::show_new_item_dialog(
                        new_markdown_controller.clone(),
                        new_markdown_parent.clone(),
                        NewItemKind::Markdown,
                        window,
                        cx,
                    );
                },
            ));

        Self::style_context_menu(
            ContextMenu::new()
                .item(
                    ContextMenuItem::action_with(
                        |_, _| Self::menu_label(LucideIcons::FolderPlus, "New Folder"),
                        move |window, cx| {
                            Self::show_new_item_dialog(
                                new_directory_controller.clone(),
                                new_directory_parent.clone(),
                                NewItemKind::Directory,
                                window,
                                cx,
                            );
                        },
                    )
                    .shortcut("Ctrl+Shift+N"),
                )
                .item(ContextMenuItem::submenu_with(
                    |_, _| Self::menu_label(LucideIcons::FileText, "New File"),
                    new_file_menu,
                ))
                .separator()
                .item(
                    ContextMenuItem::action_with(
                        |_, _| Self::menu_label(LucideIcons::Copy, "Paste Items"),
                        move |_, cx| {
                            let sources = paste_controller.read(cx).clipboard().to_vec();
                            Self::run_operation(
                                paste_controller.clone(),
                                FileOperation::CopyInto {
                                    sources,
                                    directory: paste_directory.clone(),
                                },
                                "Pasted items",
                                cx,
                            );
                        },
                    )
                    .shortcut("Ctrl+V")
                    .disabled(!can_paste),
                )
                .separator()
                .item(
                    ContextMenuItem::action_with(
                        |_, _| Self::menu_label(LucideIcons::RefreshCw, "Refresh"),
                        move |_, cx| {
                            refresh_controller.update(cx, |controller, cx| {
                                controller.dispatch(BrowserMessage::Refresh, cx);
                            });
                            navigation.update(cx, |controller, cx| controller.refresh(cx));
                        },
                    )
                    .shortcut("F5"),
                ),
        )
    }

    pub(super) fn entry_context_menu(
        browser: Entity<FileBrowser>,
        controller: Entity<BrowserController>,
        entry: BrowserItem,
        cx: &App,
    ) -> ContextMenu {
        if controller.read(cx).state().is_trash() {
            return Self::trash_entry_context_menu(controller, entry, cx);
        }
        let path = entry.path.clone();
        let is_directory = entry.is_directory;
        let can_paste = !controller.read(cx).clipboard().is_empty();
        let can_quick_look = quick_look::can_preview(&entry, cx);

        let open_controller = controller.clone();
        let open_path = path.clone();
        let info_entry = entry.clone();
        let quick_look_entry = entry.clone();
        let rename_browser = browser;
        let rename_path = path.clone();
        let rename_name = entry.name.clone();
        let copy_controller = controller.clone();
        let copy_path = path.clone();
        let move_controller = controller.clone();
        let move_path = path.clone();
        let paste_controller = controller.clone();
        let paste_directory = path.clone();
        let trash_controller = controller.clone();
        let trash_path = path.clone();

        let mut menu = ContextMenu::new()
            .item(
                ContextMenuItem::action_with(
                    |_, _| Self::menu_label(LucideIcons::FolderOpen, "Open"),
                    move |_, cx| {
                        Self::open_entry(
                            open_controller.clone(),
                            open_path.clone(),
                            is_directory,
                            cx,
                        );
                    },
                )
                .shortcut("Enter"),
            )
            .item(
                ContextMenuItem::action_with(
                    |_, _| Self::menu_label(LucideIcons::PanelsTopLeft, "Open in New Window"),
                    |_, _| {},
                )
                .disabled(true),
            )
            .separator()
            .item(
                ContextMenuItem::action_with(
                    |_, _| Self::menu_label(LucideIcons::Info, "Get Info"),
                    move |window, cx| Self::show_info(info_entry.clone(), window, cx),
                )
                .shortcut("Alt+Enter"),
            )
            .item(
                ContextMenuItem::action_with(
                    |_, _| Self::menu_label(LucideIcons::Pencil, "Rename"),
                    move |window, cx| {
                        rename_browser.update(cx, |browser, cx| {
                            browser.begin_inline_rename(
                                rename_path.clone(),
                                rename_name.clone(),
                                window,
                                cx,
                            );
                        });
                    },
                )
                .shortcut("F2"),
            )
            .item(
                ContextMenuItem::action_with(
                    |_, _| Self::menu_label(LucideIcons::Eye, "Quick Look"),
                    move |window, cx| {
                        quick_look::show(quick_look_entry.clone(), window, cx);
                    },
                )
                .shortcut("Space")
                .disabled(!can_quick_look),
            )
            .separator()
            .item(
                ContextMenuItem::action_with(
                    |_, _| Self::menu_label(LucideIcons::Copy, "Copy"),
                    move |_, cx| {
                        let count = copy_controller.update(cx, |controller, _| {
                            controller.copy_selection(copy_path.clone())
                        });
                        toast::success(
                            if count == 1 {
                                "Copied 1 item".to_owned()
                            } else {
                                format!("Copied {count} items")
                            },
                            cx,
                        );
                    },
                )
                .shortcut("Ctrl+C"),
            )
            .item(ContextMenuItem::action_with(
                |_, _| Self::menu_label(LucideIcons::FolderOpen, "Move…"),
                move |window, cx| {
                    Self::show_move_dialog(move_controller.clone(), move_path.clone(), window, cx);
                },
            ));

        if is_directory {
            menu = menu.item(
                ContextMenuItem::action_with(
                    |_, _| Self::menu_label(LucideIcons::Copy, "Paste Into Folder"),
                    move |_, cx| {
                        let sources = paste_controller.read(cx).clipboard().to_vec();
                        Self::run_operation(
                            paste_controller.clone(),
                            FileOperation::CopyInto {
                                sources,
                                directory: paste_directory.clone(),
                            },
                            "Pasted items",
                            cx,
                        );
                    },
                )
                .shortcut("Ctrl+V")
                .disabled(!can_paste),
            );
        }

        Self::style_context_menu(
            menu.separator().item(
                ContextMenuItem::action_with(
                    |_, _| Self::menu_label(LucideIcons::Trash2, "Move to Trash"),
                    move |_, cx| {
                        let paths = trash_controller
                            .read(cx)
                            .selected_paths_or(trash_path.clone());
                        Self::run_operation(
                            trash_controller.clone(),
                            FileOperation::Trash { paths },
                            "Moved to Trash",
                            cx,
                        );
                    },
                )
                .shortcut("Delete")
                .danger(),
            ),
        )
    }

    fn trash_entry_context_menu(
        controller: Entity<BrowserController>,
        entry: BrowserItem,
        cx: &App,
    ) -> ContextMenu {
        let path = entry.path.clone();
        let is_directory = entry.is_directory;
        let can_quick_look = quick_look::can_preview(&entry, cx);
        let open_controller = controller.clone();
        let open_path = path.clone();
        let info_entry = entry.clone();
        let quick_look_entry = entry;
        let copy_controller = controller.clone();
        let copy_path = path.clone();
        let delete_controller = controller;
        let delete_path = path;

        Self::style_context_menu(
            ContextMenu::new()
                .item(
                    ContextMenuItem::action_with(
                        |_, _| Self::menu_label(LucideIcons::FolderOpen, "Open"),
                        move |_, cx| {
                            Self::open_entry(
                                open_controller.clone(),
                                open_path.clone(),
                                is_directory,
                                cx,
                            );
                        },
                    )
                    .shortcut("Enter"),
                )
                .item(
                    ContextMenuItem::action_with(
                        |_, _| Self::menu_label(LucideIcons::Info, "Get Info"),
                        move |window, cx| Self::show_info(info_entry.clone(), window, cx),
                    )
                    .shortcut("Alt+Enter"),
                )
                .item(
                    ContextMenuItem::action_with(
                        |_, _| Self::menu_label(LucideIcons::Eye, "Quick Look"),
                        move |window, cx| {
                            quick_look::show(quick_look_entry.clone(), window, cx);
                        },
                    )
                    .shortcut("Space")
                    .disabled(!can_quick_look),
                )
                .separator()
                .item(
                    ContextMenuItem::action_with(
                        |_, _| Self::menu_label(LucideIcons::Copy, "Copy"),
                        move |_, cx| {
                            let count = copy_controller.update(cx, |controller, _| {
                                controller.copy_selection(copy_path.clone())
                            });
                            toast::success(
                                if count == 1 {
                                    "Copied 1 item".to_owned()
                                } else {
                                    format!("Copied {count} items")
                                },
                                cx,
                            );
                        },
                    )
                    .shortcut("Ctrl+C"),
                )
                .separator()
                .item(
                    ContextMenuItem::action_with(
                        |_, _| Self::menu_label(LucideIcons::Trash2, "Delete Permanently"),
                        move |window, cx| {
                            let paths = delete_controller
                                .read(cx)
                                .selected_paths_or(delete_path.clone());
                            Self::show_permanent_delete_confirmation(
                                delete_controller.clone(),
                                paths,
                                window,
                                cx,
                            );
                        },
                    )
                    .shortcut("Shift+Delete")
                    .danger(),
                ),
        )
    }

    fn menu_label(icon: LucideIcons, label: &'static str) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(9.))
            .child(svg().path(icon).size(px(15.)).text_color(rgba(0xece8f2d6)))
            .child(label)
    }

    fn checked_menu_label(
        icon: LucideIcons,
        label: &'static str,
        checked: bool,
    ) -> impl IntoElement {
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(9.))
            .child(svg().path(icon).size(px(15.)).text_color(rgba(0xece8f2d6)))
            .child(label)
            .child(div().flex_1())
            .when(checked, |row| {
                row.child(
                    svg()
                        .path(LucideIcons::Check)
                        .size(px(14.))
                        .text_color(rgba(0x8fc5fff2)),
                )
            })
    }

    fn style_context_menu(menu: ContextMenu) -> ContextMenu {
        menu.appearance(ContextMenuAppearance {
            muted_foreground: rgba(0xc8c2cf85).into(),
            danger_foreground: rgba(0xff6b73ef).into(),
            selected_background: rgba(0x0a84ffd6).into(),
            selected_foreground: rgba(0xffffffff).into(),
            item_height: px(32.),
            item_padding_x: px(10.),
            item_radius: px(8.),
            separator: rgba(0xffffff16).into(),
            separator_margin: px(6.),
        })
        .w(px(226.))
        .max_h(px(470.))
        .p(px(6.))
        .rounded(px(18.))
        .border(px(0.))
        .bg(rgba(0x00000000))
        .text_size(px(12.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgba(0xf5f2f8ed))
        .surface(Self::context_menu_surface)
    }

    fn context_menu_surface(
        _state: ContextMenuSurfaceState,
        content: AnyElement,
        _: &mut Window,
        _: &mut App,
    ) -> AnyElement {
        FrostedGlass::with_appearance(theme::context_menu_glass())
            .rounded(px(18.))
            .shadow(vec![
                gpui::BoxShadow::new(px(0.), px(18.), rgba(0x00000072).into())
                    .blur_radius(px(42.))
                    .spread_radius(px(-8.)),
                gpui::BoxShadow::new(px(0.), px(1.), rgba(0xffffff1f).into()).blur_radius(px(1.)),
            ])
            .child(content)
            .into_any_element()
    }
}
