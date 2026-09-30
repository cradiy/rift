use gpui::{
    AnyElement, App, Entity, FontWeight, IntoElement, MouseButton, Styled, Window, canvas, div,
    point, prelude::*, px, relative, rgba, svg,
};
use gpui_effects::FrostedGlass;
use rift_core::ports::{ConflictChoice, ConflictEntry, TransferKind, TransferPhase};
use uic::assets::LucideIcons;

use super::theme;
use crate::presentation::{TransferTask, TransferTasks, format_size};

pub(crate) fn indicator(cx: &mut App) -> Option<AnyElement> {
    let manager = TransferTasks::entity(cx);
    let state = manager.read(cx);
    let tasks = state
        .tasks
        .iter()
        .filter(|task| task.visible)
        .collect::<Vec<_>>();
    if tasks.is_empty() {
        return None;
    }
    let attention = tasks.iter().any(|task| {
        task.conflict.is_some()
            || task
                .report
                .as_ref()
                .is_some_and(|report| !report.failures.is_empty())
    });
    let active = tasks
        .iter()
        .copied()
        .filter(|task| task.report.is_none())
        .collect::<Vec<_>>();
    let fraction = if active.is_empty() {
        Some(1.)
    } else if active
        .iter()
        .any(|task| task.progress.phase == TransferPhase::Preparing)
    {
        None
    } else {
        let bytes = active.iter().try_fold((0u64, 0u64), |(done, total), task| {
            task.progress.total_bytes.map(|bytes| {
                (
                    done.saturating_add(task.progress.transferred_bytes.min(bytes)),
                    total.saturating_add(bytes),
                )
            })
        });
        match bytes.filter(|(_, total)| *total > 0) {
            Some((done, total)) => Some(done as f32 / total as f32),
            None => {
                let total = active
                    .iter()
                    .map(|task| task.progress.total_items)
                    .sum::<usize>();
                let done = active
                    .iter()
                    .map(|task| task.progress.completed_items + task.progress.skipped_items)
                    .sum::<usize>();
                (total > 0).then_some(done as f32 / total.max(1) as f32)
            }
        }
    };
    let color = if attention {
        rgba(0xffc477ff)
    } else if active.is_empty() {
        rgba(0x69dab1ff)
    } else {
        rgba(0x62c8ffff)
    };
    let label = if attention {
        "!".to_owned()
    } else {
        fraction.map_or_else(
            || "…".to_owned(),
            |fraction| {
                let percent = (fraction.clamp(0., 1.) * 100.).round() as u32;
                format!(
                    "{}%",
                    if active.is_empty() {
                        percent
                    } else {
                        percent.min(99)
                    }
                )
            },
        )
    };
    let manager = manager.clone();
    Some(
        div()
            .id("transfers-indicator")
            .debug_selector(|| "transfers-indicator".into())
            .flex_none()
            .h(px(25.))
            .px(px(7.))
            .rounded(px(7.))
            .flex()
            .items_center()
            .gap(px(7.))
            .cursor_pointer()
            .hover(|style| style.bg(rgba(0xffffff0c)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, _, cx| manager.update(cx, |manager, cx| manager.toggle(cx)))
            .child(
                div()
                    .size(px(18.))
                    .flex_none()
                    .rounded_full()
                    .bg(rgba(0xffffff12))
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| {
                                let center = bounds.center();
                                let radius = f32::from(bounds.size.width) * 0.5;
                                let fraction = fraction.unwrap_or(0.18).clamp(0., 1.);
                                if fraction == 0. {
                                    return;
                                }
                                let mut path = gpui::PathBuilder::fill();
                                path.move_to(center);
                                for step in 0..=48 {
                                    let angle = -std::f32::consts::FRAC_PI_2
                                        + std::f32::consts::TAU * fraction * step as f32 / 48.;
                                    path.line_to(point(
                                        center.x + px(angle.cos() * radius),
                                        center.y + px(angle.sin() * radius),
                                    ));
                                }
                                path.close();
                                if let Ok(path) = path.build() {
                                    window.paint_path(path, color);
                                }
                            },
                        )
                        .size(px(18.))
                        .flex_none(),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color)
                    .child(label),
            )
            .into_any_element(),
    )
}

