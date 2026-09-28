use gpui::{
    AnyElement, Bounds, Entity, FontWeight, IntoElement, ListState, MouseButton, Pixels, Point,
    SharedString, div, list, point, prelude::*, px, rgba, svg,
};
use rift_core::application::{BrowserMessage, SelectionMode, SortField, ViewMode};
use rift_core::domain::EntryCategory;
use uic::assets::LucideIcons;
use uic::components::{
    context_menu,
    scrollbar::{Scrollbar, ScrollbarAppearance, ScrollbarState},
};

use crate::{
    presentation::{BrowserController, BrowserItem, ItemIcon},
    ui::components::{FolderIcon, ImageThumbnail, ImageThumbnailLayout},
};

use super::{
    FileBrowser, FolderCountState,
    inline_rename::{InlineRenameLayout, InlineRenameView},
};

#[derive(Clone)]
enum GridRow {
    Header {
        category: EntryCategory,
        count: usize,
    },
    Items {
        entries: Vec<BrowserItem>,
        show_category: bool,
    },
}

#[derive(Clone)]
enum FileListRow {
    Entry {
        entry: BrowserItem,
        stripe_index: usize,
    },
    Empty {
        stripe_index: usize,
    },
}

#[derive(Clone)]
pub(super) struct FileItemContext {
    pub(super) controller: Entity<BrowserController>,
    pub(super) browser: Entity<FileBrowser>,
    pub(super) marquee_bounds: Option<Bounds<Pixels>>,
}

impl FileBrowser {
    const GRID_TILE_WIDTH: f32 = 122.;
    const GRID_COLUMN_GAP: f32 = 15.;
    const GRID_HORIZONTAL_PADDING: f32 = 44.;

    fn clipped_marquee_bounds(
        &self,
        current: Point<Pixels>,
        scroll_state: &ListState,
    ) -> Option<Bounds<Pixels>> {
        let marquee = self.marquee_bounds_to(current)?;
        let viewport = scroll_state.viewport_bounds();
        marquee
            .intersects(&viewport)
            .then(|| marquee.intersect(&viewport))
    }

