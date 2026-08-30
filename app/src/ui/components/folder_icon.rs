use gpui::{
    App, IntoElement, Pixels, RenderOnce, Window, div, linear_color_stop, linear_gradient,
    prelude::*, px, rgb, rgba, svg,
};
use uic::assets::LucideIcons;

const DEFAULT_WIDTH: f32 = 76.;

/// Finder-style folder artwork used by Rift.
///
/// `size` controls the visual width and preserves the original 76:60 aspect ratio.
#[derive(IntoElement)]
pub(crate) struct FolderIcon {
    width: Pixels,
    glyph: Option<LucideIcons>,
}

impl Default for FolderIcon {
    fn default() -> Self {
        Self {
            width: px(DEFAULT_WIDTH),
            glyph: None,
        }
    }
}

impl FolderIcon {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn size(mut self, width: Pixels) -> Self {
        self.width = width.max(px(1.));
        self
    }

    pub(crate) fn glyph(mut self, glyph: Option<LucideIcons>) -> Self {
        self.glyph = glyph;
        self
    }
}

impl RenderOnce for FolderIcon {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let metrics = FolderIconMetrics::for_width(self.width);

        div()
            .relative()
            .w(self.width)
            .h(metrics.height)
            .child(
                div()
                    .absolute()
                    .left(metrics.tab_left)
                    .top_0()
                    .w(metrics.tab_width)
                    .h(metrics.tab_height)
                    .rounded_tl(metrics.large_radius)
                    .rounded_tr(metrics.medium_radius)
                    .bg(rgb(0x55caed)),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(metrics.back_top)
                    .bottom(metrics.back_bottom)
                    .rounded(metrics.large_radius)
                    .bg(rgb(0x168bbf))
                    .border(metrics.stroke)
                    .border_color(rgba(0x00000028)),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(metrics.front_top)
                    .bottom_0()
                    .rounded(metrics.front_radius)
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(rgb(0x62d5f2), 0.),
                        linear_color_stop(rgb(0x269fd5), 1.),
                    ))
                    .border(metrics.stroke)
                    .border_color(rgba(0xa1e9f8b8))
                    .shadow(vec![
                        gpui::BoxShadow::new(px(0.), metrics.shadow_y, rgba(0x00000052).into())
                            .blur_radius(metrics.shadow_blur)
                            .spread_radius(-metrics.shadow_spread),
                    ]),
            )
            .child(
                div()
                    .absolute()
                    .left(metrics.highlight_inset)
                    .right(metrics.highlight_inset)
                    .top(metrics.highlight_top)
                    .h(metrics.highlight_height)
                    .bg(rgba(0xffffff61)),
            )
            .when_some(self.glyph, |folder, glyph| {
                folder.child(
                    svg()
                        .absolute()
                        .left(metrics.glyph_left)
                        .top(metrics.glyph_top)
                        .size(metrics.glyph_size)
                        .path(glyph)
                        .text_color(rgba(0x0c719c82)),
                )
            })
    }
}

#[derive(Clone, Copy)]
struct FolderIconMetrics {
    height: Pixels,
    tab_left: Pixels,
    tab_width: Pixels,
    tab_height: Pixels,
    back_top: Pixels,
    back_bottom: Pixels,
    front_top: Pixels,
    highlight_inset: Pixels,
    highlight_top: Pixels,
    highlight_height: Pixels,
    glyph_left: Pixels,
    glyph_top: Pixels,
    glyph_size: Pixels,
    medium_radius: Pixels,
    large_radius: Pixels,
    front_radius: Pixels,
    stroke: Pixels,
    shadow_y: Pixels,
    shadow_blur: Pixels,
    shadow_spread: Pixels,
}

impl FolderIconMetrics {
    fn for_width(width: Pixels) -> Self {
        let scale = width.as_f32() / DEFAULT_WIDTH;
        let scaled = |value: f32| px(value * scale);

        Self {
            height: scaled(60.),
            tab_left: scaled(5.),
            tab_width: scaled(32.),
            tab_height: scaled(15.),
            back_top: scaled(9.),
            back_bottom: scaled(3.),
            front_top: scaled(14.),
            highlight_inset: scaled(6.),
            highlight_top: scaled(15.),
            highlight_height: scaled(1.).max(px(0.5)),
            glyph_left: scaled(27.),
            glyph_top: scaled(27.),
            glyph_size: scaled(21.),
            medium_radius: scaled(6.),
            large_radius: scaled(8.),
            front_radius: scaled(9.),
            stroke: scaled(1.).max(px(0.5)),
            shadow_y: scaled(4.),
            shadow_blur: scaled(7.),
            shadow_spread: scaled(3.),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_preserve_the_original_aspect_ratio() {
        let original = FolderIconMetrics::for_width(px(76.));
        let half = FolderIconMetrics::for_width(px(38.));

        assert_eq!(original.height, px(60.));
        assert_eq!(original.glyph_size, px(21.));
        assert_eq!(half.height, px(30.));
        assert_eq!(half.glyph_size, px(10.5));
    }
}
