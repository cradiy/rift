use std::path::{Component, Path, PathBuf};

use gpui::{Div, FontWeight, IntoElement, SharedString, Stateful, div, prelude::*, px, rgba, svg};
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

const MAX_VISIBLE_BREADCRUMBS: usize = 3;

pub(super) struct ToolbarState {
    pub(super) view_mode: ViewMode,
    pub(super) can_go_back: bool,
    pub(super) can_go_forward: bool,
    pub(super) can_go_up: bool,
    pub(super) current_directory: PathBuf,
    pub(super) compact: bool,
    pub(super) sidebar_visible: bool,
    pub(super) sort: SortSpec,
    pub(super) has_selection: bool,
    pub(super) show_hidden_files: bool,
    pub(super) is_trash: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BreadcrumbSegment {
    label: String,
    path: PathBuf,
    root: bool,
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

    fn breadcrumb_segments(path: &Path) -> Vec<BreadcrumbSegment> {
        let mut segments = Vec::new();
        let mut accumulated = PathBuf::new();

        for component in path.components() {
            match component {
                Component::RootDir => {
                    accumulated.push(Path::new("/"));
                    segments.push(BreadcrumbSegment {
                        label: "File System".to_owned(),
                        path: accumulated.clone(),
                        root: true,
                    });
                }
                Component::Normal(name) => {
                    accumulated.push(name);
                    segments.push(BreadcrumbSegment {
                        label: name.to_string_lossy().into_owned(),
                        path: accumulated.clone(),
                        root: false,
                    });
                }
                Component::Prefix(_) | Component::CurDir | Component::ParentDir => {}
            }
        }

        segments
    }

    fn visible_breadcrumb_segments(path: &Path) -> Vec<BreadcrumbSegment> {
        let mut segments = Self::breadcrumb_segments(path);
        let hidden_count = segments.len().saturating_sub(MAX_VISIBLE_BREADCRUMBS);
        segments.drain(..hidden_count);
        segments
    }

    fn address_bar(
        &self,
        current_directory: PathBuf,
        is_trash: bool,
        compact: bool,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let segments = if is_trash {
            vec![BreadcrumbSegment {
                label: "Trash".to_owned(),
                path: current_directory,
                root: true,
            }]
        } else {
            Self::visible_breadcrumb_segments(&current_directory)
        };
        let segment_count = segments.len();
        let mut contents = Vec::with_capacity(segment_count.saturating_mul(2));

        for (index, segment) in segments.into_iter().enumerate() {
            if index > 0 {
                contents.push(
                    svg()
                        .path(LucideIcons::ChevronRight)
                        .size(px(12.))
                        .flex_none()
                        .text_color(rgba(0xb9c0cd56))
                        .into_any_element(),
                );
            }

            let current = index + 1 == segment_count;
            let destination = segment.path.clone();
            let controller = self.controller.clone();
            let show_root_label = current || segment_count == 1;
            let label = segment.label;
            let id = SharedString::from(format!("breadcrumb:{}", destination.display()));
            let crumb = div()
                .id(id)
                .h(px(30.))
                .min_w_0()
                .px(px(if segment.root { 7. } else { 9. }))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded_lg()
                .text_size(px(12.5))
                .font_weight(if current {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if current {
                    rgba(0xf3f5f9ed)
                } else {
                    rgba(0xc8cbd4ae)
                })
                .when(segment.root, |crumb| {
                    crumb.child(
                        svg()
                            .path(if is_trash {
                                LucideIcons::Trash2
                            } else {
                                LucideIcons::FolderRoot
                            })
                            .size(px(15.))
                            .flex_none()
                            .text_color(if current {
                                rgba(0x6bcaf1ed)
                            } else {
                                rgba(0xb8c3d3a8)
                            }),
                    )
                })
                .when(!segment.root || show_root_label, |crumb| {
                    crumb.child(
                        div()
                            .min_w_0()
                            .max_w(px(if compact { 82. } else { 118. }))
                            .truncate()
                            .child(label),
                    )
                })
                .when(current, |crumb| {
                    crumb
                        .bg(rgba(0x536a8940))
                        .border_1()
                        .border_color(rgba(0x86c9ee22))
                })
                .when(!current, |crumb| {
                    crumb
                        .cursor_pointer()
                        .hover(|style| style.bg(rgba(0xffffff11)).text_color(rgba(0xe8eaf0df)))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            controller.update(cx, |controller, cx| {
                                controller
                                    .dispatch(BrowserMessage::Navigate(destination.clone()), cx);
                            });
                        }))
                });
            contents.push(crumb.into_any_element());
        }

        div()
            .id("address-bar")
            .h(px(40.))
            .min_w_0()
            .max_w(px(if compact { 310. } else { 430. }))
            .px(px(4.))
            .flex()
            .items_center()
            .gap(px(1.))
            .overflow_hidden()
            .rounded_2xl()
            .border_1()
            .border_color(rgba(0xffffff14))
            .bg(rgba(0x11141d7d))
            .shadow(vec![
                gpui::BoxShadow::new(px(0.), px(3.), rgba(0x00000025).into())
                    .blur_radius(px(10.))
                    .spread_radius(px(-4.)),
            ])
            .hover(|style| style.border_color(rgba(0x91cde832)))
            .children(contents)
            .into_any_element()
    }

    pub(super) fn toolbar(
        &mut self,
        state: ToolbarState,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let sidebar_expand = (!state.sidebar_visible).then(|| self.sidebar_expand_button(cx));
        let address_bar =
            self.address_bar(state.current_directory, state.is_trash, state.compact, cx);
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
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .pr(px(12.))
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
                    .child(address_bar),
            )
            .child(
                div()
                    .flex_none()
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::FileBrowser;

    #[test]
    fn breadcrumb_segments_keep_each_ancestor_navigable() {
        let segments = FileBrowser::breadcrumb_segments(Path::new("/home/cradiy/logs"));

        let labels = segments
            .iter()
            .map(|segment| segment.label.as_str())
            .collect::<Vec<_>>();
        let paths = segments
            .iter()
            .map(|segment| segment.path.clone())
            .collect::<Vec<_>>();

        assert_eq!(labels, ["File System", "home", "cradiy", "logs"]);
        assert_eq!(
            paths,
            [
                PathBuf::from("/"),
                PathBuf::from("/home"),
                PathBuf::from("/home/cradiy"),
                PathBuf::from("/home/cradiy/logs"),
            ]
        );
    }

    #[test]
    fn visible_breadcrumbs_keep_the_current_directory_and_two_parents() {
        let segments =
            FileBrowser::visible_breadcrumb_segments(Path::new("/home/cradiy/code/project/src"));
        let labels = segments
            .iter()
            .map(|segment| segment.label.as_str())
            .collect::<Vec<_>>();
        let paths = segments
            .iter()
            .map(|segment| segment.path.clone())
            .collect::<Vec<_>>();

        assert_eq!(labels, ["code", "project", "src"]);
        assert_eq!(
            paths,
            [
                PathBuf::from("/home/cradiy/code"),
                PathBuf::from("/home/cradiy/code/project"),
                PathBuf::from("/home/cradiy/code/project/src"),
            ]
        );
    }
}
