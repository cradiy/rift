use gpui::{Div, FontWeight, IntoElement, Stateful, div, prelude::*, px, rgba, svg};
use rift_core::{
    application::{BrowserMessage, SortSpec, ViewMode},
    ports::FileOperation,
};
use uic::{
    assets::LucideIcons,
    components::{
        context_menu::{ContextMenuAlignment, ContextMenuTrigger},
        input::Input,
    },
};

use crate::{presentation::BrowserController, ui::theme};

use super::FileBrowser;

pub(super) struct ToolbarState {
    pub(super) view_mode: ViewMode,
    pub(super) can_go_back: bool,
    pub(super) can_go_forward: bool,
    pub(super) can_go_up: bool,
    pub(super) title: String,
    pub(super) compact: bool,
    pub(super) sidebar_visible: bool,
    pub(super) sort: SortSpec,
    pub(super) has_selection: bool,
    pub(super) show_hidden_files: bool,
    pub(super) is_trash: bool,
}

impl FileBrowser {
    fn toolbar_button(id: &'static str, icon: LucideIcons) -> impl IntoElement {
        div()
            .id(id)
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_3xl()
            .cursor_pointer()
            .text_color(rgba(0xe6e5eacf))
            .hover(|style| style.bg(rgba(0xffffff14)))
            .child(svg().path(icon).size_5().text_color(rgba(0xe6e5eacf)))
    }

    fn view_toolbar_button(
        &self,
        id: &'static str,
        icon: LucideIcons,
        mode: ViewMode,
        active: bool,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let controller = self.controller.clone();
        div()
            .id(id)
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_2xl()
            .cursor_pointer()
            .when(active, |button| {
                button
                    .bg(rgba(0x55566675))
                    .border_1()
                    .border_color(rgba(0xffffff12))
            })
            .when(!active, |button| {
                button.hover(|style| style.bg(rgba(0xffffff14)))
            })
            .on_click(cx.listener(move |_, _, _, cx| {
                controller.update(cx, |controller, cx| {
                    controller.dispatch(BrowserMessage::SetViewMode(mode), cx);
                });
            }))
            .child(svg().path(icon).size_5().text_color(rgba(0xf1f0f4e8)))
    }

