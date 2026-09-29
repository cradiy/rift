mod actions;
mod context_menu;
mod drag_drop;
mod file_actions;
mod files;
mod inline_rename;
mod sidebar;
mod toolbar;
mod which_key;

pub(crate) use actions::init as init_key_bindings;
pub(crate) use drag_drop::{
    FileDrag, drop_external_files_into, drop_files_into, file_drag_preview, finish_file_drag,
    trash_dragged_files,
};

use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    time::{Duration, SystemTime},
};

use gpui::{
    Animation, AnimationExt as _, App, Bounds, Entity, ExternalPaths, FocusHandle, Focusable,
    IntoElement, ListAlignment, ListState, MouseButton, MouseDownEvent, Pixels, Point, Render,
    Window, div, point, prelude::*, px, rgb, rgba, svg,
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
    presentation::{
        BrowserController, BrowserItem, NavigationController, format_size, present_browser,
    },
};

use self::files::FileItemContext;
use self::inline_rename::InlineRenameState;
use self::toolbar::ToolbarState;
use self::which_key::WhichKeyState;

pub(crate) const SIDEBAR_WIDTH: f32 = 300.0;
pub(crate) const TAB_BAR_HEIGHT: f32 = 42.0;
pub(crate) const TOOLBAR_HEIGHT: f32 = 72.0;
pub(crate) const SIDEBAR_ANIMATION_DURATION: Duration = Duration::from_millis(230);
const MARQUEE_DRAG_THRESHOLD: f32 = 3.0;

