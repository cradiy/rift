use gpui::{
    AnyElement, App, ImageSource, IntoElement, ObjectFit, Window, color_svg, div, img, prelude::*,
    px, rgba, svg,
};
use rift_core::domain::EntryCategory;
use uic::assets::LucideIcons;

use super::{QuickLookItem, QuickLookProvider, QuickLookSize};

pub(super) struct ImageQuickLookProvider;

impl QuickLookProvider for ImageQuickLookProvider {
    fn id(&self) -> &'static str {
        "image"
    }

    fn supports(&self, item: &QuickLookItem) -> bool {
        !item.is_directory && item.category == EntryCategory::Image
    }

    fn preferred_size(&self, _: &QuickLookItem) -> QuickLookSize {
        QuickLookSize::new(px(920.), px(680.))
    }

    fn render(&self, item: &QuickLookItem, _window: &mut Window, _cx: &mut App) -> AnyElement {
        let is_svg = item
            .path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
        let preview = if is_svg
            && item.byte_len > 0
            && let Some(external_path) = item.path.to_str()
        {
            color_svg()
                .external_path(external_path.to_owned())
                .current_color(rgba(0xe4e7eef5))
                .size_full()
                .into_any_element()
        } else if is_svg {
            unavailable_preview().into_any_element()
        } else {
            img(ImageSource::from(item.path.clone()))
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        };

        div()
            .relative()
            .size_full()
            .min_h(px(280.))
            .overflow_hidden()
            .rounded(px(11.))
            .border_1()
            .border_color(rgba(0xffffff14))
            .bg(rgba(0x0a0c12ed))
            .child(preview)
            .into_any_element()
    }
}

fn unavailable_preview() -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .text_size(px(12.))
        .text_color(rgba(0xc2c6d19a))
        .child(
            svg()
                .path(LucideIcons::ImageOff)
                .size(px(34.))
                .text_color(rgba(0xb7bdc978)),
        )
        .child("Preview unavailable")
}
