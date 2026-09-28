mod actions;
mod context_menu;
mod file_actions;
mod files;
mod inline_rename;
mod sidebar;
mod toolbar;

pub(crate) use actions::init as init_key_bindings;

use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    time::{Duration, SystemTime},
};

use gpui::{
    Animation, AnimationExt as _, Bounds, Entity, FocusHandle, Focusable, IntoElement,
    ListAlignment, ListState, MouseButton, MouseDownEvent, Pixels, Point, Render, Window, div,
    point, prelude::*, px, rgb, rgba, svg,
};
use rift_core::application::{BrowserMessage, LoadState, SortField, ViewMode};
use uic::{
    assets::LucideIcons,
    components::{
        context_menu as uic_context_menu,
        input::{InputEvent, TextInput},
        scrollbar::ScrollbarState,
    },
};

use crate::{
    config::AppConfig,
    presentation::{BrowserController, BrowserItem, NavigationController, present_browser},
};

use self::files::FileItemContext;
use self::inline_rename::InlineRenameState;
use self::toolbar::ToolbarState;

const SIDEBAR_WIDTH: f32 = 300.0;
const SIDEBAR_ANIMATION_DURATION: Duration = Duration::from_millis(230);
const MARQUEE_DRAG_THRESHOLD: f32 = 3.0;

fn sidebar_animation_easing(phase: f32) -> f32 {
    1.0 - (1.0 - phase).powi(3)
}