pub(crate) fn layer(window: &Window, available_width: f32, cx: &mut App) -> AnyElement {
    let manager = TransferTasks::entity(cx);
    let state = manager.read(cx);
    let tasks = state
        .tasks
        .iter()
        .filter(|task| task.visible)
        .collect::<Vec<_>>();
    if tasks.is_empty() || !state.expanded {
        return div().into_any_element();
    }
    let active = tasks.iter().filter(|task| task.report.is_none()).count();
    let attention = tasks
        .iter()
        .filter(|task| {
            task.conflict.is_some()
                || task
                    .report
                    .as_ref()
                    .is_some_and(|report| !report.failures.is_empty())
        })
        .count();
    let expanded = state.expanded;
    let title = if tasks.iter().any(|task| task.conflict.is_some()) {
        "Name conflict".into()
    } else if active > 0 {
        format!("{} transfer{}", active, if active == 1 { "" } else { "s" })
    } else if attention > 0 {
        "Transfers need attention".into()
    } else {
        "Transfers".into()
    };
    let manager_for_toggle = manager.clone();
    let manager_for_clear = manager.clone();
    let rows = tasks
        .iter()
        .rev()
        .map(|task| render_task(&manager, task))
        .collect::<Vec<_>>();
    let width = (available_width - 24.).clamp(180., 392.);
    let max_height = (f32::from(window.viewport_size().height) - 150.).clamp(140., 460.);
    div()
        .absolute()
        .right(px(12.))
        .bottom(px(44.))
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
                                    .child(tasks.len().to_string()),
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
                        .when(tasks.iter().any(|task| task.report.is_some()), |panel| {
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
                        })
                }),
        )
        .into_any_element()
}