pub(crate) fn sidebar_animation_easing(phase: f32) -> f32 {
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
    tab_bar_visible: bool,
    inline_rename: Option<InlineRenameState>,
    vim_trash_confirmation: Option<Vec<PathBuf>>,
    which_key: WhichKeyState,
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
    pub(crate) fn controller_entity(&self) -> Entity<BrowserController> {
        self.controller.clone()
    }

    pub(crate) fn current_directory(&self, cx: &App) -> PathBuf {
        self.controller
            .read(cx)
            .state()
            .current_directory()
            .to_path_buf()
    }

    pub(crate) fn tab_title(&self, cx: &App) -> String {
        let controller = self.controller.read(cx);
        let state = controller.state();
        if state.is_trash() {
            return "Trash".to_owned();
        }
        state
            .current_directory()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| state.current_directory().display().to_string())
    }

    pub(crate) fn sidebar_visible(&self) -> bool {
        self.sidebar_visible
    }

    pub(crate) fn sidebar_is_animating(&self) -> bool {
        self.sidebar_animated
    }

    pub(crate) fn set_tab_bar_visible(&mut self, visible: bool, cx: &mut gpui::Context<Self>) {
        if self.tab_bar_visible == visible {
            return;
        }
        self.tab_bar_visible = visible;
        cx.notify();
    }

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
            tab_bar_visible: false,
            inline_rename: None,
            vim_trash_confirmation: None,
            which_key: WhichKeyState::default(),
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

    fn request_selected_folder_counts(
        &mut self,
        items: &[BrowserItem],
        cx: &mut gpui::Context<Self>,
    ) {
        let missing = items
            .iter()
            .filter(|item| item.selected && item.is_directory)
            .map(|item| self.folder_count_key(item.path.clone(), item.modified_at))
            .filter(|key| !self.folder_counts.contains_key(key))
            .collect::<Vec<_>>();

        for key in missing {
            self.request_folder_count(key, cx);
        }
    }

    pub(super) fn begin_marquee_selection(
        &mut self,
        event: &MouseDownEvent,
        cx: &mut gpui::Context<Self>,
    ) {
        self.cancel_which_key(cx);
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

    pub(super) fn finish_marquee_selection(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        let had_marquee = self.marquee_selection.take().is_some();
        if had_marquee {
            cx.notify();
        }
        had_marquee
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
        if self.vim_trash_confirmation.as_ref().is_some_and(|paths| {
            let selected = self.controller.read(cx).selected_paths();
            selected.len() != paths.len() || selected.iter().any(|path| !paths.contains(path))
        }) {
            self.vim_trash_confirmation = None;
        }
        let all_item_count = all_items.len();
        let items = if search_query.is_empty() {
            all_items.clone()
        } else {
            let normalized_query = search_query.to_lowercase();
            all_items
                .iter()
                .filter(|item| item.name.to_lowercase().contains(&normalized_query))
                .cloned()
                .collect()
        };
        if self.rendered_active_selection != active_selection {
            self.cancel_which_key(cx);
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
        self.request_selected_folder_counts(&all_items, cx);
        let selection_status = selection_status(&all_items, |item| {
            let key = self.folder_count_key(item.path.clone(), item.modified_at);
            match self.folder_count(&key) {
                Some(FolderCountState::Ready(count)) => Some(count),
                _ => None,
            }
        });
        if directory_changed {
            self.rendered_directory = current_directory.clone();
            self.vim_trash_confirmation = None;
            self.cancel_which_key(cx);
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
        let content_drop_directory = current_directory.clone();
        let content_drop_controller = self.controller.clone();
        let external_drop_directory = current_directory.clone();
        let external_drop_controller = self.controller.clone();
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
            .when(self.tab_bar_visible, |content| {
                content.child(div().h(px(TAB_BAR_HEIGHT)).flex_none())
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .when(!is_trash, |body| {
                        body.drag_over::<FileDrag>(|style, _, _, _| style.bg(rgba(0x2caee808)))
                            .on_drop(move |drag: &FileDrag, window, cx| {
                                cx.stop_propagation();
                                drop_files_into(
                                    drag,
                                    content_drop_directory.clone(),
                                    content_drop_controller.clone(),
                                    window,
                                    cx,
                                );
                            })
                            .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgba(0x2caee808)))
                            .on_drop(move |paths: &ExternalPaths, _, cx| {
                                cx.stop_propagation();
                                drop_external_files_into(
                                    paths,
                                    external_drop_directory.clone(),
                                    external_drop_controller.clone(),
                                    cx,
                                );
                            })
                    })
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

        let showing_selection = matches!(load_state, LoadState::Idle) && selection_status.is_some();
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
            .h(px(34.))
            .px(px(12.))
            .flex()
            .items_center()
            .justify_end()
            .border_t_1()
            .border_color(rgba(0xffffff10))
            .text_xs()
            .text_color(rgba(0xc3c1c99c))
            .child(
                div()
                    .min_w_0()
                    .whitespace_nowrap()
                    .when(showing_selection, |label| {
                        label
                            .px(px(11.))
                            .py(px(4.))
                            .rounded_full()
                            .bg(rgba(0x0e1018a8))
                            .text_size(px(12.5))
                            .text_color(rgba(0xf1f0f4e8))
                    })
                    .child(status),
            )
            .into_any_element()
    }
}

fn selection_status(
    items: &[BrowserItem],
    mut folder_item_count: impl FnMut(&BrowserItem) -> Option<usize>,
) -> Option<String> {
    let selected = items
        .iter()
        .filter(|item| item.selected)
        .collect::<Vec<_>>();
    let first = *selected.first()?;

    if selected.len() == 1 {
        let detail = if first.is_directory {
            folder_item_count(first).map(|count| {
                let noun = if count == 1 { "item" } else { "items" };
                format!("{count} {noun}")
            })
        } else {
            Some(first.size.clone())
        };
        return Some(match detail {
            Some(detail) => format!("\"{}\" selected ({detail})", first.name),
            None => format!("\"{}\" selected", first.name),
        });
    }

    let folders = selected
        .iter()
        .copied()
        .filter(|item| item.is_directory)
        .collect::<Vec<_>>();
    let other_items = selected
        .iter()
        .copied()
        .filter(|item| !item.is_directory)
        .collect::<Vec<_>>();
    let mut parts = Vec::with_capacity(2);

    if !folders.is_empty() {
        let count = folders.len();
        let noun = if count == 1 { "folder" } else { "folders" };
        let contained_count = folders.iter().try_fold(0usize, |total, item| {
            folder_item_count(item).map(|count| total.saturating_add(count))
        });
        let detail = contained_count.map_or_else(String::new, |contained_count| {
            let contained_noun = if contained_count == 1 {
                "item"
            } else {
                "items"
            };
            format!(" (containing a total of {contained_count} {contained_noun})")
        });
        parts.push(format!("{count} {noun} selected{detail}"));
    }

    if !other_items.is_empty() {
        let count = other_items.len();
        let noun = match (folders.is_empty(), count) {
            (true, 1) => "item",
            (true, _) => "items",
            (false, 1) => "other item",
            (false, _) => "other items",
        };
        let byte_len = other_items
            .iter()
            .fold(0u64, |total, item| total.saturating_add(item.byte_len));
        parts.push(format!(
            "{count} {noun} selected ({})",
            format_size(byte_len)
        ));
    }

    Some(parts.join(", "))
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
        if !vim_mode || !self.focus_handle.is_focused(window) {
            self.which_key.reset();
        }
        let sidebar = self.sidebar(cx);
        let content = self.content(window, cx);
        let which_key = self.render_which_key();
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
            self.which_key.context_token(),
        ) {
            (true, true, _, _) => "FileBrowser editing vim",
            (true, false, _, _) => "FileBrowser editing",
            (false, true, true, _) => "FileBrowser vim trash_confirm",
            (false, true, false, Some("copy_prefix")) => "FileBrowser vim which_key copy_prefix",
            (false, true, false, Some("go_prefix")) => "FileBrowser vim which_key go_prefix",
            (false, true, false, _) => "FileBrowser vim",
            (false, false, _, _) => "FileBrowser",
        };
        div()
            .relative()
            .key_context(key_context)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_which_key_key_down))
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
            .on_action(cx.listener(Self::start_copy_prefix))
            .on_action(cx.listener(Self::start_go_prefix))
            .on_action(cx.listener(Self::copy_file_name_text))
            .on_action(cx.listener(Self::copy_file_path_text))
            .on_action(cx.listener(Self::copy_parent_directory_text))
            .on_action(cx.listener(Self::cancel_which_key_action))
            .on_action(cx.listener(Self::select_all_items))
            .on_action(cx.listener(Self::clear_selection))
            .on_action(cx.listener(Self::toggle_hidden_files))
            .size_full()
            .overflow_hidden()
            .bg(rgb(0x20222e))
            .child(sidebar)
            .child(content_frame)
            .children(which_key)
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

    fn item(
        name: &str,
        is_directory: bool,
        size: &str,
        byte_len: u64,
        selected: bool,
    ) -> BrowserItem {
        BrowserItem {
            path: PathBuf::from(name),
            name: name.to_owned(),
            detail: if is_directory { "Folder" } else { size }.to_owned(),
            modified: "—".to_owned(),
            size: size.to_owned(),
            byte_len,
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
        let folder = item("Photos", true, "—", 0, true);
        let file = item("notes.md", false, "1.5 KB", 1536, true);

        assert_eq!(
            selection_status(std::slice::from_ref(&folder), |_| Some(9)).as_deref(),
            Some("\"Photos\" selected (9 items)")
        );
        assert_eq!(
            selection_status(std::slice::from_ref(&file), |_| None).as_deref(),
            Some("\"notes.md\" selected (1.5 KB)")
        );
        assert_eq!(
            selection_status(&[folder, file], |_| Some(9)).as_deref(),
            Some(
                "1 folder selected (containing a total of 9 items), 1 other item selected (1.5 KB)"
            )
        );
    }

    #[test]
    fn status_keeps_folder_and_other_item_totals_separate() {
        let items = [
            item("Photos", true, "—", 0, true),
            item("Projects", true, "—", 0, true),
            item("one.apk", false, "53.8 MB", 56_400_000, true),
            item("two.apk", false, "53.8 MB", 56_400_000, true),
            item("three.apk", false, "53.8 MB", 56_400_000, true),
        ];

        assert_eq!(
            selection_status(&items, |item| match item.name.as_str() {
                "Photos" => Some(4),
                "Projects" => Some(5),
                _ => None,
            })
            .as_deref(),
            Some(
                "2 folders selected (containing a total of 9 items), 3 other items selected (161.4 MB)"
            )
        );
    }

    #[test]
    fn vim_trash_confirmation_status_uses_the_selected_item_count() {
        assert_eq!(vim_trash_confirmation_status(1), "Move 1 item to Trash?");
        assert_eq!(vim_trash_confirmation_status(3), "Move 3 items to Trash?");
    }
}