#[derive(Clone)]
pub(super) struct MarqueeSelection {
    start: Point<Pixels>,
    current: Point<Pixels>,
    initial_selection: BTreeSet<PathBuf>,
    additive: bool,
    active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct FolderCountKey {
    path: PathBuf,
    modified_at: Option<SystemTime>,
    include_hidden: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum FolderCountState {
    Loading,
    Ready(usize),
    Unavailable,
}

pub(crate) struct FileBrowser {
    pub(super) controller: Entity<BrowserController>,
    pub(super) navigation: Entity<NavigationController>,
    pub(super) grid_scroll: ListState,
    pub(super) list_scroll: ListState,
    pub(super) grid_scrollbar: ScrollbarState,
    pub(super) list_scrollbar: ScrollbarState,
    pub(super) search_input: Entity<TextInput>,
    focus_handle: FocusHandle,
    sidebar_visible: bool,
    sidebar_animated: bool,
    inline_rename: Option<InlineRenameState>,
    vim_trash_confirmation: Option<Vec<PathBuf>>,
    rendered_directory: std::path::PathBuf,
    rendered_directory_request: Option<u64>,
    rendered_show_hidden_files: bool,
    rendered_item_count: usize,
    rendered_active_selection: Option<PathBuf>,
    grid_columns: usize,
    grid_grouped: bool,
    marquee_selection: Option<MarqueeSelection>,
    folder_counts: HashMap<FolderCountKey, FolderCountState>,
    folder_count_generation: u64,
}

impl FileBrowser {
    pub(crate) fn new(
        controller: Entity<BrowserController>,
        navigation: Entity<NavigationController>,
        sidebar_visible: bool,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        cx.observe(&controller, |_, _, cx| cx.notify()).detach();
        cx.observe(&navigation, |_, _, cx| cx.notify()).detach();
        let search_input = cx.new(|cx| TextInput::new(cx).placeholder("Search"));
        cx.subscribe(&search_input, |browser, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change(_)) {
                browser.vim_trash_confirmation = None;
                browser.grid_scroll.reset(browser.grid_scroll.item_count());
                browser.list_scroll.reset(browser.list_scroll.item_count());
                browser.controller.update(cx, |controller, cx| {
                    controller.dispatch(BrowserMessage::ClearSelection, cx);
                });
                cx.notify();
            }
        })
        .detach();
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            controller,
            navigation,
            grid_scroll: ListState::new(0, ListAlignment::Top, px(220.)),
            list_scroll: ListState::new(0, ListAlignment::Top, px(160.)),
            grid_scrollbar: ScrollbarState::new(),
            list_scrollbar: ScrollbarState::new(),
            search_input,
            focus_handle,
            sidebar_visible,
            sidebar_animated: false,
            inline_rename: None,
            vim_trash_confirmation: None,
            rendered_directory: std::path::PathBuf::new(),
            rendered_directory_request: None,
            rendered_show_hidden_files: false,
            rendered_item_count: 0,
            rendered_active_selection: None,
            grid_columns: 1,
            grid_grouped: false,
            marquee_selection: None,
            folder_counts: HashMap::new(),
            folder_count_generation: 0,
        }
    }

    fn invalidate_folder_counts(&mut self) {
        self.folder_counts.clear();
        self.folder_count_generation = self.folder_count_generation.wrapping_add(1);
    }

    pub(super) fn folder_count_key(
        &self,
        path: PathBuf,
        modified_at: Option<SystemTime>,
    ) -> FolderCountKey {
        FolderCountKey {
            path,
            modified_at,
            include_hidden: self.rendered_show_hidden_files,
        }
    }

    pub(super) fn folder_count(&self, key: &FolderCountKey) -> Option<FolderCountState> {
        self.folder_counts.get(key).copied()
    }

    pub(super) fn request_folder_count(
        &mut self,
        key: FolderCountKey,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.folder_counts.contains_key(&key) {
            return;
        }
        self.folder_counts
            .insert(key.clone(), FolderCountState::Loading);
        let generation = self.folder_count_generation;
        let path = key.path.clone();
        let include_hidden = key.include_hidden;
        let count = self.controller.update(cx, |controller, cx| {
            controller.count_directory_items(path, include_hidden, cx)
        });

        cx.spawn(async move |this, cx| {
            let result = count.await;
            let Some(this) = this.upgrade() else {
                return;
            };
            this.update(cx, |browser, cx| {
                if browser.folder_count_generation != generation {
                    return;
                }
                let state = match result {
                    Ok(count) => FolderCountState::Ready(count),
                    Err(error) => {
                        log::debug!("unable to count directory items: {error}");
                        FolderCountState::Unavailable
                    }
                };
                browser.folder_counts.insert(key, state);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn begin_marquee_selection(
        &mut self,
        event: &MouseDownEvent,
        cx: &mut gpui::Context<Self>,
    ) {
        let initial_selection = self.controller.read(cx).state().selection().clone();
        let additive = event.modifiers.shift;
        self.marquee_selection = Some(MarqueeSelection {
            start: event.position,
            current: event.position,
            initial_selection,
            additive,
            active: false,
        });

        if !additive {
            self.controller.update(cx, |controller, cx| {
                controller.dispatch(BrowserMessage::ClearSelection, cx);
            });
        }
        cx.notify();
    }

    pub(super) fn marquee_bounds_to(&self, current: Point<Pixels>) -> Option<Bounds<Pixels>> {
        let marquee = self.marquee_selection.as_ref()?;
        let left = marquee.start.x.min(current.x);
        let top = marquee.start.y.min(current.y);
        let right = marquee.start.x.max(current.x);
        let bottom = marquee.start.y.max(current.y);
        Some(Bounds::from_corners(point(left, top), point(right, bottom)))
    }

    pub(super) fn update_marquee_selection(
        &mut self,
        current: Point<Pixels>,
        hits: impl IntoIterator<Item = PathBuf>,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(marquee) = self.marquee_selection.as_mut() else {
            return;
        };
        marquee.current = current;
        let delta = current.relative_to(&marquee.start);
        marquee.active = marquee.active
            || delta.x.abs() >= px(MARQUEE_DRAG_THRESHOLD)
            || delta.y.abs() >= px(MARQUEE_DRAG_THRESHOLD);
        if !marquee.active {
            return;
        }

        let mut selection = if marquee.additive {
            marquee.initial_selection.clone()
        } else {
            BTreeSet::new()
        };
        selection.extend(hits);
        let selection_changed = self.controller.read(cx).state().selection() != &selection;
        if selection_changed {
            self.controller.update(cx, |controller, cx| {
                controller.dispatch(
                    BrowserMessage::SetSelection(selection.into_iter().collect()),
                    cx,
                );
            });
        } else {
            cx.notify();
        }
    }

    pub(super) fn finish_marquee_selection(&mut self, cx: &mut gpui::Context<Self>) {
        if self.marquee_selection.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn marquee_bounds(&self) -> Option<Bounds<Pixels>> {
        let marquee = self.marquee_selection.as_ref()?;
        marquee.active.then_some(Bounds::from_corners(
            point(
                marquee.start.x.min(marquee.current.x),
                marquee.start.y.min(marquee.current.y),
            ),
            point(
                marquee.start.x.max(marquee.current.x),
                marquee.start.y.max(marquee.current.y),
            ),
        ))
    }

    pub(super) fn set_sidebar_visible(&mut self, visible: bool, cx: &mut gpui::Context<Self>) {
        if self.sidebar_visible == visible {
            return;
        }
        self.sidebar_visible = visible;
        self.sidebar_animated = true;
        AppConfig::update_sidebar_visible(cx, visible);
        cx.notify();
    }

    fn content(&mut self, window: &Window, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let compact_toolbar = window.bounds().size.width < px(1050.);
        let (
            view_mode,
            can_go_back,
            can_go_forward,
            can_go_up,
            current_directory,
            active_selection,
            load_state,
            all_items,
            sort,
            has_selection,
            show_hidden_files,
            is_trash,
        ) = {
            let controller = self.controller.read(cx);
            let state = controller.state();
            (
                state.view_mode(),
                state.can_go_back(),
                state.can_go_forward(),
                state.can_go_up(),
                state.current_directory().to_path_buf(),
                state.active_selection().map(PathBuf::from),
                state.load_state().clone(),
                present_browser(state),
                state.sort(),
                !state.selection().is_empty(),
                state.show_hidden_files(),
                state.is_trash(),
            )
        };
        let search_query = self.search_input.read(cx).value().trim().to_owned();
        let selection_status = selection_status(&all_items);
        if self.vim_trash_confirmation.as_ref().is_some_and(|paths| {
            let selected = self.controller.read(cx).selected_paths();
            selected.len() != paths.len() || selected.iter().any(|path| !paths.contains(path))
        }) {
            self.vim_trash_confirmation = None;
        }
        let all_item_count = all_items.len();
        let items = if search_query.is_empty() {
            all_items
        } else {
            let normalized_query = search_query.to_lowercase();
            all_items
                .into_iter()
                .filter(|item| item.name.to_lowercase().contains(&normalized_query))
                .collect()
        };
        if self.rendered_active_selection != active_selection {
            self.rendered_active_selection = active_selection.clone();
            if let Some(path) = active_selection {
                let browser = cx.entity();
                cx.defer(move |cx| {
                    browser.update(cx, |browser, cx| {
                        browser.reveal_path(&path, cx);
                        cx.notify();
                    });
                });
            }
        }
        let directory_changed = self.rendered_directory != current_directory;
        let hidden_policy_changed = self.rendered_show_hidden_files != show_hidden_files;
        let request_id = match &load_state {
            LoadState::Loading { request_id, .. } => Some(*request_id),
            _ => None,
        };
        let directory_reloaded =
            request_id.is_some() && self.rendered_directory_request != request_id;
        if directory_changed || hidden_policy_changed || directory_reloaded {
            self.rendered_show_hidden_files = show_hidden_files;
            self.rendered_directory_request = request_id.or(self.rendered_directory_request);
            self.invalidate_folder_counts();
        }
        if directory_changed {
            self.rendered_directory = current_directory.clone();
            self.vim_trash_confirmation = None;
            self.marquee_selection = None;
            self.grid_scroll.reset(self.grid_scroll.item_count());
            self.list_scroll.reset(self.list_scroll.item_count());
            let search_input = self.search_input.clone();
            cx.defer(move |cx| {
                search_input.update(cx, |input, cx| input.clear(cx));
            });
        }
        let item_count = items.len();
        if self.rendered_item_count != item_count {
            self.rendered_item_count = item_count;
            let entity = cx.entity();
            cx.defer(move |cx| cx.notify(entity.entity_id()));
        }
        let load_notice = match &load_state {
            LoadState::Loading { .. } if items.is_empty() => Some("Loading folder…".to_owned()),
            LoadState::Failed { message, .. } => Some(message.clone()),
            _ => None,
        };
        let inline_rename = self.inline_rename_view();
        let marquee_bounds = self.marquee_bounds();
        let group_by_kind = sort.field == SortField::Kind;
        let content_width = if self.sidebar_visible {
            (window.bounds().size.width - px(SIDEBAR_WIDTH)).max(px(1.))
        } else {
            window.bounds().size.width
        };
        let grid_columns = Self::grid_column_count(content_width);
        if self.grid_columns != grid_columns || self.grid_grouped != group_by_kind {
            self.grid_columns = grid_columns;
            self.grid_grouped = group_by_kind;
            self.grid_scroll.reset(0);
        }

        div()
            .flex_1()
            .flex()
            .flex_col()
            .child(self.toolbar(
                ToolbarState {
                    view_mode,
                    can_go_back,
                    can_go_forward,
                    can_go_up,
                    current_directory: current_directory.clone(),
                    compact: compact_toolbar,
                    sidebar_visible: self.sidebar_visible,
                    sort,
                    has_selection,
                    show_hidden_files,
                    is_trash,
                },
                cx,
            ))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                            let menu = Self::blank_context_menu(
                                this.controller.clone(),
                                this.navigation.clone(),
                                cx,
                            );
                            let _ = uic_context_menu::show(menu, event.position, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .child(match view_mode {
                        ViewMode::Grid => Self::grid_content(
                            items,
                            group_by_kind,
                            grid_columns,
                            FileItemContext {
                                controller: self.controller.clone(),
                                browser: cx.entity(),
                                marquee_bounds,
                            },
                            inline_rename,
                            &self.grid_scroll,
                            &self.grid_scrollbar,
                        )
                        .into_any_element(),
                        ViewMode::List => Self::list_content(
                            items,
                            FileItemContext {
                                controller: self.controller.clone(),
                                browser: cx.entity(),
                                marquee_bounds,
                            },
                            inline_rename,
                            &self.list_scroll,
                            &self.list_scrollbar,
                        )
                        .into_any_element(),
                    })
                    .when(items_are_empty(&load_state, item_count), |body| {
                        let (icon, message) = if search_query.is_empty() {
                            (LucideIcons::FolderOpen, "This folder is empty")
                        } else {
                            (LucideIcons::SearchX, "No matching items")
                        };
                        body.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap(px(10.))
                                .text_color(rgba(0xbfc1cad1))
                                .child(svg().path(icon).size(px(44.)).text_color(rgba(0x8e929f8c)))
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .child(message),
                                ),
                        )
                    })
                    .when_some(load_notice, |body, message| {
                        body.child(
                            div()
                                .absolute()
                                .left(px(22.))
                                .bottom(px(18.))
                                .px(px(12.))
                                .py(px(8.))
                                .rounded_lg()
                                .bg(rgba(0x151722e8))
                                .text_xs()
                                .text_color(rgba(0xe5e3e9d9))
                                .child(message),
                        )
                    }),
            )
            .child(Self::status_bar(
                item_count,
                all_item_count,
                !search_query.is_empty(),
                selection_status,
                self.vim_trash_confirmation.as_deref(),
                &load_state,
            ))
            .into_any_element()
    }

    fn status_bar(
        item_count: usize,
        all_item_count: usize,
        search_active: bool,
        selection_status: Option<String>,
        vim_trash_confirmation: Option<&[PathBuf]>,
        load_state: &LoadState,
    ) -> gpui::AnyElement {
        if let (LoadState::Idle, Some(paths)) = (load_state, vim_trash_confirmation) {
            return div()
                .h(px(44.))
                .px(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .border_t_1()
                .border_color(rgba(0xff69697a))
                .bg(rgba(0x321e26f2))
                .text_color(rgba(0xfff3f3f2))
                .child(
                    div()
                        .size(px(27.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(rgba(0xff5f6429))
                        .child(
                            svg()
                                .path(LucideIcons::Trash2)
                                .size(px(15.))
                                .text_color(rgba(0xff7777ff)),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(vim_trash_confirmation_status(paths.len())),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .min_w(px(24.))
                                .h(px(23.))
                                .px(px(7.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(rgba(0xff8a8ab8))
                                .bg(rgba(0xff656533))
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child("Y"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgba(0xffb9b9e8))
                                .child("Confirm"),
                        ),
                )
                .child(div().w(px(1.)).h(px(18.)).bg(rgba(0xffffff20)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .h(px(23.))
                                .px(px(7.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(rgba(0xffffff24))
                                .bg(rgba(0xffffff0e))
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgba(0xe8e6edcc))
                                .child("N / Esc"),
                        )
                        .child(div().text_xs().text_color(rgba(0xc7c4ceaa)).child("Cancel")),
                )
                .into_any_element();
        }

        let status = match load_state {
            LoadState::Loading { .. } => "Loading".to_owned(),
            LoadState::Failed { .. } => "Unavailable".to_owned(),
            LoadState::Idle => match selection_status {
                Some(status) => status,
                None if search_active => format!("{item_count} of {all_item_count} items"),
                None => format!("{item_count} items"),
            },
        };
        div()
            .h(px(27.))
            .px(px(12.))
            .flex()
            .items_center()
            .justify_end()
            .border_t_1()
            .border_color(rgba(0xffffff10))
            .text_xs()
            .text_color(rgba(0xc3c1c99c))
            .child(div().max_w(px(480.)).truncate().child(status))
            .into_any_element()
    }
}

fn selection_status(items: &[BrowserItem]) -> Option<String> {
    let mut selected = items.iter().filter(|item| item.selected);
    let first = selected.next()?;
    let second = selected.next();

    if second.is_none() {
        return Some(if first.is_directory {
            "1 folder selected".to_owned()
        } else {
            format!("1 file selected · {}", first.size)
        });
    }

    Some(format!("{} items selected", 2 + selected.count()))
}

fn vim_trash_confirmation_status(item_count: usize) -> String {
    let noun = if item_count == 1 { "item" } else { "items" };
    format!("Move {item_count} {noun} to Trash?")
}

fn items_are_empty(load_state: &LoadState, item_count: usize) -> bool {
    item_count == 0 && matches!(load_state, LoadState::Idle)
}

impl Render for FileBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let vim_mode = AppConfig::vim_mode(cx);
        if !vim_mode {
            self.vim_trash_confirmation = None;
        }
        let sidebar = self.sidebar(cx);
        let content = self.content(window, cx);
        let visible = self.sidebar_visible;
        let content_width = if visible {
            (window.bounds().size.width - px(SIDEBAR_WIDTH)).max(px(1.0))
        } else {
            window.bounds().size.width
        };
        let content_frame = div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(content_width)
            .min_w_0()
            .flex()
            .child(content);
        let content_frame = if self.sidebar_animated {
            content_frame
                .with_animation(
                    if visible {
                        "content-expand-sidebar"
                    } else {
                        "content-collapse-sidebar"
                    },
                    Animation::new(SIDEBAR_ANIMATION_DURATION)
                        .with_easing(sidebar_animation_easing),
                    move |content, phase| {
                        let progress = if visible { phase } else { 1.0 - phase };
                        content.left(px(SIDEBAR_WIDTH * progress))
                    },
                )
                .into_any_element()
        } else {
            content_frame
                .left(px(if visible { SIDEBAR_WIDTH } else { 0.0 }))
                .into_any_element()
        };
        let key_context = match (
            self.inline_rename.is_some(),
            vim_mode,
            self.vim_trash_confirmation.is_some(),
        ) {
            (true, true, _) => "FileBrowser editing vim",
            (true, false, _) => "FileBrowser editing",
            (false, true, true) => "FileBrowser vim trash_confirm",
            (false, true, false) => "FileBrowser vim",
            (false, false, _) => "FileBrowser",
        };
        div()
            .relative()
            .key_context(key_context)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::open_selection))
            .on_action(cx.listener(Self::move_selection_left))
            .on_action(cx.listener(Self::move_selection_right))
            .on_action(cx.listener(Self::move_selection_up))
            .on_action(cx.listener(Self::move_selection_down))
            .on_action(cx.listener(Self::extend_selection_left))
            .on_action(cx.listener(Self::extend_selection_right))
            .on_action(cx.listener(Self::extend_selection_up))
            .on_action(cx.listener(Self::extend_selection_down))
            .on_action(cx.listener(Self::select_first_item))
            .on_action(cx.listener(Self::select_last_item))
            .on_action(cx.listener(Self::select_previous_page))
            .on_action(cx.listener(Self::select_next_page))
            .on_action(cx.listener(Self::go_to_parent_directory))
            .on_action(cx.listener(Self::go_back_directory))
            .on_action(cx.listener(Self::go_forward_directory))
            .on_action(cx.listener(Self::quick_look_selection))
            .on_action(cx.listener(Self::rename_selection))
            .on_action(cx.listener(Self::get_info_selection))
            .on_action(cx.listener(Self::copy_items))
            .on_action(cx.listener(Self::paste_items))
            .on_action(cx.listener(Self::trash_items))
            .on_action(cx.listener(Self::request_vim_trash))
            .on_action(cx.listener(Self::confirm_vim_trash))
            .on_action(cx.listener(Self::cancel_vim_trash))
            .on_action(cx.listener(Self::vim_trash_immediately))
            .on_action(cx.listener(Self::permanently_delete_items))
            .on_action(cx.listener(Self::refresh_directory))
            .on_action(cx.listener(Self::new_folder))
            .on_action(cx.listener(Self::add_item))
            .on_action(cx.listener(Self::select_all_items))
            .on_action(cx.listener(Self::clear_selection))
            .on_action(cx.listener(Self::toggle_hidden_files))
            .size_full()
            .overflow_hidden()
            .bg(rgb(0x20222e))
            .child(sidebar)
            .child(content_frame)
    }
}

impl Focusable for FileBrowser {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rift_core::domain::EntryCategory;

    use super::{BrowserItem, selection_status, vim_trash_confirmation_status};
    use crate::presentation::ItemIcon;

    fn item(name: &str, is_directory: bool, size: &str, selected: bool) -> BrowserItem {
        BrowserItem {
            path: PathBuf::from(name),
            name: name.to_owned(),
            detail: if is_directory { "Folder" } else { size }.to_owned(),
            modified: "—".to_owned(),
            size: size.to_owned(),
            byte_len: 0,
            modified_at: None,
            kind: if is_directory { "Folder" } else { "Document" }.to_owned(),
            category: if is_directory {
                EntryCategory::Folder
            } else {
                EntryCategory::Document
            },
            icon: if is_directory {
                ItemIcon::Folder
            } else {
                ItemIcon::File
            },
            is_directory,
            alias: false,
            selected,
        }
    }

    #[test]
    fn status_summarizes_the_current_selection() {
        let folder = item("Photos", true, "—", true);
        let file = item("notes.md", false, "1.5 KB", true);

        assert_eq!(
            selection_status(std::slice::from_ref(&folder)).as_deref(),
            Some("1 folder selected")
        );
        assert_eq!(
            selection_status(std::slice::from_ref(&file)).as_deref(),
            Some("1 file selected · 1.5 KB")
        );
        assert_eq!(
            selection_status(&[folder, file]).as_deref(),
            Some("2 items selected")
        );
    }

    #[test]
    fn vim_trash_confirmation_status_uses_the_selected_item_count() {
        assert_eq!(vim_trash_confirmation_status(1), "Move 1 item to Trash?");
        assert_eq!(vim_trash_confirmation_status(3), "Move 3 items to Trash?");
    }
}
