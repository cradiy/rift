use std::{path::Path, time::Duration};

use gpui::{
    Animation, AnimationExt as _, AnyElement, ClipboardItem, FontWeight, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Styled, Task, Window, deferred, div,
    ease_out_quint, px, rgba,
};
use gpui_effects::FrostedGlass;
use uic::components::toast;

use crate::ui::theme;

use super::{
    FileBrowser,
    actions::{
        CancelWhichKey, CopyFileNameText, CopyFilePathText, CopyParentDirectoryText,
        StartCopyPrefix, StartGoPrefix,
    },
};

const WHICH_KEY_DELAY: Duration = Duration::from_millis(280);

#[derive(Default)]
pub(super) struct WhichKeyState {
    prefix: Option<WhichKeyPrefix>,
    visible: bool,
    generation: u64,
    task: Option<Task<()>>,
}

impl WhichKeyState {
    pub(super) fn context_token(&self) -> Option<&'static str> {
        self.prefix.map(WhichKeyPrefix::context_token)
    }

    pub(super) fn reset(&mut self) {
        self.prefix = None;
        self.visible = false;
        self.generation = self.generation.wrapping_add(1);
        self.task = None;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WhichKeyPrefix {
    Copy,
    Go,
}

impl WhichKeyPrefix {
    fn context_token(self) -> &'static str {
        match self {
            Self::Copy => "copy_prefix",
            Self::Go => "go_prefix",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Copy => "c",
            Self::Go => "g",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::Go => "Go",
        }
    }

    fn items(self) -> &'static [WhichKeyItem] {
        match self {
            Self::Copy => &[
                WhichKeyItem {
                    key: "f",
                    label: "File Name",
                },
                WhichKeyItem {
                    key: "c",
                    label: "File Path",
                },
                WhichKeyItem {
                    key: "d",
                    label: "Parent Directory",
                },
            ],
            Self::Go => &[WhichKeyItem {
                key: "g",
                label: "First Item",
            }],
        }
    }

    fn panel_width(self) -> f32 {
        match self {
            Self::Copy => 520.0,
            Self::Go => 260.0,
        }
    }
}

struct WhichKeyItem {
    key: &'static str,
    label: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CopyTarget {
    FileName,
    FilePath,
    ParentDirectory,
}

impl CopyTarget {
    fn value(self, path: &Path, name: &str) -> Option<String> {
        match self {
            Self::FileName => Some(name.to_owned()),
            Self::FilePath => Some(path.display().to_string()),
            Self::ParentDirectory => path.parent().map(|parent| parent.display().to_string()),
        }
    }

    fn success_message(self) -> &'static str {
        match self {
            Self::FileName => "Copied file name",
            Self::FilePath => "Copied file path",
            Self::ParentDirectory => "Copied parent directory",
        }
    }
}