fn render_task(manager: &Entity<TransferTasks>, task: &TransferTask) -> AnyElement {
    if let Some(conflict) = task.conflict.as_ref() {
        return render_conflict(manager, task, conflict);
    }
    let id = task.id;
    let manager_for_cancel = manager.clone();
    let manager_for_dismiss = manager.clone();
    let is_copy = task.request.kind == TransferKind::Copy;
    let report = task.report.as_ref();
    let running = report.is_none();
    let has_failures = report.is_some_and(|report| !report.failures.is_empty());
    let cancelled = report.is_some_and(|report| report.cancelled);
    let waiting = task.conflict.is_some();
    let accent = if has_failures || waiting {
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
    let title = if let Some(conflict) = &task.conflict {
        format!(
            "\"{}\" already exists",
            conflict
                .destination
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        )
    } else {
        format!("{} {}", if is_copy { "Copy" } else { "Move" }, name)
    };
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
                "{} done · {} skipped · {} failed",
                report.completed.len(),
                report.skipped.len(),
                report.failures.len()
            )
        } else if !report.skipped.is_empty() {
            format!(
                "{} done · {} skipped",
                report.completed.len(),
                report.skipped.len()
            )
        } else {
            format!(
                "Completed · {} item{}",
                report.completed.len(),
                if report.completed.len() == 1 { "" } else { "s" }
            )
        }
    } else if waiting {
        "Waiting for your choice".into()
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
        .p(px(if waiting { 16. } else { 12. }))
        .rounded(px(11.))
        .border_1()
        .border_color(if waiting {
            rgba(0xffc47766)
        } else {
            rgba(0xffffff12)
        })
        .bg(rgba(0x0b132536))
        .flex()
        .flex_col()
        .gap(px(if waiting { 14. } else { 7. }))
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
                        .text_size(px(if waiting { 13. } else { 12. }))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgba(0xf1f3f7ed))
                        .child(title),
                )
                .when(waiting, |row| {
                    let manager = manager.clone();
                    row.child(text_button(("cancel-transfer", id), "Cancel").on_click(
                        move |_, _, cx| manager.update(cx, |manager, cx| manager.cancel(id, cx)),
                    ))
                })
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
        .when(!waiting, |card| {
            card.child(
                div()
                    .truncate()
                    .text_size(px(10.5))
                    .text_color(rgba(0xa9b5c9b0))
                    .child(format!("To {}", task.request.directory.display())),
            )
        })
        .when(running && !waiting, |card| {
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
        .when(!waiting, |card| {
            card.child(
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
        })
        .when(!waiting, |card| {
            card.child(
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
        })
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

fn render_conflict(
    manager: &Entity<TransferTasks>,
    task: &TransferTask,
    conflict: &rift_core::ports::TransferConflict,
) -> AnyElement {
    let id = task.id;
    let cancel_manager = manager.clone();
    let apply_manager = manager.clone();
    let name = conflict
        .destination
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let folder = conflict.incoming.is_directory || conflict.existing.is_directory;
    div()
        .id(("transfer-task", id))
        .flex_none()
        .p(px(12.))
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(42.))
                        .flex_none()
                        .rounded(px(11.))
                        .bg(rgba(0x62c8ff18))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path(if conflict.incoming.is_directory {
                                    LucideIcons::Folder
                                } else {
                                    LucideIcons::File
                                })
                                .size(px(24.))
                                .text_color(rgba(0x82d4ffff)),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgba(0xf5f5faff))
                                .line_clamp(2)
                                .child(name),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(rgba(0xb4bdcdbb))
                                .child("Already in this destination"),
                        ),
                )
                .child(
                    div()
                        .id(("cancel-transfer", id))
                        .size(px(24.))
                        .flex_none()
                        .rounded(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgba(0xffffff14)))
                        .on_click(move |_, _, cx| {
                            cancel_manager.update(cx, |manager, cx| manager.cancel(id, cx))
                        })
                        .child(
                            svg()
                                .path(LucideIcons::X)
                                .size(px(14.))
                                .text_color(rgba(0xb8c1d1cc)),
                        ),
                ),
        )
        .child(
            div()
                .rounded(px(10.))
                .bg(rgba(0xffffff05))
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(11.))
                .child(conflict_entry("From", &conflict.source, &conflict.incoming))
                .child(div().h(px(1.)).bg(rgba(0xffffff0b)))
                .child(conflict_entry(
                    "To",
                    &conflict.destination,
                    &conflict.existing,
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(7.))
                .child(
                    svg()
                        .path(if conflict.replace_allowed {
                            LucideIcons::Trash2
                        } else {
                            LucideIcons::TriangleAlert
                        })
                        .size(px(12.))
                        .flex_none()
                        .text_color(rgba(0xdbb6a8aa)),
                )
                .child(div().text_size(px(10.)).text_color(rgba(0xc7bdc9b8)).child(
                    if !conflict.replace_allowed {
                        "Locations overlap · cannot replace"
                    } else if folder {
                        "Whole folder replaced · old item → Trash"
                    } else {
                        "Replaced item goes to Trash"
                    },
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .id(("conflict-apply-all", id))
                        .debug_selector(|| "conflict-apply-all".into())
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            apply_manager
                                .update(cx, |manager, cx| manager.toggle_apply_to_all(id, cx))
                        })
                        .child(
                            svg()
                                .path(if task.apply_to_all {
                                    LucideIcons::SquareCheck
                                } else {
                                    LucideIcons::Square
                                })
                                .size(px(14.))
                                .text_color(if task.apply_to_all {
                                    rgba(0x82d4ffff)
                                } else {
                                    rgba(0x8995a9bb)
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgba(0xb6c1d2dd))
                                .child("Apply to all conflicts"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(conflict_button(
                            manager,
                            id,
                            ConflictChoice::KeepBoth,
                            "Keep both",
                            true,
                        ))
                        .child(conflict_button(
                            manager,
                            id,
                            ConflictChoice::Skip,
                            "Skip",
                            true,
                        ))
                        .child(conflict_button(
                            manager,
                            id,
                            ConflictChoice::Replace,
                            "Replace",
                            conflict.replace_allowed,
                        )),
                ),
        )
        .into_any_element()
}

fn conflict_entry(
    label: &'static str,
    path: &std::path::Path,
    entry: &ConflictEntry,
) -> impl IntoElement {
    let kind = if entry.is_symlink {
        "Link".to_owned()
    } else if entry.is_directory {
        "Folder".to_owned()
    } else {
        format_size(entry.byte_len)
    };
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgba(0xa9b5c9b0))
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgba(0xa9b5c9b0))
                        .child(kind),
                ),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgba(0xf1f3f7ed))
                .line_clamp(1)
                .child(display_location(path.parent().unwrap_or(path))),
        )
}

fn display_location(path: &std::path::Path) -> String {
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(relative) = path.strip_prefix(std::path::Path::new(&home))
    {
        return if relative.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", relative.display())
        };
    }
    path.display().to_string()
}

