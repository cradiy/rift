mod image;
mod item;

use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, FocusHandle, Focusable, FontWeight, Global, KeyDownEvent, Pixels,
    Render, Window, div, prelude::*, px, relative, rgba, svg,
};
use gpui_effects::LiquidGlass;
use uic::{
    assets::LucideIcons,
    components::modal::{self, Modal},
};

use crate::presentation::BrowserItem;

use super::theme;

use self::{image::ImageQuickLookProvider, item::QuickLookItem};

#[derive(Clone, Copy, Debug)]
pub(crate) struct QuickLookSize {
    width: Pixels,
    height: Pixels,
}

impl QuickLookSize {
    pub(crate) fn new(width: Pixels, height: Pixels) -> Self {
        Self { width, height }
    }
}

/// Extension point for new preview formats. Providers decide capability and
/// render only their content; selection, keyboard handling and modal chrome
/// remain shared by the Quick Look host.
pub(crate) trait QuickLookProvider {
    fn id(&self) -> &'static str;
    fn supports(&self, item: &QuickLookItem) -> bool;
    fn preferred_size(&self, item: &QuickLookItem) -> QuickLookSize;
    fn render(&self, item: &QuickLookItem, window: &mut Window, cx: &mut App) -> AnyElement;
}

struct QuickLookRegistry {
    providers: Vec<Rc<dyn QuickLookProvider>>,
}

impl Global for QuickLookRegistry {}

pub(crate) fn init(cx: &mut App) {
    let mut registry = QuickLookRegistry {
        providers: Vec::new(),
    };
    registry.register(ImageQuickLookProvider);
    cx.set_global(registry);
}

impl QuickLookRegistry {
    fn register(&mut self, provider: impl QuickLookProvider + 'static) {
        debug_assert!(
            self.providers
                .iter()
                .all(|existing| existing.id() != provider.id()),
            "duplicate Quick Look provider id: {}",
            provider.id()
        );
        self.providers.push(Rc::new(provider));
    }

    fn provider_for(&self, item: &QuickLookItem) -> Option<Rc<dyn QuickLookProvider>> {
        self.providers
            .iter()
            .find(|provider| provider.supports(item))
            .cloned()
    }
}

struct QuickLookView {
    item: QuickLookItem,
    provider: Rc<dyn QuickLookProvider>,
    focus_handle: FocusHandle,
}

impl QuickLookView {
    fn new(
        item: QuickLookItem,
        provider: Rc<dyn QuickLookProvider>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            item,
            provider,
            focus_handle: cx.focus_handle(),
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "space" {
            modal::dismiss(window, cx);
            cx.stop_propagation();
        }
    }
}

impl Render for QuickLookView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let subtitle = format!(
            "{} · {} · {}",
            self.item.kind, self.item.size, self.item.modified
        );
        let path = self.item.path.display().to_string();

        LiquidGlass::with_appearance(theme::quick_look_glass())
            .key_context("QuickLook")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .min_h(px(280.))
            .overflow_hidden()
            .rounded(px(20.))
            .border_1()
            .border_color(rgba(0xcce8ff31))
            .shadow(vec![
                gpui::BoxShadow::new(px(0.), px(28.), rgba(0x0000009e).into())
                    .blur_radius(px(72.))
                    .spread_radius(px(-14.)),
                gpui::BoxShadow::new(px(0.), px(2.), rgba(0x55bff21a).into())
                    .blur_radius(px(22.))
                    .spread_radius(px(-6.)),
            ])
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(48.))
                    .flex_none()
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .border_b_1()
                    .border_color(rgba(0xffffff14))
                    .bg(rgba(0x11172545))
                    .child(
                        div()
                            .size(px(25.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(8.))
                            .bg(rgba(0x51bde91c))
                            .child(
                                svg()
                                    .path(LucideIcons::Eye)
                                    .size(px(15.))
                                    .text_color(rgba(0x73d0f6ef)),
                            ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgba(0xf2f5faeb))
                            .child(self.item.name.clone()),
                    )
                    .child(
                        LiquidGlass::with_appearance(theme::quick_look_control_glass())
                            .id("quick-look-close")
                            .size(px(30.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .cursor_pointer()
                            .hover(|button| button.bg(rgba(0xffffff13)))
                            .on_click(|_, window, cx| {
                                modal::dismiss(window, cx);
                                cx.stop_propagation();
                            })
                            .child(
                                svg()
                                    .path(LucideIcons::X)
                                    .size(px(14.))
                                    .text_color(rgba(0xf0f2f7c9)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(12.))
                    .child(self.provider.render(&self.item, window, cx)),
            )
            .child(
                div()
                    .h(px(38.))
                    .flex_none()
                    .w_full()
                    .min_w_0()
                    .px(px(15.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .border_t_1()
                    .border_color(rgba(0xffffff10))
                    .bg(rgba(0x0e14213b))
                    .text_size(px(11.5))
                    .text_color(rgba(0xc5c7d09c))
                    .child(subtitle)
                    .child(div().flex_1())
                    .child(div().max_w(relative(0.58)).truncate().child(path)),
            )
    }
}

impl Focusable for QuickLookView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

pub(crate) fn can_preview(entry: &BrowserItem, cx: &App) -> bool {
    let item = QuickLookItem::from(entry.clone());
    cx.global::<QuickLookRegistry>()
        .provider_for(&item)
        .is_some()
}

pub(crate) fn show(entry: BrowserItem, window: &mut Window, cx: &mut App) {
    let item = QuickLookItem::from(entry);
    let Some(provider) = cx.global::<QuickLookRegistry>().provider_for(&item) else {
        return;
    };
    let size = provider.preferred_size(&item);
    let preview = cx.new(|cx| QuickLookView::new(item, provider, cx));
    let preview_focus = preview.read(cx).focus_handle.clone();

    let modal = Modal::view(preview)
        .hide_footer()
        .ok_on_enter(false)
        .w(size.width)
        .h(size.height)
        .max_w(relative(0.92))
        .max_h(relative(0.88));

    modal::show(theme::style_quick_look_modal(modal), window, cx);
    window.focus(&preview_focus, cx);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rift_core::domain::EntryCategory;

    use super::*;

    fn item(category: EntryCategory, is_directory: bool) -> QuickLookItem {
        QuickLookItem {
            path: PathBuf::from("preview.png"),
            name: "preview.png".to_owned(),
            kind: "Image".to_owned(),
            size: "1 KB".to_owned(),
            modified: "—".to_owned(),
            byte_len: 1024,
            category,
            is_directory,
        }
    }

    #[test]
    fn image_provider_rejects_directories_and_non_images() {
        let provider = ImageQuickLookProvider;
        assert!(provider.supports(&item(EntryCategory::Image, false)));
        assert!(!provider.supports(&item(EntryCategory::Image, true)));
        assert!(!provider.supports(&item(EntryCategory::Document, false)));
    }
}