    fn update_grid_marquee(
        &mut self,
        current: Point<Pixels>,
        rows: &[GridRow],
        scroll_state: &ListState,
        cx: &mut gpui::Context<Self>,
    ) {
        let hits = self
            .clipped_marquee_bounds(current, scroll_state)
            .map(|marquee| {
                rows.iter()
                    .enumerate()
                    .filter_map(|(row_index, row)| {
                        let GridRow::Items { entries, .. } = row else {
                            return None;
                        };
                        let row_bounds = scroll_state.bounds_for_item(row_index)?;
                        Some(
                            entries
                                .iter()
                                .enumerate()
                                .filter_map(move |(column, entry)| {
                                    let left = row_bounds.left()
                                        + px(22.)
                                        + px(column as f32
                                            * (Self::GRID_TILE_WIDTH + Self::GRID_COLUMN_GAP));
                                    let item_bounds = Bounds::from_corners(
                                        point(left, row_bounds.top()),
                                        point(
                                            left + px(Self::GRID_TILE_WIDTH),
                                            row_bounds.bottom(),
                                        ),
                                    );
                                    marquee.intersects(&item_bounds).then(|| entry.path.clone())
                                }),
                        )
                    })
                    .flatten()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.update_marquee_selection(current, hits, cx);
    }

    fn update_list_marquee(
        &mut self,
        current: Point<Pixels>,
        rows: &[FileListRow],
        scroll_state: &ListState,
        cx: &mut gpui::Context<Self>,
    ) {
        let hits = self
            .clipped_marquee_bounds(current, scroll_state)
            .map(|marquee| {
                rows.iter()
                    .enumerate()
                    .filter_map(|(row_index, row)| {
                        let FileListRow::Entry { entry, .. } = row else {
                            return None;
                        };
                        scroll_state
                            .bounds_for_item(row_index)
                            .filter(|bounds| marquee.intersects(bounds))
                            .map(|_| entry.path.clone())
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.update_marquee_selection(current, hits, cx);
    }

    fn marquee_overlay(
        marquee: Option<Bounds<Pixels>>,
        scroll_state: &ListState,
    ) -> Option<AnyElement> {
        let marquee = marquee?;
        let viewport = scroll_state.viewport_bounds();
        if !marquee.intersects(&viewport) {
            return None;
        }
        let marquee = marquee.intersect(&viewport);
        let origin = marquee.origin.relative_to(&viewport.origin);
        Some(
            div()
                .absolute()
                .left(origin.x)
                .top(origin.y)
                .w(marquee.size.width)
                .h(marquee.size.height)
                .rounded(px(3.))
                .border_1()
                .border_color(rgba(0x68b8f4dc))
                .bg(rgba(0x3f9fe52b))
                .into_any_element(),
        )
    }

    pub(super) fn grid_column_count(content_width: Pixels) -> usize {
        let available = (content_width - px(Self::GRID_HORIZONTAL_PADDING)).max(px(1.));
        (((available + px(Self::GRID_COLUMN_GAP))
            / px(Self::GRID_TILE_WIDTH + Self::GRID_COLUMN_GAP))
        .floor() as usize)
            .max(1)
    }

    fn group_by_category(items: Vec<BrowserItem>) -> Vec<(EntryCategory, Vec<BrowserItem>)> {
        let mut groups: Vec<(EntryCategory, Vec<BrowserItem>)> = Vec::new();
        for item in items {
            if let Some((_, entries)) = groups
                .last_mut()
                .filter(|(category, _)| *category == item.category)
            {
                entries.push(item);
            } else {
                groups.push((item.category, vec![item]));
            }
        }
        groups
    }

    fn push_grid_item_rows(
        rows: &mut Vec<GridRow>,
        entries: Vec<BrowserItem>,
        columns: usize,
        show_category: bool,
    ) {
        let mut entries = entries.into_iter();
        loop {
            let row = entries.by_ref().take(columns).collect::<Vec<_>>();
            if row.is_empty() {
                break;
            }
            rows.push(GridRow::Items {
                entries: row,
                show_category,
            });
        }
    }

    fn grid_rows(items: Vec<BrowserItem>, group_by_kind: bool, columns: usize) -> Vec<GridRow> {
        let mut rows = Vec::new();
        if group_by_kind {
            for (category, entries) in Self::group_by_category(items) {
                let count = entries.len();
                rows.push(GridRow::Header { category, count });
                Self::push_grid_item_rows(&mut rows, entries, columns, true);
            }
        } else {
            Self::push_grid_item_rows(&mut rows, items, columns, false);
        }
        rows
    }

    pub(super) fn reveal_path(&mut self, path: &std::path::Path, cx: &gpui::App) {
        let items = self.navigable_items(cx);
        let (view_mode, group_by_kind) = {
            let controller = self.controller.read(cx);
            let state = controller.state();
            (state.view_mode(), state.sort().field == SortField::Kind)
        };

        match view_mode {
            ViewMode::List => {
                if let Some(index) = items.iter().position(|item| item.path == path) {
                    self.list_scroll.scroll_to_reveal_item(index);
                }
            }
            ViewMode::Grid => {
                let rows = Self::grid_rows(items, group_by_kind, self.grid_columns);
                if let Some(index) = rows.iter().position(|row| {
                    matches!(row, GridRow::Items { entries, .. } if entries.iter().any(|item| item.path == path))
                }) {
                    self.grid_scroll.scroll_to_reveal_item(index);
                }
            }
        }
    }

    fn file_list_rows(items: Vec<BrowserItem>) -> Vec<FileListRow> {
        let row_count = items.len();
        let mut rows = items
            .into_iter()
            .enumerate()
            .map(|(stripe_index, entry)| FileListRow::Entry {
                entry,
                stripe_index,
            })
            .collect::<Vec<_>>();
        rows.extend((0..18).map(|index| FileListRow::Empty {
            stripe_index: row_count + index,
        }));
        rows
    }

    fn category_label(category: EntryCategory) -> &'static str {
        match category {
            EntryCategory::Folder => "Folders",
            EntryCategory::Application => "Applications",
            EntryCategory::Document => "Documents",
            EntryCategory::Image => "Images",
            EntryCategory::Audio => "Audio",
            EntryCategory::Video => "Videos",
            EntryCategory::Archive => "Archives",
            EntryCategory::Code => "Source Code",
            EntryCategory::Alias => "Symbolic Links",
            EntryCategory::Other => "Other",
        }
    }

    fn category_icon(category: EntryCategory) -> LucideIcons {
        match category {
            EntryCategory::Folder => LucideIcons::Folder,
            EntryCategory::Application => LucideIcons::AppWindow,
            EntryCategory::Document => LucideIcons::FileText,
            EntryCategory::Image => LucideIcons::Image,
            EntryCategory::Audio => LucideIcons::Music,
            EntryCategory::Video => LucideIcons::Film,
            EntryCategory::Archive => LucideIcons::Archive,
            EntryCategory::Code => LucideIcons::Code,
            EntryCategory::Alias => LucideIcons::Link2,
            EntryCategory::Other => LucideIcons::Files,
        }
    }

    fn category_accent(category: EntryCategory) -> gpui::Rgba {
        match category {
            EntryCategory::Folder => rgba(0x50c8eff0),
            EntryCategory::Application => rgba(0x8ca7fff0),
            EntryCategory::Document => rgba(0x8bcdf0e8),
            EntryCategory::Image => rgba(0x60d4c3ee),
            EntryCategory::Audio => rgba(0xc493f3ec),
            EntryCategory::Video => rgba(0x7faaf5ec),
            EntryCategory::Archive => rgba(0xe9b76cea),
            EntryCategory::Code => rgba(0x80c98eea),
            EntryCategory::Alias => rgba(0x99bce2e6),
            EntryCategory::Other => rgba(0xb8bbc5d6),
        }
    }

    fn category_header(category: EntryCategory, count: usize) -> impl IntoElement {
        div()
            .h(px(28.))
            .self_stretch()
            .px(px(4.))
            .flex()
            .items_center()
            .gap(px(7.))
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgba(0xe2e0e6d1))
            .child(
                div()
                    .size(px(18.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path(Self::category_icon(category))
                            .size(px(14.))
                            .text_color(Self::category_accent(category)),
                    ),
            )
            .child(Self::category_label(category))
            .child(
                div()
                    .text_size(px(10.5))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(rgba(0xc4c2ca8f))
                    .child(format!("{count}")),
            )
            .child(div().ml(px(5.)).flex_1().h(px(1.)).bg(rgba(0xffffff10)))
    }

    fn vertical_scrollbar(
        id: &'static str,
        scroll_state: &ListState,
        scrollbar_state: &ScrollbarState,
    ) -> AnyElement {
        div()
            .absolute()
            .top(px(4.))
            .right(px(2.))
            .bottom(px(4.))
            .w(px(10.))
            .child(
                Scrollbar::vertical(
                    SharedString::from(format!("{id}-scrollbar")),
                    scrollbar_state,
                    scroll_state,
                )
                .appearance(ScrollbarAppearance {
                    thumb: rgba(0xb9bec775).into(),
                    hover_thumb: rgba(0xd3d7de9e).into(),
                    dragging_thumb: rgba(0xe4e7ecbd).into(),
                    focus_ring: rgba(0x0a84ff80).into(),
                    thumb_radius: px(999.),
                    min_thumb_length: px(42.),
                })
                .auto_hide(false),
            )
            .into_any_element()
    }

    fn file_icon(category: Option<EntryCategory>) -> impl IntoElement {
        let accent = category.map(Self::category_accent);
        div()
            .relative()
            .w(px(76.))
            .h(px(60.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                svg()
                    .path(LucideIcons::File)
                    .size(px(48.))
                    .text_color(accent.unwrap_or_else(|| rgba(0xd8d8decc))),
            )
    }

    fn entry_icon(entry: &BrowserItem) -> Option<LucideIcons> {
        match entry.icon {
            ItemIcon::Folder | ItemIcon::File => None,
            ItemIcon::Application => Some(LucideIcons::AppWindow),
            ItemIcon::Desktop => Some(LucideIcons::Monitor),
            ItemIcon::Document => Some(LucideIcons::FileText),
            ItemIcon::Download => Some(LucideIcons::CircleArrowDown),
            ItemIcon::Videos => Some(LucideIcons::Film),
            ItemIcon::Music => Some(LucideIcons::Music),
            ItemIcon::Picture => Some(LucideIcons::Image),
        }
    }

    fn folder_tile(
        entry: BrowserItem,
        show_category: bool,
        controller: Entity<BrowserController>,
        browser: Entity<FileBrowser>,
        inline_rename: Option<InlineRenameView>,
        cx: &mut gpui::App,
    ) -> gpui::AnyElement {
        let icon = Self::entry_icon(&entry);
        let path = entry.path.clone();
        let is_directory = entry.is_directory;
        let selected = entry.selected;
        let context_path = path.clone();
        let context_entry = entry.clone();
        let context_controller = controller.clone();
        let context_browser = browser.clone();
        let click_browser = browser.clone();
        let entry_rename = Self::inline_rename_for(inline_rename, &path);
        let detail = if entry.is_directory {
            let key = browser
                .read(cx)
                .folder_count_key(entry.path.clone(), entry.modified_at);
            match browser.read(cx).folder_count(&key) {
                Some(FolderCountState::Ready(1)) => "1 item".to_owned(),
                Some(FolderCountState::Ready(count)) => format!("{count} items"),
                Some(FolderCountState::Unavailable) => "—".to_owned(),
                Some(FolderCountState::Loading) => "…".to_owned(),
                None => {
                    let count_browser = browser.clone();
                    cx.defer(move |cx| {
                        count_browser.update(cx, |browser, cx| {
                            browser.request_folder_count(key, cx);
                        });
                    });
                    "…".to_owned()
                }
            }
        } else {
            entry.detail.clone()
        };
        div()
            .id(SharedString::from(format!("grid:{}", path.display())))
            .w(px(122.))
            .p(px(7.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .rounded_xl()
            .border_1()
            .border_color(rgba(0x00000000))
            .cursor_pointer()
            .when(selected, |tile| {
                tile.bg(rgba(0x4e6f9b52)).border_color(rgba(0x79bde45c))
            })
            .when(!selected, |tile| {
                tile.hover(|style| style.bg(rgba(0xffffff0c)))
            })
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                let focus_handle = click_browser.read(cx).focus_handle.clone();
                click_browser.update(cx, |browser, cx| {
                    browser.cancel_active_inline_rename(cx);
                });
                window.focus(&focus_handle, cx);
                cx.stop_propagation();
            })
            .on_mouse_down(
                gpui::MouseButton::Right,
                move |event: &gpui::MouseDownEvent, window, cx| {
                    if !selected {
                        context_controller.update(cx, |controller, cx| {
                            controller.dispatch(
                                BrowserMessage::Select {
                                    path: context_path.clone(),
                                    mode: SelectionMode::Replace,
                                },
                                cx,
                            );
                        });
                    }
                    let menu = Self::entry_context_menu(
                        context_browser.clone(),
                        context_controller.clone(),
                        context_entry.clone(),
                        cx,
                    );
                    let _ = context_menu::show(menu, event.position, window, cx);
                    cx.stop_propagation();
                },
            )
            .on_click(move |event: &gpui::ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    Self::open_entry(controller.clone(), path.clone(), is_directory, cx);
                    return;
                }
                let selection_mode = if event.modifiers().shift || event.modifiers().secondary() {
                    SelectionMode::Toggle
                } else {
                    SelectionMode::Replace
                };
                controller.update(cx, |controller, cx| {
                    controller.dispatch(
                        BrowserMessage::Select {
                            path: path.clone(),
                            mode: selection_mode,
                        },
                        cx,
                    );
                });
            })
            .child(if entry.is_directory {
                FolderIcon::new()
                    .size(px(76.))
                    .glyph(icon)
                    .into_any_element()
            } else if entry.category == EntryCategory::Image {
                ImageThumbnail::new(
                    entry.path.clone(),
                    entry.modified_at,
                    entry.byte_len,
                    ImageThumbnailLayout::Grid,
                )
                .into_any_element()
            } else {
                Self::file_icon(show_category.then_some(entry.category)).into_any_element()
            })
            .child(if let Some(rename) = entry_rename {
                Self::inline_rename_field(rename, InlineRenameLayout::Grid, browser)
            } else {
                div()
                    .w_full()
                    .flex_none()
                    .px(px(3.))
                    .overflow_hidden()
                    .text_center()
                    .text_size(px(13.))
                    .line_clamp(2)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgba(0xf2f1f5e8))
                    .child(entry.name)
                    .into_any_element()
            })
            .child(
                div()
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(rgba(0x30a9e6c2))
                    .child(detail),
            )
            .into_any_element()
    }

    pub(super) fn grid_content(
        items: Vec<BrowserItem>,
        group_by_kind: bool,
        columns: usize,
        item_context: FileItemContext,
        inline_rename: Option<InlineRenameView>,
        scroll_state: &ListState,
        scrollbar_state: &ScrollbarState,
    ) -> impl IntoElement {
        let marquee_bounds = item_context.marquee_bounds;
        let controller = item_context.controller;
        let browser = item_context.browser;
        let rows = Self::grid_rows(items, group_by_kind, columns);
        if scroll_state.item_count() != rows.len() {
            scroll_state.reset_with_uniform_height(rows.len(), px(142.));
        }
        let rows = std::sync::Arc::new(rows);
        let rendered_rows = rows.clone();
        let rendered_browser = browser.clone();
        let scrollbar = Self::vertical_scrollbar("grid", scroll_state, scrollbar_state);
        let grid = list(
            scroll_state.clone(),
            move |index, _, cx| match rendered_rows[index].clone() {
                GridRow::Header { category, count } => div()
                    .h(px(38.))
                    .w_full()
                    .flex_none()
                    .px(px(22.))
                    .pt(px(8.))
                    .pb(px(2.))
                    .child(Self::category_header(category, count))
                    .into_any_element(),
                GridRow::Items {
                    entries,
                    show_category,
                } => div()
                    .h(px(if show_category { 128. } else { 142. }))
                    .w_full()
                    .flex_none()
                    .px(px(22.))
                    .flex()
                    .content_start()
                    .items_start()
                    .gap_x(px(Self::GRID_COLUMN_GAP))
                    .children(entries.into_iter().map(|entry| {
                        Self::folder_tile(
                            entry,
                            show_category,
                            controller.clone(),
                            rendered_browser.clone(),
                            inline_rename.clone(),
                            cx,
                        )
                    }))
                    .into_any_element(),
            },
        )
        .size_full()
        .pt(if group_by_kind { px(0.) } else { px(15.) })
        .pb(px(42.));
        let move_browser = browser.clone();
        let move_rows = rows.clone();
        let move_scroll = scroll_state.clone();
        let up_browser = browser.clone();
        let up_out_browser = browser.clone();
        let overlay = Self::marquee_overlay(marquee_bounds, scroll_state);
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                let focus_handle = browser.read(cx).focus_handle.clone();
                browser.update(cx, |browser, cx| {
                    browser.cancel_active_inline_rename(cx);
                    browser.begin_marquee_selection(event, cx);
                });
                window.focus(&focus_handle, cx);
                cx.stop_propagation();
            })
            .on_mouse_move(move |event, _, cx| {
                if event.dragging() {
                    move_browser.update(cx, |browser, cx| {
                        browser.update_grid_marquee(
                            event.position,
                            move_rows.as_slice(),
                            &move_scroll,
                            cx,
                        );
                    });
                    cx.stop_propagation();
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                up_browser.update(cx, |browser, cx| {
                    browser.finish_marquee_selection(cx);
                });
                cx.stop_propagation();
            })
            .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                up_out_browser.update(cx, |browser, cx| {
                    browser.finish_marquee_selection(cx);
                });
            })
            .child(grid)
            .when_some(overlay, |surface, overlay| surface.child(overlay))
            .child(scrollbar)
    }

