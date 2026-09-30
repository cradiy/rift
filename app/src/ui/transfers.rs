use gpui::{
    AnyElement, App, Entity, FontWeight, IntoElement, MouseButton, Styled, Window, div, prelude::*,
    px, relative, rgba, svg,
};
use gpui_effects::FrostedGlass;
use rift_core::ports::{TransferKind, TransferPhase};
use uic::assets::LucideIcons;

use super::theme;
use crate::presentation::{TransferTask, TransferTasks, format_size};

pub(crate) fn layer(window: &Window, cx: &mut App) -> AnyElement {
    let manager = TransferTasks::entity(cx);
    let state = manager.read(cx);
    if state.tasks.is_empty() {
        return div().into_any_element();
    }
    let active = state
        .tasks
        .iter()
        .filter(|task| task.report.is_none())
        .count();
    let attention = state
        .tasks
        .iter()
        .filter(|task| {
            task.report
                .as_ref()
                .is_some_and(|report| !report.failures.is_empty())
        })
        .count();
    let expanded = state.expanded;
    let title = if active > 0 {
        format!("{} transfer{}", active, if active == 1 { "" } else { "s" })
    } else if attention > 0 {
        "Transfers need attention".into()
    } else {
        "Transfers".into()
    };
    let manager_for_toggle = manager.clone();
    let manager_for_clear = manager.clone();
    let rows = state
        .tasks
        .iter()
        .rev()
        .map(|task| render_task(&manager, task))
        .collect::<Vec<_>>();
    let width = (f32::from(window.viewport_size().width) - 32.).clamp(180., 392.);
    let max_height = (f32::from(window.viewport_size().height) - 230.).clamp(100., 460.);
    div()
        .absolute()
        .right(px(16.))
        .bottom(px(48.))
        .w(px(width))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            FrostedGlass::with_appearance(theme::transfer_glass())
                .w_full()
                .flex()
                .flex_col()
                .rounded(px(16.))
                .overflow_hidden()
                .shadow(vec![
                    gpui::BoxShadow::new(px(0.), px(12.), rgba(0x00000060).into())
                        .blur_radius(px(32.))
                        .spread_radius(px(-8.)),
                ])
                .child(
                    div()
                        .id("transfers-header")
                        .debug_selector(|| "transfers-header".into())
                        .h(px(48.))
                        .px(px(16.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            manager_for_toggle.update(cx, |manager, cx| manager.toggle(cx));
                        })
                        .child(
                            svg()
                                .path(LucideIcons::Copy)
                                .size(px(16.))
                                .text_color(rgba(0x62c8ffff)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgba(0xf4f3f8f2))
                                .child(title),
                        )
                        .when(!expanded, |header| {
                            header.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgba(0xaeb8c9d0))
                                    .child(state.tasks.len().to_string()),
                            )
                        })
                        .child(
                            svg()
                                .path(if expanded {
                                    LucideIcons::ChevronDown
                                } else {
                                    LucideIcons::ChevronUp
                                })
                                .size(px(15.))
                                .text_color(rgba(0xb3c0d4ce)),
                        ),
                )
                .when(expanded, |panel| {
                    panel
                        .child(
                            div()
                                .id("transfer-task-list")
                                .max_h(px(max_height))
                                .overflow_y_scroll()
                                .px(px(10.))
                                .pb(px(10.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .children(rows),
                        )
                        .when(
                            state.tasks.iter().any(|task| task.report.is_some()),
                            |panel| {
                                panel.child(
                                    div()
                                        .px(px(16.))
                                        .py(px(9.))
                                        .border_t_1()
                                        .border_color(rgba(0xffffff12))
                                        .flex()
                                        .justify_end()
                                        .items_center()
                                        .child(
                                            div()
                                                .id("clear-finished-transfers")
                                                .cursor_pointer()
                                                .text_size(px(11.))
                                                .text_color(rgba(0xbfe8ffe6))
                                                .on_click(move |_, _, cx| {
                                                    manager_for_clear.update(cx, |manager, cx| {
                                                        manager.clear_finished(cx)
                                                    })
                                                })
                                                .child("Clear finished"),
                                        ),
                                )
                            },
                        )
                }),
        )
        .into_any_element()
}