impl FileBrowser {
    pub(super) fn handle_which_key_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.which_key.prefix.is_some() && !event.is_held {
            self.cancel_which_key(cx);
            cx.stop_propagation();
        }
    }

    pub(super) fn start_copy_prefix(
        &mut self,
        _: &StartCopyPrefix,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.primary_item(cx).is_none() {
            return;
        }

        self.start_which_key(WhichKeyPrefix::Copy, cx);
    }

    pub(super) fn start_go_prefix(
        &mut self,
        _: &StartGoPrefix,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.navigable_items(cx).is_empty() {
            return;
        }

        self.start_which_key(WhichKeyPrefix::Go, cx);
    }

    fn start_which_key(&mut self, prefix: WhichKeyPrefix, cx: &mut gpui::Context<Self>) {
        self.which_key.prefix = Some(prefix);
        self.which_key.visible = false;
        self.which_key.generation = self.which_key.generation.wrapping_add(1);
        let generation = self.which_key.generation;
        self.which_key.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WHICH_KEY_DELAY).await;
            let Some(this) = this.upgrade() else {
                return;
            };
            this.update(cx, |browser, cx| {
                if browser.which_key.prefix == Some(prefix)
                    && browser.which_key.generation == generation
                {
                    browser.which_key.visible = true;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    pub(super) fn copy_file_name_text(
        &mut self,
        _: &CopyFileNameText,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.finish_copy_which_key(CopyTarget::FileName, cx);
    }

    pub(super) fn copy_file_path_text(
        &mut self,
        _: &CopyFilePathText,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.finish_copy_which_key(CopyTarget::FilePath, cx);
    }

    pub(super) fn copy_parent_directory_text(
        &mut self,
        _: &CopyParentDirectoryText,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.finish_copy_which_key(CopyTarget::ParentDirectory, cx);
    }

    pub(super) fn cancel_which_key_action(
        &mut self,
        _: &CancelWhichKey,
        _: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.cancel_which_key(cx);
    }

    pub(super) fn cancel_which_key(&mut self, cx: &mut gpui::Context<Self>) {
        let changed = self.which_key.prefix.is_some() || self.which_key.visible;
        self.which_key.reset();
        if changed {
            cx.notify();
        }
    }

    fn copy_selected_text(&self, target: CopyTarget, cx: &mut gpui::Context<Self>) {
        let Some(item) = self.primary_item(cx) else {
            return;
        };
        let Some(value) = target.value(&item.path, &item.name) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(value));
        toast::success(target.success_message(), cx);
    }

    fn finish_copy_which_key(&mut self, target: CopyTarget, cx: &mut gpui::Context<Self>) {
        self.cancel_which_key(cx);
        self.copy_selected_text(target, cx);
    }

    pub(super) fn render_which_key(&self) -> Option<AnyElement> {
        if !self.which_key.visible {
            return None;
        }
        let prefix = self.which_key.prefix?;

        let panel = FrostedGlass::with_appearance(theme::which_key_glass())
            .w(px(prefix.panel_width()))
            .max_w_full()
            .px(px(13.))
            .py(px(11.))
            .rounded(px(16.))
            .overflow_hidden()
            .shadow(vec![
                gpui::BoxShadow::new(px(0.), px(18.), rgba(0x0000008a).into())
                    .blur_radius(px(46.))
                    .spread_radius(px(-10.)),
                gpui::BoxShadow::new(px(0.), px(1.), rgba(0xffffff1b).into()).blur_radius(px(1.)),
            ])
            .flex()
            .items_center()
            .gap(px(12.))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pr(px(13.))
                    .border_r_1()
                    .border_color(rgba(0xffffff1b))
                    .child(key_badge(prefix.key(), true))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgba(0xf7f4f8ed))
                            .child(prefix.label()),
                    ),
            )
            .children(
                prefix
                    .items()
                    .iter()
                    .map(|item| which_key_item(item.key, item.label)),
            )
            .with_animation(
                "which-key-open",
                Animation::new(Duration::from_millis(120)).with_easing(ease_out_quint()),
                |panel, phase| panel.opacity(phase),
            );

        Some(
            deferred(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(42.))
                    .px(px(18.))
                    .flex()
                    .justify_center()
                    .child(panel),
            )
            .with_priority(500)
            .into_any_element(),
        )
    }
}

fn which_key_item(key: &'static str, label: &'static str) -> impl IntoElement {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(7.))
        .child(key_badge(key, false))
        .child(
            div()
                .truncate()
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgba(0xe7e4eacd))
                .child(label),
        )
}

fn key_badge(key: &'static str, prefix: bool) -> impl IntoElement {
    div()
        .min_w(px(25.))
        .h(px(24.))
        .px(px(7.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .border_1()
        .border_color(if prefix {
            rgba(0x62c8ff70)
        } else {
            rgba(0xffffff25)
        })
        .bg(if prefix {
            rgba(0x159fe334)
        } else {
            rgba(0xffffff0f)
        })
        .text_size(px(11.))
        .font_weight(FontWeight::BOLD)
        .text_color(if prefix {
            rgba(0x86d9ffff)
        } else {
            rgba(0xf2f0f4e8)
        })
        .child(key)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{CopyTarget, WhichKeyPrefix};

    #[test]
    fn copy_targets_resolve_the_expected_path_component() {
        let path = Path::new("/home/cradiy/Pictures/cover.jpg");
        assert_eq!(
            CopyTarget::FileName.value(path, "cover.jpg").as_deref(),
            Some("cover.jpg")
        );
        assert_eq!(
            CopyTarget::FilePath.value(path, "cover.jpg").as_deref(),
            Some("/home/cradiy/Pictures/cover.jpg")
        );
        assert_eq!(
            CopyTarget::ParentDirectory
                .value(path, "cover.jpg")
                .as_deref(),
            Some("/home/cradiy/Pictures")
        );
    }

    #[test]
    fn every_current_multi_key_prefix_exposes_its_continuations() {
        let copy = WhichKeyPrefix::Copy.items();
        assert_eq!(
            copy.iter()
                .map(|item| (item.key, item.label))
                .collect::<Vec<_>>(),
            vec![
                ("f", "File Name"),
                ("c", "File Path"),
                ("d", "Parent Directory"),
            ]
        );

        let go = WhichKeyPrefix::Go.items();
        assert_eq!(
            go.iter()
                .map(|item| (item.key, item.label))
                .collect::<Vec<_>>(),
            vec![("g", "First Item")]
        );
    }
}
