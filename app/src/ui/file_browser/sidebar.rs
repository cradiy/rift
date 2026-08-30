use std::path::{Path, PathBuf};

use gpui::{
    Animation, AnimationExt as _, FontWeight, IntoElement, SharedString, div, prelude::*, px, rgba,
    svg,
};
use gpui_effects::FrostedGlass;
use rift_core::{
    application::{BrowserMessage, NavigationLoadState},
    domain::{Location, LocationKind},
};
use uic::assets::LucideIcons;

use crate::{presentation::BrowserController, ui::theme};

use super::{FileBrowser, SIDEBAR_ANIMATION_DURATION, SIDEBAR_WIDTH, sidebar_animation_easing};

impl FileBrowser {
    fn sidebar_header(&mut self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        div()
            .h(px(54.))
            .px(px(12.))
            .pt(px(8.))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pl(px(10.))
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgba(0xa39dab9c))
                    .child("Location"),
            )
            .child(
                div()
                    .id("collapse-sidebar")
                    .size(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_lg()
                    .cursor_pointer()
                    .hover(|button| button.bg(rgba(0xffffff14)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_sidebar_visible(false, cx);
                    }))
                    .child(
                        svg()
                            .path(LucideIcons::ChevronsLeft)
                            .size(px(17.))
                            .text_color(rgba(0xd8d6dee0)),
                    ),
            )
            .into_any_element()
    }

    fn section(label: &'static str) -> impl IntoElement {
        div()
            .h(px(32.))
            .px(px(22.))
            .pb(px(5.))
            .flex()
            .items_end()
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgba(0xa39dab9c))
            .child(label)
    }

    fn location_icon(kind: LocationKind) -> LucideIcons {
        match kind {
            LocationKind::Home => LucideIcons::House,
            LocationKind::Applications => LucideIcons::AppWindow,
            LocationKind::Desktop => LucideIcons::Monitor,
            LocationKind::Documents => LucideIcons::FileText,
            LocationKind::Downloads => LucideIcons::CircleArrowDown,
            LocationKind::Videos => LucideIcons::Film,
            LocationKind::Music => LucideIcons::Music,
            LocationKind::Pictures => LucideIcons::Image,
            LocationKind::Trash => LucideIcons::Trash2,
            LocationKind::FileSystem => LucideIcons::FolderRoot,
            LocationKind::Volume { removable: true } => LucideIcons::Usb,
            LocationKind::Volume { removable: false } => LucideIcons::HardDrive,
        }
    }

    fn location_item(
        location: Location,
        selected_path: Option<&Path>,
        controller: gpui::Entity<BrowserController>,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let path = location.path().to_path_buf();
        let selected = selected_path == Some(path.as_path());
        let id = SharedString::from(format!("location:{}", path.display()));
        div()
            .id(id)
            .h(px(30.))
            .mx(px(12.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(9.))
            .rounded_lg()
            .cursor_pointer()
            .text_sm()
            .font_weight(if selected {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::MEDIUM
            })
            .text_color(rgba(0xf3f1f7ed))
            .when(selected, |row| row.bg(rgba(0x3a39488f)))
            .when(!selected, |row| {
                row.hover(|style| style.bg(rgba(0xffffff14)))
            })
            .on_click(cx.listener(move |_, _, _, cx| {
                controller.update(cx, |controller, cx| {
                    controller.dispatch(BrowserMessage::Navigate(path.clone()), cx);
                });
            }))
            .child(
                svg()
                    .path(Self::location_icon(location.kind()))
                    .size(px(17.))
                    .text_color(rgba(0xf5f3f9e8)),
            )
            .child(div().min_w_0().truncate().child(location.name().to_owned()))
            .into_any_element()
    }

    pub(super) fn sidebar(&mut self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let current_directory = self
            .controller
            .read(cx)
            .state()
            .current_directory()
            .to_path_buf();
        let (location, devices, load_state) = {
            let navigation = self.navigation.read(cx);
            let state = navigation.state();
            (
                state.snapshot().locations.clone(),
                state.snapshot().devices.clone(),
                state.load_state().clone(),
            )
        };
        let selected_path = selected_location(&current_directory, &location, &devices);
        let controller = self.controller.clone();
        let header = self.sidebar_header(cx);

        let surface = FrostedGlass::with_appearance(theme::sidebar_glass())
            .h_full()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .child(
                div().size_full().flex().flex_col().child(header).child(
                    div()
                        .id("sidebar-scroll")
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .overflow_y_scroll()
                        .children(location.into_iter().map(|location| {
                            Self::location_item(
                                location,
                                selected_path.as_deref(),
                                controller.clone(),
                                cx,
                            )
                        }))
                        .child(Self::section("Devices"))
                        .children(devices.into_iter().map(|location| {
                            Self::location_item(
                                location,
                                selected_path.as_deref(),
                                controller.clone(),
                                cx,
                            )
                        }))
                        .when(
                            matches!(load_state, NavigationLoadState::Loading { .. })
                                && selected_path.is_none(),
                            |sidebar| {
                                sidebar.child(
                                    div()
                                        .px(px(22.))
                                        .py(px(8.))
                                        .text_xs()
                                        .text_color(rgba(0xc3c1c999))
                                        .child("Discovering locations…"),
                                )
                            },
                        )
                        .when_some(
                            match load_state {
                                NavigationLoadState::Failed { message } => Some(message),
                                _ => None,
                            },
                            |sidebar, message| {
                                sidebar.child(
                                    div()
                                        .mx(px(12.))
                                        .mt(px(8.))
                                        .p(px(10.))
                                        .rounded_lg()
                                        .bg(rgba(0xff453a1f))
                                        .text_xs()
                                        .text_color(rgba(0xffb4aee6))
                                        .child(message),
                                )
                            },
                        ),
                ),
            );

        let sidebar = div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .overflow_hidden()
            .border_r_1()
            .border_color(rgba(0xffffff10))
            .child(surface);

        if !self.sidebar_animated {
            return sidebar
                .left(px(if self.sidebar_visible {
                    0.0
                } else {
                    -SIDEBAR_WIDTH
                }))
                .into_any_element();
        }

        let visible = self.sidebar_visible;
        sidebar
            .with_animation(
                if visible {
                    "sidebar-expand"
                } else {
                    "sidebar-collapse"
                },
                Animation::new(SIDEBAR_ANIMATION_DURATION).with_easing(sidebar_animation_easing),
                move |sidebar, phase| {
                    let progress = if visible { phase } else { 1.0 - phase };
                    sidebar.left(px(-SIDEBAR_WIDTH * (1.0 - progress)))
                },
            )
            .into_any_element()
    }
}

fn selected_location(
    current_directory: &Path,
    favorites: &[Location],
    devices: &[Location],
) -> Option<PathBuf> {
    favorites
        .iter()
        .chain(devices)
        .filter(|location| current_directory.starts_with(location.path()))
        .max_by_key(|location| location.path().components().count())
        .map(|location| location.path().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_the_most_specific_location() {
        let favorites = vec![Location::new(
            PathBuf::from("/home/me"),
            "Home",
            LocationKind::Home,
        )];
        let devices = vec![Location::new(
            PathBuf::from("/"),
            "File System",
            LocationKind::FileSystem,
        )];

        assert_eq!(
            selected_location(Path::new("/home/me/code"), &favorites, &devices),
            Some(PathBuf::from("/home/me"))
        );
    }
}