    fn browser_toolbar_button(
        &self,
        id: &'static str,
        icon: LucideIcons,
        message: BrowserMessage,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let controller = self.controller.clone();
        div()
            .id(id)
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_3xl()
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .hover(|style| style.bg(rgba(0xffffff14)))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        controller.update(cx, |controller: &mut BrowserController, cx| {
                            controller.dispatch(message.clone(), cx);
                        });
                    }))
            })
            .when(!enabled, |button| button.opacity(0.35))
            .child(svg().path(icon).size_5().text_color(rgba(0xe6e5eacf)))
            .into_any_element()
    }

    fn sort_toolbar_button(&self, sort: SortSpec, show_hidden_files: bool) -> gpui::AnyElement {
        let controller = self.controller.clone();
        let button = div()
            .id("sort")
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_3xl()
            .cursor_pointer()
            .hover(|style| style.bg(rgba(0xffffff14)))
            .child(
                svg()
                    .path(LucideIcons::ListFilter)
                    .size_5()
                    .text_color(rgba(0xe6e5eacf)),
            );
        ContextMenuTrigger::new(button, move |_, _| {
            Self::sort_context_menu(controller.clone(), sort, show_hidden_files)
        })
        .alignment(ContextMenuAlignment::Center)
        .gap(px(6.))
        .into_any_element()
    }

    fn new_folder_toolbar_button(
        &self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id("new-folder")
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_3xl()
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .hover(|style| style.bg(rgba(0xffffff14)))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let directory = this
                            .controller
                            .read(cx)
                            .state()
                            .current_directory()
                            .to_path_buf();
                        Self::show_new_item_dialog(
                            this.controller.clone(),
                            directory,
                            super::file_actions::NewItemKind::Directory,
                            window,
                            cx,
                        );
                    }))
            })
            .when(!enabled, |button| button.opacity(0.35))
            .child(
                svg()
                    .path(LucideIcons::FolderPlus)
                    .size_5()
                    .text_color(rgba(0xe6e5eacf)),
            )
            .into_any_element()
    }

    fn delete_toolbar_button(
        &self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id("delete")
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_3xl()
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .hover(|style| style.bg(rgba(0xffffff14)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let paths = this.controller.read(cx).selected_paths();
                        if paths.is_empty() {
                            return;
                        }
                        if this.controller.read(cx).state().is_trash() {
                            Self::show_permanent_delete_confirmation(
                                this.controller.clone(),
                                paths,
                                window,
                                cx,
                            );
                        } else {
                            Self::run_operation(
                                this.controller.clone(),
                                FileOperation::Trash { paths },
                                "Moved to Trash",
                                cx,
                            );
                        }
                    }))
            })
            .when(!enabled, |button| button.opacity(0.35))
            .child(
                svg()
                    .path(LucideIcons::Trash2)
                    .size_5()
                    .text_color(rgba(0xe6e5eacf)),
            )
            .into_any_element()
    }

    fn sidebar_expand_button(&mut self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        div()
            .id("expand-sidebar")
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_2xl()
            .cursor_pointer()
            .text_color(rgba(0xe6e5eacf))
            .hover(|style| style.bg(rgba(0xffffff14)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.set_sidebar_visible(true, cx);
            }))
            .child(
                svg()
                    .path(LucideIcons::ChevronsRight)
                    .size(px(18.))
                    .text_color(rgba(0xe6e5eacf)),
            )
            .into_any_element()
    }

    fn toolbar_group(id: &'static str, buttons: Vec<gpui::AnyElement>) -> Stateful<Div> {
        div()
            .id(id)
            .h(px(40.))
            .px(px(3.))
            .flex()
            .items_center()
            .rounded_3xl()
            .bg(rgba(0x12131a15))
            .border_1()
            .border_color(rgba(0xffffff16))
            .shadow_sm()
            .children(buttons)
    }

    pub(super) fn toolbar(
        &mut self,
        state: ToolbarState,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let sidebar_expand = (!state.sidebar_visible).then(|| self.sidebar_expand_button(cx));
        div()
            .h(px(72.))
            .px(px(19.))
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(rgba(0xffffff08))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(15.))
                    .when_some(sidebar_expand, |toolbar, button| toolbar.child(button))
                    .child(Self::toolbar_group(
                        "history-controls",
                        vec![
                            self.browser_toolbar_button(
                                "back",
                                LucideIcons::ChevronLeft,
                                BrowserMessage::GoBack,
                                state.can_go_back,
                                cx,
                            ),
                            self.browser_toolbar_button(
                                "forward",
                                LucideIcons::ChevronRight,
                                BrowserMessage::GoForward,
                                state.can_go_forward,
                                cx,
                            ),
                            self.browser_toolbar_button(
                                "up",
                                LucideIcons::ArrowUp,
                                BrowserMessage::GoUp,
                                state.can_go_up,
                                cx,
                            ),
                        ],
                    ))
                    .child(
                        div()
                            .max_w(px(230.))
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgba(0xf4f3f6ee))
                            .child(state.title),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        Self::toolbar_group(
                            "view-controls",
                            vec![
                                self.view_toolbar_button(
                                    "grid",
                                    LucideIcons::Grid2x2,
                                    ViewMode::Grid,
                                    state.view_mode == ViewMode::Grid,
                                    cx,
                                )
                                .into_any_element(),
                                self.view_toolbar_button(
                                    "list",
                                    LucideIcons::List,
                                    ViewMode::List,
                                    state.view_mode == ViewMode::List,
                                    cx,
                                )
                                .into_any_element(),
                            ],
                        )
                        .pl_3()
                        .pr_3(),
                    )
                    .when(!state.compact, |toolbar| {
                        toolbar
                            .child(Self::toolbar_group(
                                "sort-controls",
                                vec![self.sort_toolbar_button(state.sort, state.show_hidden_files)],
                            ))
                            .child(Self::toolbar_group(
                                "file-controls",
                                vec![
                                    self.new_folder_toolbar_button(!state.is_trash, cx),
                                    self.delete_toolbar_button(state.has_selection, cx),
                                    Self::toolbar_button("more", LucideIcons::Ellipsis)
                                        .into_any_element(),
                                ],
                            ))
                            .child(
                                Input::new(&self.search_input)
                                    .appearance(theme::input_appearance())
                                    .w(px(190.))
                                    .h(px(40.))
                                    .px(px(13.))
                                    .gap(px(7.))
                                    .rounded_2xl()
                                    .bg(rgba(0x12131a75))
                                    .border_1()
                                    .border_color(rgba(0xffffff0f))
                                    .text_size(px(13.))
                                    .text_color(rgba(0xe8e7ebdd))
                                    .prefix(
                                        svg()
                                            .path(LucideIcons::Search)
                                            .size(px(16.))
                                            .text_color(rgba(0xb9b7c1a3)),
                                    ),
                            )
                    })
                    .when(state.compact, |toolbar| {
                        toolbar.child(Self::toolbar_group(
                            "compact-controls",
                            vec![
                                Self::toolbar_button("compact-more", LucideIcons::Ellipsis)
                                    .into_any_element(),
                            ],
                        ))
                    }),
            )
    }
}