fn conflict_button(
    manager: &Entity<TransferTasks>,
    id: u64,
    choice: ConflictChoice,
    label: &'static str,
    enabled: bool,
) -> impl IntoElement {
    let manager = manager.clone();
    let danger = choice == ConflictChoice::Replace;
    div()
        .id((label, id))
        .debug_selector(move || {
            match choice {
                ConflictChoice::KeepBoth => "conflict-keep-both",
                ConflictChoice::Skip => "conflict-skip",
                ConflictChoice::Replace => "conflict-replace",
            }
            .into()
        })
        .flex_1()
        .min_w(px(82.))
        .h(px(36.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .border_1()
        .border_color(if danger {
            rgba(0xe4797955)
        } else if choice == ConflictChoice::Skip {
            rgba(0xffffff12)
        } else {
            rgba(0x62c8ff55)
        })
        .bg(if danger {
            rgba(0xcc555518)
        } else if choice == ConflictChoice::Skip {
            rgba(0xffffff04)
        } else {
            rgba(0x62c8ff26)
        })
        .text_color(if danger {
            rgba(0xffb4aeff)
        } else {
            rgba(0xc4eaffed)
        })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| {
                    style.bg(if danger {
                        rgba(0xcc555536)
                    } else {
                        rgba(0x62c8ff30)
                    })
                })
                .on_click(move |_, _, cx| {
                    manager.update(cx, |manager, cx| manager.resolve(id, choice, cx))
                })
        })
        .when(!enabled, |button| button.opacity(0.35))
        .child(label)
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
                .child(
                    div()
                        .absolute()
                        .right(px(12.))
                        .bottom(px(4.))
                        .children(indicator(cx)),
                )
                .child(layer(window, f32::from(window.viewport_size().width), cx))
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
                    sources: vec![source.clone()],
                    directory: target.clone(),
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
        assert!(visual.debug_bounds("transfers-header").is_none());
        assert!(visual.debug_bounds("transfers-indicator").is_none());
        visual.cx.update(|cx| {
            TransferTasks::start(
                TransferRequest {
                    kind: TransferKind::Copy,
                    sources: vec![source],
                    directory: target.clone(),
                },
                Arc::new(LocalFileSystem),
                Vec::new(),
                cx,
            );
        });
        visual.run_until_parked();
        let id = visual.cx.update(|cx| {
            let manager = TransferTasks::entity(cx);
            let task = manager.read(cx).tasks.last().unwrap();
            assert!(task.conflict.is_some());
            task.id
        });
        visual.update(|window, cx| window.draw(cx).clear());
        // An attention prompt opens automatically. Explicitly opening it
        // from the compact entry opts into keeping its completed result.
        let entry = visual.debug_bounds("transfers-indicator").unwrap().center();
        visual.simulate_click(entry, Default::default());
        visual.update(|window, cx| window.draw(cx).clear());
        assert!(visual.debug_bounds("conflict-keep-both").is_none());
        visual.simulate_click(entry, Default::default());
        visual.update(|window, cx| window.draw(cx).clear());
        let apply = visual.debug_bounds("conflict-apply-all").unwrap().center();
        visual.simulate_click(apply, Default::default());
        visual.cx.update(|cx| {
            assert!(
                TransferTasks::entity(cx)
                    .read(cx)
                    .tasks
                    .iter()
                    .find(|task| task.id == id)
                    .unwrap()
                    .apply_to_all
            )
        });
        visual.update(|window, cx| window.draw(cx).clear());
        let keep = visual.debug_bounds("conflict-keep-both").unwrap().center();
        visual.simulate_click(keep, Default::default());
        visual.run_until_parked();
        visual.update(|window, cx| {
            assert!(input.read(cx).focus_handle(cx).is_focused(window));
            assert_eq!(input.read(cx).value().as_ref(), "unsubmitted name");
            assert!(
                TransferTasks::entity(cx)
                    .read(cx)
                    .tasks
                    .last()
                    .unwrap()
                    .report
                    .is_some()
            );
        });
        assert!(target.join("file copy.txt").exists());
        let center = visual.debug_bounds("transfers-header").unwrap().center();
        visual.simulate_click(center, Default::default());
        visual
            .cx
            .update(|cx| assert!(!TransferTasks::entity(cx).read(cx).expanded));
    }
}