    fn mini_folder(alias: bool) -> impl IntoElement {
        div()
            .relative()
            .w(px(28.))
            .h(px(23.))
            .flex_none()
            .child(FolderIcon::new().size(px(28.)))
            .when(alias, |folder| {
                folder.child(
                    div()
                        .absolute()
                        .left(px(-2.))
                        .bottom(px(-2.))
                        .size(px(9.))
                        .rounded_full()
                        .bg(rgba(0xf2f2f2f2))
                        .child(
                            svg()
                                .absolute()
                                .inset(px(1.5))
                                .path(LucideIcons::ArrowUpRight)
                                .text_color(rgba(0x252630e8)),
                        ),
                )
            })
    }

    fn list_header() -> impl IntoElement {
        div()
            .h(px(27.))
            .px(px(13.))
            .flex()
            .items_center()
            .border_b_1()
            .border_color(rgba(0xffffff17))
            .text_size(px(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgba(0xd6d4dbbf))
            .child(div().flex_1().min_w_0().child("Name"))
            .child(div().w(px(215.)).child("Date Modified"))
            .child(div().w(px(85.)).child("Size"))
            .child(div().w(px(90.)).child("Kind"))
    }

    fn list_row(
        entry: BrowserItem,
        index: usize,
        controller: Entity<BrowserController>,
        browser: Entity<FileBrowser>,
        inline_rename: Option<InlineRenameView>,
        _cx: &mut gpui::App,
    ) -> gpui::AnyElement {
        let path = entry.path.clone();
        let is_directory = entry.is_directory;
        let context_path = path.clone();
        let context_entry = entry.clone();
        let context_selected = entry.selected;
        let context_controller = controller.clone();
        let context_browser = browser.clone();
        let click_browser = browser.clone();
        let entry_rename = Self::inline_rename_for(inline_rename, &path);
        div()
            .id(SharedString::from(format!("list:{}", path.display())))
            .w_full()
            .h(px(46.))
            .px(px(13.))
            .flex()
            .items_center()
            .text_size(px(14.5))
            .text_color(rgba(0xe8e6ebdf))
            .when(index % 2 == 1, |row| row.bg(rgba(0xffffff08)))
            .when(entry.selected, |row| row.bg(rgba(0x3f78b66e)))
            .when(!entry.selected, |row| {
                row.hover(|style| style.bg(rgba(0xffffff10)))
            })
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                let focus_handle = click_browser.read(cx).focus_handle.clone();
                click_browser.update(cx, |browser, cx| {
                    browser.cancel_active_inline_rename(cx);
                });
                window.focus(&focus_handle, cx);
                cx.stop_propagation();
            })
            .on_mouse_down(
                gpui::MouseButton::Right,
                move |event: &gpui::MouseDownEvent, window, cx| {
                    if !context_selected {
                        context_controller.update(cx, |controller, cx| {
                            controller.dispatch(
                                BrowserMessage::Select {
                                    path: context_path.clone(),
                                    mode: SelectionMode::Replace,
                                },
                                cx,
                            );
                        });
                    }
                    let menu = Self::entry_context_menu(
                        context_browser.clone(),
                        context_controller.clone(),
                        context_entry.clone(),
                        cx,
                    );
                    let _ = context_menu::show(menu, event.position, window, cx);
                    cx.stop_propagation();
                },
            )
            .on_click(move |event: &gpui::ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    Self::open_entry(controller.clone(), path.clone(), is_directory, cx);
                    return;
                }
                let selection_mode = if event.modifiers().shift || event.modifiers().secondary() {
                    SelectionMode::Toggle
                } else {
                    SelectionMode::Replace
                };
                controller.update(cx, |controller, cx| {
                    controller.dispatch(
                        BrowserMessage::Select {
                            path: path.clone(),
                            mode: selection_mode,
                        },
                        cx,
                    );
                });
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pr(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .child(if entry.is_directory {
                                Self::mini_folder(entry.alias).into_any_element()
                            } else if entry.category == EntryCategory::Image {
                                ImageThumbnail::new(
                                    entry.path.clone(),
                                    entry.modified_at,
                                    entry.byte_len,
                                    ImageThumbnailLayout::List,
                                )
                                .into_any_element()
                            } else {
                                svg()
                                    .path(LucideIcons::File)
                                    .size(px(25.))
                                    .text_color(rgba(0xd8d8decc))
                                    .into_any_element()
                            })
                            .child(if let Some(rename) = entry_rename {
                                Self::inline_rename_field(rename, InlineRenameLayout::List, browser)
                            } else {
                                div().truncate().child(entry.name).into_any_element()
                            }),
                    ),
            )
            .child(
                div()
                    .w(px(215.))
                    .text_color(rgba(0xd5d3dac2))
                    .child(entry.modified),
            )
            .child(
                div()
                    .w(px(85.))
                    .text_color(rgba(0xd5d3daa6))
                    .child(entry.size),
            )
            .child(
                div()
                    .w(px(90.))
                    .text_color(rgba(0xd5d3daa6))
                    .child(entry.kind),
            )
            .into_any_element()
    }

    fn empty_list_row(index: usize) -> impl IntoElement {
        div()
            .w_full()
            .h(px(26.))
            .when(index % 2 == 1, |row| row.bg(rgba(0xffffff08)))
    }

    pub(super) fn list_content(
        items: Vec<BrowserItem>,
        item_context: FileItemContext,
        inline_rename: Option<InlineRenameView>,
        scroll_state: &ListState,
        scrollbar_state: &ScrollbarState,
    ) -> impl IntoElement {
        let marquee_bounds = item_context.marquee_bounds;
        let controller = item_context.controller;
        let browser = item_context.browser;
        let rows = Self::file_list_rows(items);
        if scroll_state.item_count() != rows.len() {
            scroll_state.reset_with_uniform_height(rows.len(), px(46.));
        }
        let rows = std::sync::Arc::new(rows);
        let rendered_rows = rows.clone();
        let rendered_browser = browser.clone();
        let scrollbar = Self::vertical_scrollbar("list", scroll_state, scrollbar_state);
        let list_rows = list(
            scroll_state.clone(),
            move |index, _, cx| match rendered_rows[index].clone() {
                FileListRow::Entry {
                    entry,
                    stripe_index,
                } => Self::list_row(
                    entry,
                    stripe_index,
                    controller.clone(),
                    rendered_browser.clone(),
                    inline_rename.clone(),
                    cx,
                ),
                FileListRow::Empty { stripe_index } => {
                    Self::empty_list_row(stripe_index).into_any_element()
                }
            },
        )
        .size_full();
        let move_browser = browser.clone();
        let move_rows = rows.clone();
        let move_scroll = scroll_state.clone();
        let up_browser = browser.clone();
        let up_out_browser = browser.clone();
        let overlay = Self::marquee_overlay(marquee_bounds, scroll_state);
        div()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(Self::list_header())
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        let focus_handle = browser.read(cx).focus_handle.clone();
                        browser.update(cx, |browser, cx| {
                            browser.cancel_active_inline_rename(cx);
                            browser.begin_marquee_selection(event, cx);
                        });
                        window.focus(&focus_handle, cx);
                        cx.stop_propagation();
                    })
                    .on_mouse_move(move |event, _, cx| {
                        if event.dragging() {
                            move_browser.update(cx, |browser, cx| {
                                browser.update_list_marquee(
                                    event.position,
                                    move_rows.as_slice(),
                                    &move_scroll,
                                    cx,
                                );
                            });
                            cx.stop_propagation();
                        }
                    })
                    .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                        up_browser.update(cx, |browser, cx| {
                            browser.finish_marquee_selection(cx);
                        });
                        cx.stop_propagation();
                    })
                    .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                        up_out_browser.update(cx, |browser, cx| {
                            browser.finish_marquee_selection(cx);
                        });
                    })
                    .child(list_rows)
                    .when_some(overlay, |surface, overlay| surface.child(overlay))
                    .child(scrollbar),
            )
    }
}
