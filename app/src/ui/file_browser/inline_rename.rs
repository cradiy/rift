use std::path::PathBuf;

use gpui::{AppContext, Entity, Focusable, IntoElement, Subscription, div, prelude::*, px, rgba};
use rift_core::ports::FileOperation;
use uic::components::{
    input::{Input, InputEvent, TextInput},
    toast,
};

use crate::{presentation::BrowserController, ui::theme};

use super::{FileBrowser, file_actions::validate_file_name};

pub(super) struct InlineRenameState {
    path: PathBuf,
    input: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone)]
pub(super) struct InlineRenameView {
    path: PathBuf,
    input: Entity<TextInput>,
}

#[derive(Clone, Copy)]
pub(super) enum InlineRenameLayout {
    Grid,
    List,
}

impl FileBrowser {
    pub(super) fn begin_inline_rename(
        &mut self,
        path: PathBuf,
        name: String,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let input = cx.new(|cx| TextInput::new(cx).initial_value(name));
        let input_for_blur = input.clone();
        let submit_subscription = cx.subscribe_in(
            &input,
            window,
            |browser, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Submit(_)) {
                    browser.commit_inline_rename(input.clone(), window, cx);
                }
            },
        );
        let focus_handle = input.read(cx).focus_handle(cx);
        let blur_subscription = cx.on_blur(&focus_handle, window, move |browser, _, cx| {
            browser.cancel_inline_rename(input_for_blur.clone(), cx);
        });

        self.inline_rename = Some(InlineRenameState {
            path,
            input: input.clone(),
            _subscriptions: vec![submit_subscription, blur_subscription],
        });
        cx.notify();
        window.focus(&focus_handle, cx);
    }

    pub(super) fn inline_rename_view(&self) -> Option<InlineRenameView> {
        self.inline_rename.as_ref().map(|rename| InlineRenameView {
            path: rename.path.clone(),
            input: rename.input.clone(),
        })
    }

    pub(super) fn inline_rename_for(
        rename: Option<InlineRenameView>,
        path: &std::path::Path,
    ) -> Option<InlineRenameView> {
        rename.filter(|rename| rename.path == path)
    }

    pub(super) fn inline_rename_field(
        rename: InlineRenameView,
        layout: InlineRenameLayout,
        browser: Entity<Self>,
    ) -> gpui::AnyElement {
        let input_for_escape = rename.input.clone();
        let input = Input::new(&rename.input)
            .appearance(theme::input_appearance())
            .h(match layout {
                InlineRenameLayout::Grid => px(27.),
                InlineRenameLayout::List => px(30.),
            })
            .px(px(7.))
            .rounded(px(6.))
            .border_color(rgba(0x0a84ffff))
            .bg(rgba(0x131620f2))
            .text_size(px(13.))
            .text_color(rgba(0xf5f4f8f2));

        div()
            .id(("inline-rename", rename.input.entity_id()))
            .when(matches!(layout, InlineRenameLayout::Grid), |field| {
                field.w_full().px(px(2.))
            })
            .when(matches!(layout, InlineRenameLayout::List), |field| {
                field.w(px(300.)).max_w_full()
            })
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_click(|_, _, cx| cx.stop_propagation())
            .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
                browser.update(cx, |browser, cx| {
                    if event.keystroke.key == "escape" {
                        if browser.cancel_inline_rename(input_for_escape.clone(), cx) {
                            cx.focus_self(window);
                        }
                        cx.stop_propagation();
                    }
                });
            })
            .child(input)
            .into_any_element()
    }

    fn cancel_inline_rename(
        &mut self,
        expected_input: Entity<TextInput>,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if self
            .inline_rename
            .as_ref()
            .is_some_and(|rename| rename.input.entity_id() == expected_input.entity_id())
        {
            self.inline_rename = None;
            cx.notify();
            true
        } else {
            false
        }
    }

    pub(super) fn cancel_active_inline_rename(&mut self, cx: &mut gpui::Context<Self>) {
        if self.inline_rename.take().is_some() {
            cx.notify();
        }
    }

    fn commit_inline_rename(
        &mut self,
        expected_input: Entity<TextInput>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(rename) = self.inline_rename.as_ref() else {
            return;
        };
        if rename.input.entity_id() != expected_input.entity_id() {
            return;
        }

        let name = rename.input.read(cx).value().trim().to_owned();
        if let Err(message) = validate_file_name(&name) {
            toast::error(message, cx);
            return;
        }
        let source = rename.path.clone();
        let old_name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name == old_name {
            self.inline_rename = None;
            cx.notify();
            cx.focus_self(window);
            return;
        }
        let Some(parent) = source.parent() else {
            toast::error("This item cannot be renamed", cx);
            return;
        };
        let destination = parent.join(name);
        let controller: Entity<BrowserController> = self.controller.clone();
        self.inline_rename = None;
        cx.notify();
        cx.focus_self(window);
        Self::run_operation(
            controller,
            FileOperation::Rename {
                source,
                destination,
            },
            "Item renamed",
            cx,
        );
    }
}