fn render_task(manager: &Entity<TransferTasks>, task: &TransferTask) -> AnyElement {
    let id = task.id;
    let manager_for_cancel = manager.clone();
    let manager_for_dismiss = manager.clone();
    let is_copy = task.request.kind == TransferKind::Copy;
    let report = task.report.as_ref();
    let running = report.is_none();
    let has_failures = report.is_some_and(|report| !report.failures.is_empty());
    let cancelled = report.is_some_and(|report| report.cancelled);
    let accent = if has_failures {
        rgba(0xffc477ff)
    } else if cancelled {
        rgba(0xa9b4c8ff)
    } else if running {
        rgba(0x62c8ffff)
    } else {
        rgba(0x69dab1ff)
    };
    let name = if task.request.sources.len() == 1 {
        task.request.sources[0]
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else {
        format!("{} items", task.request.sources.len())
    };
    let title = format!("{} {}", if is_copy { "Copy" } else { "Move" }, name);
    let progress = &task.progress;
    let status = if let Some(report) = report {
        if report.cancelled {
            format!(
                "Cancelled · {} done · {} remaining",
                report.completed.len(),
                report.remaining.len()
            )
        } else if !report.failures.is_empty() {
            format!(
                "{} done · {} failed",
                report.completed.len(),
                report.failures.len()
            )
        } else {
            format!(
                "Completed · {} item{}",
                report.completed.len(),
                if report.completed.len() == 1 { "" } else { "s" }
            )
        }
    } else if task.cancel_requested {
        "Cancelling…".into()
    } else if progress.phase == TransferPhase::Preparing {
        "Preparing…".into()
    } else if progress.phase == TransferPhase::Finishing {
        "Finishing current item…".into()
    } else {
        match progress.total_bytes {
            Some(total) if total > 0 => format!(
                "{} / {} · {} / {} items",
                format_size(progress.transferred_bytes.min(total)),
                format_size(total),
                progress.completed_items,
                progress.total_items
            ),
            _ => format!(
                "{} / {} items",
                progress.completed_items, progress.total_items
            ),
        }
    };
    let fraction = if let Some(total) = progress.total_bytes.filter(|total| *total > 0) {
        progress.transferred_bytes as f32 / total as f32
    } else if progress.total_items > 0 {
        progress.completed_items as f32 / progress.total_items as f32
    } else {
        0.
    };
    let fraction = if !running && !has_failures && !cancelled {
        1.
    } else {
        fraction.clamp(0., 1.)
    };
    let retry =
        report.is_some_and(|report| !report.retry_sources().is_empty()) && !task.retry_started;
    div()
        .id(("transfer-task", id))
        .flex_none()
        .p(px(12.))
        .rounded(px(11.))
        .border_1()
        .border_color(rgba(0xffffff12))
        .bg(rgba(0x0b132536))
        .flex()
        .flex_col()
        .gap(px(7.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgba(0xf1f3f7ed))
                        .child(title),
                )
                .when(!running, |row| {
                    row.child(
                        div()
                            .id(("dismiss-transfer", id))
                            .size(px(20.))
                            .rounded(px(5.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|button| button.bg(rgba(0xffffff12)))
                            .on_click(move |_, _, cx| {
                                manager_for_dismiss
                                    .update(cx, |manager, cx| manager.dismiss(id, cx))
                            })
                            .child(
                                svg()
                                    .path(LucideIcons::X)
                                    .size(px(12.))
                                    .text_color(rgba(0xbfcbdccc)),
                            ),
                    )
                }),
        )
        .child(
            div()
                .truncate()
                .text_size(px(10.5))
                .text_color(rgba(0xa9b5c9b0))
                .child(format!("To {}", task.request.directory.display())),
        )
        .when(running, |card| {
            card.child(
                div()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(rgba(0xd3dce9df))
                    .child(
                        progress
                            .current_path
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "Reading selected items…".into()),
                    ),
            )
        })
        .child(
            div()
                .h(px(3.))
                .w_full()
                .rounded_full()
                .overflow_hidden()
                .bg(rgba(0xffffff12))
                .child(
                    div()
                        .h_full()
                        .w(relative(fraction))
                        .rounded_full()
                        .bg(accent),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(10.5))
                        .text_color(accent)
                        .child(status),
                )
                .when(running && !task.cancel_requested, |row| {
                    row.child(text_button(("cancel-transfer", id), "Cancel").on_click(
                        move |_, _, cx| {
                            manager_for_cancel.update(cx, |manager, cx| manager.cancel(id, cx))
                        },
                    ))
                })
                .when(retry, |row| {
                    row.child(
                        text_button(("retry-transfer", id), "Retry remaining")
                            .on_click(move |_, _, cx| TransferTasks::retry(id, cx)),
                    )
                }),
        )
        .when_some(
            report.filter(|report| !report.failures.is_empty()),
            |card, report| {
                card.child(
                    div()
                        .id(("transfer-failures", id))
                        .max_h(px(145.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .children(report.failures.iter().map(|failure| {
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .pt(px(5.))
                                .border_t_1()
                                .border_color(rgba(0xffffff0e))
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .text_color(rgba(0xffd5a8ed))
                                        .child(failure.source.display().to_string()),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .text_color(rgba(0xc3cdddce))
                                        .child(failure.error.message.clone())
                                        .when(failure.error.path != failure.source, |message| {
                                            message.child(format!(
                                                " ({})",
                                                failure.error.path.display()
                                            ))
                                        }),
                                )
                        })),
                )
            },
        )
        .when(cancelled && !has_failures, |card| {
            card.child(
                div()
                    .text_size(px(10.))
                    .text_color(rgba(0xa9b5c9b0))
                    .child("Incomplete copies were removed. Completed items stay."),
            )
        })
        .into_any_element()
}

