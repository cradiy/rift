use std::{hash::Hash, path::PathBuf, sync::Arc, time::SystemTime};

use gpui::{
    App, Asset, ImageCacheError, ImageSource, IntoElement, ObjectFit, Pixels, RenderImage,
    RenderOnce, Window, color_svg, div, img, prelude::*, px, rgba, svg,
};
use image::{Frame, ImageReader};
use uic::assets::LucideIcons;

const THUMBNAIL_EDGE: u32 = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImageThumbnailLayout {
    Grid,
    List,
}

impl ImageThumbnailLayout {
    fn size(self) -> (Pixels, Pixels) {
        match self {
            Self::Grid => (px(76.), px(60.)),
            Self::List => (px(32.), px(32.)),
        }
    }

    fn corner_radius(self) -> Pixels {
        match self {
            Self::Grid => px(8.),
            Self::List => px(5.),
        }
    }

    fn placeholder_icon_size(self) -> Pixels {
        match self {
            Self::Grid => px(24.),
            Self::List => px(17.),
        }
    }
}

/// A reusable file thumbnail. Raster images are decoded off the UI thread and
/// reduced before entering GPUI's image cache, while SVGs retain vector loading.
#[derive(IntoElement)]
pub(crate) struct ImageThumbnail {
    source: ThumbnailSource,
    layout: ImageThumbnailLayout,
}

impl ImageThumbnail {
    pub(crate) fn new(
        path: PathBuf,
        modified: Option<SystemTime>,
        byte_len: u64,
        layout: ImageThumbnailLayout,
    ) -> Self {
        Self {
            source: ThumbnailSource {
                path,
                modified,
                byte_len,
            },
            layout,
        }
    }
}

impl RenderOnce for ImageThumbnail {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let (width, height) = self.layout.size();
        let radius = self.layout.corner_radius();
        let icon_size = self.layout.placeholder_icon_size();
        let path = self.source.path.clone();
        let is_svg = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
        let svg_path = (is_svg && self.source.byte_len > 0)
            .then(|| path.to_str())
            .flatten()
            .map(str::to_owned);
        let show_placeholder = is_svg && svg_path.is_none();
        let preview = if let Some(external_path) = svg_path {
            color_svg()
                .external_path(external_path)
                .current_color(rgba(0xd8d8decc))
                .object_fit(ObjectFit::Contain)
                .absolute()
                .inset_0()
                .size_full()
                .rounded(radius)
                .into_any_element()
        } else if is_svg {
            // `color_svg` accepts UTF-8 paths and GPUI retries failed SVG
            // assets. Keep the placeholder for empty files and the uncommon
            // case of a Linux filename containing invalid UTF-8 so malformed
            // entries cannot flood the log on every repaint.
            div().into_any_element()
        } else {
            let source = self.source;
            img(ImageSource::from(
                move |window: &mut Window, cx: &mut App| {
                    window.use_asset::<ThumbnailAsset>(&source, cx)
                },
            ))
            .absolute()
            .inset_0()
            .size_full()
            .rounded(radius)
            .object_fit(ObjectFit::Cover)
            .into_any_element()
        };

        div()
            .relative()
            .w(width)
            .h(height)
            .flex_none()
            .overflow_hidden()
            .rounded(radius)
            .border_1()
            .border_color(rgba(0xffffff1f))
            .bg(rgba(0x11131bc4))
            .shadow(vec![
                gpui::BoxShadow::new(px(0.), px(3.), rgba(0x00000038).into())
                    .blur_radius(px(7.))
                    .spread_radius(px(-2.)),
            ])
            .when(show_placeholder, |thumbnail| {
                thumbnail.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path(LucideIcons::Image)
                                .size(icon_size)
                                .text_color(rgba(0x9da5b878)),
                        ),
                )
            })
            .child(preview)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ThumbnailSource {
    path: PathBuf,
    modified: Option<SystemTime>,
    byte_len: u64,
}

enum ThumbnailAsset {}

impl Asset for ThumbnailAsset {
    type Source = ThumbnailSource;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    // An `async fn` would retain GPUI's non-Send App reference in its future.
    #[allow(clippy::manual_async_fn)]
    fn load(
        source: Self::Source,
        _cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        async move {
            let decoded = ImageReader::open(&source.path)?
                .with_guessed_format()?
                .decode()?;
            let mut pixels = decoded
                .thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE)
                .into_rgba8();

            // GPUI's renderer consumes BGRA pixels.
            for pixel in pixels.pixels_mut() {
                pixel.0.swap(0, 2);
            }

            Ok(Arc::new(RenderImage::new(vec![Frame::new(pixels)])))
        }
    }
}