fn text_button(id: (&'static str, u64), label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px(px(7.))
        .py(px(3.))
        .rounded(px(5.))
        .cursor_pointer()
        .text_size(px(10.5))
        .text_color(rgba(0xc4eaffed))
        .hover(|button| button.bg(rgba(0x62c8ff18)))
        .child(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext, Focusable, Render, TestAppContext, VisualTestContext, size};
    use rift_core::ports::TransferRequest;
    use rift_fs::LocalFileSystem;
    use std::{fs, sync::Arc};
    use uic::components::input::{Input, TextInput};

    struct Fixture {
        input: Entity<TextInput>,
    }

    impl Render for Fixture {
        fn render(
            &mut self,
            window: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            div()
                .size_full()
                .relative()
                .child(Input::new(&self.input))
                .child(layer(window, cx))
        }
    }

    #[gpui::test]
    fn showing_and_updating_the_panel_keeps_existing_input_focus(cx: &mut TestAppContext) {
        cx.update(uic::init);
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("file.txt");
        let target = root.path().join("target");
        fs::write(&source, "data").unwrap();
        fs::create_dir(&target).unwrap();
        let input = cx.new(|cx| TextInput::new(cx).initial_value("unsubmitted name"));
        let input_for_view = input.clone();
        let handle = cx.open_window(size(px(520.), px(500.)), |window, cx| {
            window.focus(&input_for_view.read(cx).focus_handle(cx), cx);
            let manager = TransferTasks::entity(cx);
            cx.observe(&manager, |_, _, cx| cx.notify()).detach();
            Fixture {
                input: input_for_view,
            }
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, cx| window.draw(cx).clear());
        visual.cx.update(|cx| {
            TransferTasks::start(
                TransferRequest {
                    kind: TransferKind::Copy,
                    sources: vec![source],
                    directory: target,
                },
                Arc::new(LocalFileSystem),
                Vec::new(),
                cx,
            )
        });
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.draw(cx).clear();
            assert!(input.read(cx).focus_handle(cx).is_focused(window));
            assert_eq!(input.read(cx).value().as_ref(), "unsubmitted name");
        });
        assert!(visual.debug_bounds("transfers-header").is_some());
        let center = visual.debug_bounds("transfers-header").unwrap().center();
        visual.simulate_click(center, Default::default());
        visual
            .cx
            .update(|cx| assert!(!TransferTasks::entity(cx).read(cx).expanded));
    }
}
