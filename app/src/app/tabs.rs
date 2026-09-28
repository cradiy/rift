use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    Animation, AnimationExt as _, App, AppContext, Bounds, CursorStyle, DragEnd, DragMoveEvent,
    DragSourceWindowPolicy, Entity, Focusable, FontWeight, InteractiveElement, IntoElement,
    KeyBinding, MouseButton, ParentElement, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, SystemDragOptions, WeakEntity, Window, WindowId, actions,
    div, point, prelude::FluentBuilder, px, rgba, size, svg,
};
use rift_core::application::{BrowserMessage, BrowserState};
use rift_core::ports::FileSystem;
use uic::assets::LucideIcons;

use crate::{
    config::{AppConfig, BrowserPreferences},
    presentation::{BrowserController, NavigationController, SharedFileClipboard},
    ui::file_browser::{
        FileBrowser, FileDrag, SIDEBAR_ANIMATION_DURATION, SIDEBAR_WIDTH, TAB_BAR_HEIGHT,
        TOOLBAR_HEIGHT, drop_files_into, sidebar_animation_easing,
    },
};

use super::{
    DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, RiftApp, RiftWindowRegistry, find_rift_window,
    open_detached_window,
};

const TAB_HEIGHT: f32 = 32.0;
const TAB_GAP: f32 = 5.0;
const TAB_MIN_WIDTH: f32 = 136.0;
const TAB_BAR_PADDING: f32 = 8.0;
const NEW_TAB_BUTTON_WIDTH: f32 = 32.0;
const TAB_SPRING_OMEGA: f32 = 64.0;
const SOURCE_WINDOW_CLOSE_DELAY: Duration = Duration::from_millis(32);

actions!(rift_tabs, [NewTab, CloseTab, NextTab, PreviousTab]);

pub(super) fn init_key_bindings(cx: &mut App) {
    let context = Some("RiftTabs");
    cx.bind_keys([
        KeyBinding::new("ctrl-t", NewTab, context),
        KeyBinding::new("ctrl-w", CloseTab, context),
        KeyBinding::new("ctrl-tab", NextTab, context),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, context),
        KeyBinding::new("ctrl-pagedown", NextTab, context),
        KeyBinding::new("ctrl-pageup", PreviousTab, context),
    ]);

    let vim_context = Some("FileBrowser && vim && !editing && !trash_confirm && !which_key");
    cx.bind_keys([
        KeyBinding::new("shift-h", PreviousTab, vim_context),
        KeyBinding::new("shift-l", NextTab, vim_context),
    ]);
}

pub(super) struct BrowserTab {
    id: u64,
    pub(super) browser: Entity<FileBrowser>,
}

#[derive(Clone, Copy)]
struct SpringSlot {
    current: f32,
    velocity: f32,
    target: f32,
}

impl SpringSlot {
    fn settled(target: f32) -> Self {
        Self {
            current: target,
            velocity: 0.0,
            target,
        }
    }

    fn step(&mut self, dt: f32) -> bool {
        let displacement = self.current - self.target;
        let spring = self.velocity + TAB_SPRING_OMEGA * displacement;
        let decay = (-TAB_SPRING_OMEGA * dt).exp();
        self.current = self.target + (displacement + spring * dt) * decay;
        self.velocity = (self.velocity - TAB_SPRING_OMEGA * spring * dt) * decay;

        if (self.target - self.current).abs() < 0.18 && self.velocity.abs() < 20.0 {
            self.current = self.target;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }
}

#[derive(Clone)]
pub(super) struct TabDrag {
    tab_id: u64,
    title: SharedString,
    source: WindowId,
    source_entity: WeakEntity<RiftApp>,
    navigation: Entity<NavigationController>,
    file_system: Arc<dyn FileSystem>,
    clipboard: SharedFileClipboard,
    width: f32,
    preview: Rc<RefCell<Option<Entity<TabDragPreview>>>>,
    transaction: Rc<RefCell<TabDragTransaction>>,
}

struct TabDragTransaction {
    origin_index: usize,
    origin_active_id: u64,
    cursor_offset_x: f32,
    native: bool,
    owner: Option<WindowId>,
    detached_tab: Option<BrowserTab>,
}

pub(super) struct TabStripState {
    slots: BTreeMap<u64, SpringSlot>,
    drag: Option<TabDrag>,
    pub(super) native_payload: Option<TabDrag>,
    snap_index: Option<usize>,
    scroll: ScrollHandle,
    last_frame: Instant,
}

impl Default for TabStripState {
    fn default() -> Self {
        Self {
            slots: BTreeMap::new(),
            drag: None,
            native_payload: None,
            snap_index: None,
            scroll: ScrollHandle::new(),
            last_frame: Instant::now(),
        }
    }
}

struct TabDragPreview {
    title: SharedString,
    width: f32,
    detached: bool,
    cursor_offset_y: f32,
    y_offset: f32,
}

impl Render for TabDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div().w(px(self.width)).h(px(TAB_HEIGHT)).relative().child(
            div()
                .absolute()
                .top(px(self.y_offset))
                .w_full()
                .h_full()
                .rounded(px(9.))
                .border_1()
                .border_color(if self.detached {
                    rgba(0x8bdcff72)
                } else {
                    rgba(0x8bdcff52)
                })
                .bg(if self.detached {
                    rgba(0x252b39f7)
                } else {
                    rgba(0x2b3140f5)
                })
                .text_color(rgba(0xf5f3f8f2))
                .shadow(vec![
                    gpui::BoxShadow::new(px(0.), px(8.), rgba(0x00000066).into())
                        .blur_radius(px(20.))
                        .spread_radius(px(-7.)),
                    gpui::BoxShadow::new(px(0.), px(1.), rgba(0xffffff18).into())
                        .blur_radius(px(1.)),
                ])
                .child(
                    div()
                        .absolute()
                        .left(px(32.))
                        .right(px(32.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .min_w_0()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_center()
                                .child(self.title.clone()),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .right(px(5.))
                        .top(px(5.))
                        .size(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path(LucideIcons::X)
                                .size(px(13.))
                                .text_color(rgba(0xdad7dfb8)),
                        ),
                ),
        )
    }
}

#[derive(Clone, Copy)]
struct TabMetrics {
    bar_left: f32,
    inner_width: f32,
    tab_width: f32,
    content_width: f32,
}

impl RiftApp {
    pub(super) fn from_transferred_tab(
        tab: BrowserTab,
        navigation: Entity<NavigationController>,
        file_system: Arc<dyn FileSystem>,
        clipboard: SharedFileClipboard,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let app = Self {
            tabs: vec![tab],
            tab_strip: TabStripState::default(),
            active_tab: 0,
            id: window.window_handle().window_id(),
            navigation,
            file_system,
            clipboard,
        };
        app.sync_tab_bar_visibility(cx);
        app
    }

    pub(super) fn active_browser(&self) -> Entity<FileBrowser> {
        self.tabs[self.active_tab].browser.clone()
    }

    pub(super) fn open_tab(
        &mut self,
        directory: PathBuf,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let BrowserPreferences {
            view_mode,
            sort,
            show_hidden_files,
            sidebar_visible,
        } = cx.global::<AppConfig>().browser_preferences();
        let file_system = self.file_system.clone();
        let clipboard = self.clipboard.clone();
        let controller = cx.new(|cx| {
            let mut state = BrowserState::new(directory);
            state.update(BrowserMessage::SetViewMode(view_mode));
            state.update(BrowserMessage::SetSort(sort));
            state.update(BrowserMessage::SetShowHiddenFiles(show_hidden_files));
            let mut controller = BrowserController::with_clipboard(state, file_system, clipboard);
            controller.dispatch(BrowserMessage::Refresh, cx);
            controller
        });
        let navigation = self.navigation.clone();
        let browser =
            cx.new(|cx| FileBrowser::new(controller, navigation, sidebar_visible, window, cx));
        cx.observe(&browser, |_, _, cx| cx.notify()).detach();

        let id = RiftWindowRegistry::allocate_tab_id(cx);
        self.tabs.push(BrowserTab { id, browser });
        self.active_tab = self.tabs.len() - 1;
        self.sync_tab_bar_visibility(cx);
        cx.notify();
    }

    fn open_sibling_tab(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let directory = self.active_browser().read(cx).current_directory(cx);
        self.open_tab(directory, window, cx);
    }

    fn activate_tab(&mut self, id: u64, window: &mut Window, cx: &mut gpui::Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        if self.active_tab == index {
            return;
        }
        self.active_tab = index;
        self.focus_active_browser(window, cx);
        cx.notify();
    }

    fn close_tab(&mut self, id: u64, window: &mut Window, cx: &mut gpui::Context<Self>) {
        if self.tabs.len() == 1 {
            return;
        }
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let closing_active = index == self.active_tab;
        self.tabs.remove(index);
        self.tab_strip.slots.remove(&id);
        self.active_tab = active_index_after_close(self.active_tab, index, self.tabs.len());
        self.sync_tab_bar_visibility(cx);
        if closing_active {
            self.focus_active_browser(window, cx);
        }
        cx.notify();
    }

    fn focus_active_browser(&self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let focus = self.active_browser().read(cx).focus_handle(cx);
        focus.focus(window, cx);
    }

    fn sync_tab_bar_visibility(&self, cx: &mut gpui::Context<Self>) {
        let visible = tabs_are_visible(self.tabs.len());
        for tab in &self.tabs {
            tab.browser.update(cx, |browser, cx| {
                browser.set_tab_bar_visible(visible, cx);
            });
        }
    }

    fn tab_metrics(&self, sidebar_visible: bool, window: &Window) -> TabMetrics {
        let visual_tab_count = self.tabs.len()
            + usize::from(
                self.tab_strip.native_payload.is_some() && self.tab_strip.snap_index.is_some(),
            );
        self.tab_metrics_for(sidebar_visible, visual_tab_count, window)
    }

    fn tab_metrics_for(
        &self,
        sidebar_visible: bool,
        visual_tab_count: usize,
        window: &Window,
    ) -> TabMetrics {
        let bar_left = if sidebar_visible { SIDEBAR_WIDTH } else { 0.0 };
        let bar_width = (f32::from(window.viewport_size().width) - bar_left).max(1.0);
        let inner_width = (bar_width - TAB_BAR_PADDING * 2.0).max(1.0);
        let tab_width = tab_width_for(inner_width, visual_tab_count);
        let content_width = slot_left(visual_tab_count, tab_width) + NEW_TAB_BUTTON_WIDTH;
        TabMetrics {
            bar_left,
            inner_width,
            tab_width,
            content_width,
        }
    }

    fn animate_tab_slots(&mut self, tab_width: f32, window: &mut Window) {
        let now = Instant::now();
        let dt = now
            .saturating_duration_since(self.tab_strip.last_frame)
            .as_secs_f32()
            .min(1.0 / 20.0);
        self.tab_strip.last_frame = now;

        let mut moving = false;
        for (index, tab) in self.tabs.iter().enumerate() {
            let visual_index = if self
                .tab_strip
                .snap_index
                .is_some_and(|placeholder| index >= placeholder)
            {
                index + 1
            } else {
                index
            };
            let target = slot_left(visual_index, tab_width);
            let slot = self
                .tab_strip
                .slots
                .entry(tab.id)
                .or_insert_with(|| SpringSlot::settled(target));
            slot.target = target;
            moving |= slot.step(dt);
        }
        self.tab_strip
            .slots
            .retain(|id, _| self.tabs.iter().any(|tab| tab.id == *id));
        if moving {
            window.request_animation_frame();
        }
    }

    fn begin_tab_drag(
        &mut self,
        payload: TabDrag,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.tab_strip.drag = Some(payload.clone());
        self.activate_tab(payload.tab_id, window, cx);
        cx.notify();
    }

    fn detach_owned_tab(&mut self, payload: &TabDrag, cx: &mut gpui::Context<Self>) -> bool {
        let mut transaction = payload.transaction.borrow_mut();
        if transaction.owner != Some(self.id) || transaction.detached_tab.is_some() {
            return false;
        }
        let Some(index) = self.tabs.iter().position(|tab| tab.id == payload.tab_id) else {
            return false;
        };
        let tab = self.tabs.remove(index);
        self.tab_strip.slots.remove(&tab.id);
        self.active_tab = active_index_after_close(self.active_tab, index, self.tabs.len());
        transaction.owner = None;
        transaction.detached_tab = Some(tab);
        drop(transaction);
        self.sync_tab_bar_visibility(cx);
        cx.notify();
        true
    }

    fn reorder_dragged_tab(&mut self, tab_id: u64, new_index: usize) -> bool {
        let Some(old_index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return false;
        };
        if old_index == new_index {
            return false;
        }
        let active_id = self.tabs[self.active_tab].id;
        let tab = self.tabs.remove(old_index);
        self.tabs.insert(new_index.min(self.tabs.len()), tab);
        self.active_tab = self
            .tabs
            .iter()
            .position(|tab| tab.id == active_id)
            .unwrap_or(0);
        true
    }

    pub(super) fn tab_drag_moved(
        &mut self,
        event: &DragMoveEvent<TabDrag>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let payload = event.drag(cx).clone();
        let position = event.event.position;
        let sidebar_visible = self
            .tabs
            .get(self.active_tab)
            .is_some_and(|tab| tab.browser.read(cx).sidebar_visible());
        let x = f32::from(position.x);
        let y = f32::from(position.y);
        let bar_left = if sidebar_visible { SIDEBAR_WIDTH } else { 0.0 };
        let over_strip = x >= bar_left
            && x <= f32::from(window.viewport_size().width)
            && (TOOLBAR_HEIGHT..TOOLBAR_HEIGHT + TAB_BAR_HEIGHT).contains(&y);
        let native = payload.transaction.borrow().native;

        if let Some(preview) = payload.preview.borrow().clone() {
            preview.update(cx, |preview, cx| {
                let next_offset = if over_strip {
                    let preview_root_y = y - preview.cursor_offset_y;
                    TOOLBAR_HEIGHT + (TAB_BAR_HEIGHT - TAB_HEIGHT) * 0.5 - preview_root_y
                } else {
                    0.0
                };
                let detached = !over_strip;
                if preview.detached != detached || (preview.y_offset - next_offset).abs() > 0.01 {
                    preview.detached = detached;
                    preview.y_offset = next_offset;
                    cx.notify();
                }
            });
        }

        if !over_strip {
            let was_hovering = self
                .tab_strip
                .native_payload
                .as_ref()
                .is_some_and(|drag| drag.tab_id == payload.tab_id);
            if was_hovering {
                self.tab_strip.native_payload = None;
                self.tab_strip.snap_index = None;
                cx.notify();
            }

            let owner = payload.transaction.borrow().owner;
            if owner == Some(self.id) && payload.source == self.id && !native {
                let source_window = if self.tabs.len() == 1 {
                    DragSourceWindowPolicy::HideWhileNative
                } else {
                    DragSourceWindowPolicy::KeepVisible
                };
                match window.promote_active_drag_to_system_with_options(
                    SystemDragOptions {
                        source_window,
                        ..SystemDragOptions::default()
                    },
                    cx,
                ) {
                    Ok(_) => {
                        payload.transaction.borrow_mut().native = true;
                        self.detach_owned_tab(&payload, cx);
                    }
                    Err(error) => {
                        log::warn!("unable to detach tab into a system drag: {error:#}");
                    }
                }
            }
            return;
        }

        if native {
            self.tab_strip.native_payload = Some(payload.clone());
            let visual_count = self.tabs.len() + 1;
            let metrics = self.tab_metrics_for(sidebar_visible, visual_count, window);
            let content_x = x
                - metrics.bar_left
                - TAB_BAR_PADDING
                - f32::from(self.tab_strip.scroll.offset().x);
            let new_index =
                insertion_index(content_x, metrics.tab_width, visual_count).min(self.tabs.len());
            if self.tab_strip.snap_index != Some(new_index) {
                self.tab_strip.snap_index = Some(new_index);
                cx.notify();
            }
            return;
        }

        if payload.transaction.borrow().owner != Some(self.id) {
            return;
        }
        let metrics = self.tab_metrics(sidebar_visible, window);
        let content_x =
            x - metrics.bar_left - TAB_BAR_PADDING - f32::from(self.tab_strip.scroll.offset().x);
        let new_index = insertion_index(content_x, metrics.tab_width, self.tabs.len());
        if self.reorder_dragged_tab(payload.tab_id, new_index) {
            cx.notify();
        }
    }

    fn dropped_on_tab_bar(
        &mut self,
        payload: &TabDrag,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if payload.transaction.borrow().native {
            let index = self
                .tab_strip
                .snap_index
                .unwrap_or(self.tabs.len())
                .min(self.tabs.len());
            let Some(tab) = payload.transaction.borrow_mut().detached_tab.take() else {
                return;
            };
            self.tabs.insert(index, tab);
            self.active_tab = index;
            payload.transaction.borrow_mut().owner = Some(self.id);
            self.sync_tab_bar_visibility(cx);
            self.focus_active_browser(window, cx);
        }
        self.tab_strip.drag = None;
        self.tab_strip.native_payload = None;
        self.tab_strip.snap_index = None;
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn dropped_as_window(
        &mut self,
        payload: &TabDrag,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !payload.transaction.borrow().native {
            restore_local_order(payload, cx);
            return;
        }
        let Some(tab) = payload.transaction.borrow_mut().detached_tab.take() else {
            return;
        };
        let cursor_offset_x = payload.transaction.borrow().cursor_offset_x;
        let position = window.mouse_position();
        let window_origin = window.bounds().origin;
        let origin = point(
            window_origin.x + position.x - px(cursor_offset_x),
            window_origin.y + position.y - px(TOOLBAR_HEIGHT + TAB_HEIGHT * 0.5),
        );
        let bounds = Bounds {
            origin,
            size: size(px(DEFAULT_WINDOW_WIDTH), px(DEFAULT_WINDOW_HEIGHT)),
        };
        let detached = open_detached_window(
            cx,
            tab,
            payload.navigation.clone(),
            payload.file_system.clone(),
            payload.clipboard.clone(),
            bounds,
        );
        payload.transaction.borrow_mut().owner = Some(detached.window_id());
        self.tab_strip.native_payload = None;
        self.tab_strip.snap_index = None;
        schedule_close_window_if_empty(payload.source, cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn cycle_tab(&mut self, direction: isize, window: &mut Window, cx: &mut gpui::Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let len = self.tabs.len() as isize;
        self.active_tab = (self.active_tab as isize + direction).rem_euclid(len) as usize;
        self.focus_active_browser(window, cx);
        cx.notify();
    }

    pub(super) fn new_tab_action(
        &mut self,
        _: &NewTab,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.open_sibling_tab(window, cx);
    }

    pub(super) fn close_tab_action(
        &mut self,
        _: &CloseTab,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.tabs.is_empty() {
            return;
        }
        let id = self.tabs[self.active_tab].id;
        self.close_tab(id, window, cx);
    }

    pub(super) fn next_tab_action(
        &mut self,
        _: &NextTab,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.cycle_tab(1, window, cx);
    }

    pub(super) fn previous_tab_action(
        &mut self,
        _: &PreviousTab,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.cycle_tab(-1, window, cx);
    }

    pub(super) fn render_tab_bar(
        &mut self,
        sidebar_visible: bool,
        sidebar_is_animating: bool,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let metrics = self.tab_metrics(sidebar_visible, window);
        self.animate_tab_slots(metrics.tab_width, window);
        let tabs = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let id = tab.id;
                let active = index == self.active_tab;
                let title = SharedString::from(tab.browser.read(cx).tab_title(cx));
                let dragging = self
                    .tab_strip
                    .drag
                    .as_ref()
                    .is_some_and(|drag| drag.tab_id == id);
                let drop_browser = tab.browser.clone();
                let left = self
                    .tab_strip
                    .slots
                    .get(&id)
                    .map_or(slot_left(index, metrics.tab_width), |slot| slot.current);
                let payload = TabDrag {
                    tab_id: id,
                    title: title.clone(),
                    source: self.id,
                    source_entity: cx.entity().downgrade(),
                    navigation: self.navigation.clone(),
                    file_system: self.file_system.clone(),
                    clipboard: self.clipboard.clone(),
                    width: metrics.tab_width,
                    preview: Rc::new(RefCell::new(None)),
                    transaction: Rc::new(RefCell::new(TabDragTransaction {
                        origin_index: index,
                        origin_active_id: self.tabs[self.active_tab].id,
                        cursor_offset_x: 0.0,
                        native: false,
                        owner: Some(self.id),
                        detached_tab: None,
                    })),
                };
                div()
                    .id(("browser-tab", id))
                    .debug_selector(move || format!("browser-tab-{id}"))
                    .absolute()
                    .left(px(left))
                    .top(px((TAB_BAR_HEIGHT - TAB_HEIGHT) * 0.5))
                    .w(px(metrics.tab_width))
                    .h(px(TAB_HEIGHT))
                    .rounded(px(9.))
                    .border_1()
                    .opacity(if dragging { 0.0 } else { 1.0 })
                    .cursor(if dragging {
                        CursorStyle::ClosedHand
                    } else {
                        CursorStyle::OpenHand
                    })
                    .when(active, |tab| {
                        tab.bg(rgba(0x2b3140e8))
                            .border_color(rgba(0x8bdcff38))
                            .text_color(rgba(0xf5f3f8f2))
                            .shadow(vec![
                                gpui::BoxShadow::new(px(0.), px(5.), rgba(0x00000040).into())
                                    .blur_radius(px(14.))
                                    .spread_radius(px(-6.)),
                                gpui::BoxShadow::new(px(0.), px(1.), rgba(0xffffff13).into())
                                    .blur_radius(px(1.)),
                            ])
                    })
                    .when(!active, |tab| {
                        tab.border_color(rgba(0xffffff00))
                            .text_color(rgba(0xd1cfd8ad))
                            .hover(|style| {
                                style.bg(rgba(0xffffff09)).border_color(rgba(0xffffff0e))
                            })
                    })
                    .drag_over::<FileDrag>(|style, _, _, _| {
                        style.bg(rgba(0x36aee82e)).border_color(rgba(0x68cff68f))
                    })
                    .on_drop(move |drag: &FileDrag, window, cx| {
                        cx.stop_propagation();
                        let (directory, controller) = {
                            let browser = drop_browser.read(cx);
                            (browser.current_directory(cx), browser.controller_entity())
                        };
                        drop_files_into(drag, directory, controller, window, cx);
                    })
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _, window, cx| {
                            this.close_tab(id, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.activate_tab(id, window, cx);
                    }))
                    .on_drag(payload, |drag, cursor_offset, window, cx| {
                        drag.transaction.borrow_mut().cursor_offset_x = f32::from(cursor_offset.x);
                        let _ = drag.source_entity.update(cx, |app, cx| {
                            app.begin_tab_drag(drag.clone(), window, cx);
                        });
                        let preview = cx.new(|_| TabDragPreview {
                            title: drag.title.clone(),
                            width: drag.width,
                            detached: false,
                            cursor_offset_y: f32::from(cursor_offset.y),
                            y_offset: 0.0,
                        });
                        *drag.preview.borrow_mut() = Some(preview.clone());
                        preview
                    })
                    .on_drag_end::<TabDrag>(|event, drag, _, cx| {
                        let event = *event;
                        let drag = drag.clone();
                        cx.defer(move |cx| finish_tab_drag(event, drag, cx));
                    })
                    .child(
                        div()
                            .absolute()
                            .left(px(32.))
                            .right(px(32.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .min_w_0()
                            .text_size(px(12.))
                            .font_weight(if active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .child(div().min_w_0().truncate().text_center().child(title)),
                    )
                    .when(active, |tab| {
                        tab.child(
                            div()
                                .id(("close-browser-tab", id))
                                .absolute()
                                .right(px(5.))
                                .top(px(5.))
                                .size(px(22.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(6.))
                                .hover(|style| style.bg(rgba(0xffffff16)))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.close_tab(id, window, cx);
                                    cx.stop_propagation();
                                }))
                                .child(
                                    svg()
                                        .path(LucideIcons::X)
                                        .size(px(13.))
                                        .text_color(rgba(0xdad7dfb8)),
                                ),
                        )
                    })
            })
            .collect::<Vec<_>>();
        let snap_preview = self
            .tab_strip
            .native_payload
            .as_ref()
            .zip(self.tab_strip.snap_index)
            .map(|(payload, index)| {
                div()
                    .absolute()
                    .left(px(slot_left(index, metrics.tab_width)))
                    .top(px((TAB_BAR_HEIGHT - TAB_HEIGHT) * 0.5))
                    .w(px(metrics.tab_width))
                    .h(px(TAB_HEIGHT))
                    .rounded(px(9.))
                    .border_1()
                    .border_color(rgba(0x8bdcff5c))
                    .bg(rgba(0x45bff01a))
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(32.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgba(0xeaf7ffdb))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_center()
                            .child(payload.title.clone()),
                    )
            });
        let visual_tab_count = self.tabs.len() + usize::from(snap_preview.is_some());
        let new_tab_button = div()
            .id("new-browser-tab")
            .absolute()
            .left(px(slot_left(visual_tab_count, metrics.tab_width)))
            .top(px((TAB_BAR_HEIGHT - NEW_TAB_BUTTON_WIDTH) * 0.5))
            .size(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(9.))
            .border_1()
            .border_color(rgba(0xffffff00))
            .cursor_pointer()
            .hover(|style| style.bg(rgba(0xffffff0b)).border_color(rgba(0xffffff10)))
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_sibling_tab(window, cx);
            }))
            .child(
                svg()
                    .path(LucideIcons::Plus)
                    .size(px(16.))
                    .text_color(rgba(0xd8d5dfb8)),
            );

        let tab_bar = div()
            .absolute()
            .top(px(TOOLBAR_HEIGHT))
            .right_0()
            .h(px(TAB_BAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .px(px(8.))
            .border_b_1()
            .border_color(rgba(0xffffff0d))
            .bg(rgba(0x1b1d28f7))
            .on_drop(cx.listener(Self::dropped_on_tab_bar))
            .child(
                div()
                    .id("browser-tabs-scroll")
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .scrollbar_width(px(0.))
                    .track_scroll(&self.tab_strip.scroll)
                    .child(
                        div()
                            .relative()
                            .h_full()
                            .w(px(metrics.content_width.max(metrics.inner_width)))
                            .children(tabs)
                            .children(snap_preview)
                            .child(new_tab_button),
                    ),
            );

        if sidebar_is_animating {
            tab_bar
                .with_animation(
                    if sidebar_visible {
                        "tab-bar-expand-sidebar"
                    } else {
                        "tab-bar-collapse-sidebar"
                    },
                    Animation::new(SIDEBAR_ANIMATION_DURATION)
                        .with_easing(sidebar_animation_easing),
                    move |tab_bar, phase| {
                        let progress = if sidebar_visible { phase } else { 1.0 - phase };
                        tab_bar.left(px(SIDEBAR_WIDTH * progress))
                    },
                )
                .into_any_element()
        } else {
            tab_bar
                .left(px(if sidebar_visible { SIDEBAR_WIDTH } else { 0.0 }))
                .into_any_element()
        }
    }
}

fn clear_drag_indicators(tab_id: u64, cx: &mut App) {
    for handle in RiftWindowRegistry::handles(cx) {
        let _ = handle.update(cx, |app, _, cx| {
            let mut changed = false;
            if app
                .tab_strip
                .drag
                .as_ref()
                .is_some_and(|drag| drag.tab_id == tab_id)
            {
                app.tab_strip.drag = None;
                changed = true;
            }
            if app
                .tab_strip
                .native_payload
                .as_ref()
                .is_some_and(|drag| drag.tab_id == tab_id)
            {
                app.tab_strip.native_payload = None;
                app.tab_strip.snap_index = None;
                changed = true;
            }
            if changed {
                cx.notify();
            }
        });
    }
}

fn restore_local_order(payload: &TabDrag, cx: &mut App) -> bool {
    let Some(source) = find_rift_window(payload.source, cx) else {
        return false;
    };
    let (origin_index, origin_active_id) = {
        let transaction = payload.transaction.borrow();
        (transaction.origin_index, transaction.origin_active_id)
    };
    source
        .update(cx, |app, _, cx| {
            app.reorder_dragged_tab(payload.tab_id, origin_index);
            if let Some(active) = app.tabs.iter().position(|tab| tab.id == origin_active_id) {
                app.active_tab = active;
            }
            app.tab_strip.drag = None;
            cx.notify();
        })
        .is_ok()
}

fn restore_tab_to_source(payload: &TabDrag, cx: &mut App) -> bool {
    if payload.transaction.borrow().detached_tab.is_none() {
        return restore_local_order(payload, cx);
    }
    let Some(source) = find_rift_window(payload.source, cx) else {
        return false;
    };
    let (origin_index, origin_active_id, tab) = {
        let mut transaction = payload.transaction.borrow_mut();
        let Some(tab) = transaction.detached_tab.take() else {
            return false;
        };
        (transaction.origin_index, transaction.origin_active_id, tab)
    };
    let restored = source
        .update(cx, |app, window, cx| {
            let index = origin_index.min(app.tabs.len());
            app.tabs.insert(index, tab);
            app.active_tab = app
                .tabs
                .iter()
                .position(|tab| tab.id == origin_active_id)
                .unwrap_or(index);
            app.tab_strip.drag = None;
            app.sync_tab_bar_visibility(cx);
            app.focus_active_browser(window, cx);
            cx.notify();
        })
        .is_ok();
    if restored {
        let mut transaction = payload.transaction.borrow_mut();
        transaction.owner = Some(payload.source);
        transaction.native = false;
    }
    restored
}

fn open_detached_tab(payload: &TabDrag, cx: &mut App) -> bool {
    let Some(tab) = payload.transaction.borrow_mut().detached_tab.take() else {
        return false;
    };
    let bounds = Bounds::centered(
        None,
        size(px(DEFAULT_WINDOW_WIDTH), px(DEFAULT_WINDOW_HEIGHT)),
        cx,
    );
    let detached = open_detached_window(
        cx,
        tab,
        payload.navigation.clone(),
        payload.file_system.clone(),
        payload.clipboard.clone(),
        bounds,
    );
    payload.transaction.borrow_mut().owner = Some(detached.window_id());
    schedule_close_window_if_empty(payload.source, cx);
    true
}

fn close_window_if_empty(id: WindowId, cx: &mut App) {
    let Some(handle) = find_rift_window(id, cx) else {
        return;
    };
    let _ = handle.update(cx, |app, window, _| {
        if app.tabs.is_empty() {
            window.remove_window();
        }
    });
}

fn schedule_close_window_if_empty(id: WindowId, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(SOURCE_WINDOW_CLOSE_DELAY)
            .await;
        cx.update(|cx| close_window_if_empty(id, cx));
    })
    .detach();
}

fn finish_tab_drag(event: DragEnd, payload: TabDrag, cx: &mut App) {
    match event {
        DragEnd::Dropped { .. } => {
            if payload.transaction.borrow().owner.is_none() {
                open_detached_tab(&payload, cx);
            } else if payload.transaction.borrow().native {
                schedule_close_window_if_empty(payload.source, cx);
            }
        }
        DragEnd::Unaccepted => {
            if payload.transaction.borrow().native {
                open_detached_tab(&payload, cx);
            } else {
                restore_local_order(&payload, cx);
            }
        }
        DragEnd::Cancelled => {
            if payload.transaction.borrow().native {
                open_detached_tab(&payload, cx);
            } else {
                restore_tab_to_source(&payload, cx);
            }
        }
    }
    clear_drag_indicators(payload.tab_id, cx);
}

pub(super) fn tabs_are_visible(tab_count: usize) -> bool {
    tab_count > 1
}

fn tab_width_for(inner_width: f32, tab_count: usize) -> f32 {
    let tab_count = tab_count.max(1);
    ((inner_width - NEW_TAB_BUTTON_WIDTH - TAB_GAP * tab_count as f32) / tab_count as f32)
        .max(TAB_MIN_WIDTH)
}

fn slot_left(index: usize, tab_width: f32) -> f32 {
    index as f32 * (tab_width + TAB_GAP)
}

fn insertion_index(content_x: f32, tab_width: f32, tab_count: usize) -> usize {
    let index =
        ((content_x + (tab_width + TAB_GAP) * 0.5) / (tab_width + TAB_GAP)).floor() as isize;
    index.clamp(0, tab_count.saturating_sub(1) as isize) as usize
}

fn active_index_after_close(active: usize, closed: usize, remaining_len: usize) -> usize {
    if closed < active {
        active - 1
    } else if closed == active {
        active.min(remaining_len.saturating_sub(1))
    } else {
        active
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use gpui::{MouseButton, TestAppContext, VisualTestContext, point, px, size};
    use rift_config::{LoadedConfig, RiftConfig};
    use rift_core::{domain::NavigationSnapshot, ports::NavigationError};

    use super::{
        NEW_TAB_BUTTON_WIDTH, SpringSlot, TAB_GAP, active_index_after_close, insertion_index,
        tab_width_for, tabs_are_visible,
    };

    struct EmptyNavigation;

    impl rift_core::ports::NavigationSource for EmptyNavigation {
        fn load(&self) -> Result<NavigationSnapshot, NavigationError> {
            Ok(NavigationSnapshot::default())
        }
    }

    #[test]
    fn tab_bar_appears_only_when_multiple_tabs_exist() {
        assert!(!tabs_are_visible(0));
        assert!(!tabs_are_visible(1));
        assert!(tabs_are_visible(2));
    }

    #[test]
    fn closing_a_tab_keeps_the_same_neighbor_active() {
        assert_eq!(active_index_after_close(1, 1, 2), 1);
        assert_eq!(active_index_after_close(2, 2, 2), 1);
        assert_eq!(active_index_after_close(2, 0, 2), 1);
        assert_eq!(active_index_after_close(0, 2, 2), 0);
    }

    #[test]
    fn tabs_share_all_space_left_by_the_new_tab_button() {
        let inner_width = 700.0;
        let width = tab_width_for(inner_width, 2);
        assert_eq!(
            width * 2.0 + TAB_GAP * 2.0 + NEW_TAB_BUTTON_WIDTH,
            inner_width
        );
    }

    #[test]
    fn insertion_index_changes_at_tab_centers() {
        let width = 180.0;
        assert_eq!(insertion_index(0.0, width, 3), 0);
        assert_eq!(insertion_index(width + TAB_GAP, width, 3), 1);
        assert_eq!(insertion_index(10_000.0, width, 3), 2);
    }

    #[test]
    fn reorder_spring_converges_without_overshoot_tail() {
        let mut spring = SpringSlot::settled(0.0);
        spring.target = 200.0;
        for _ in 0..240 {
            spring.step(1.0 / 120.0);
        }
        assert!((spring.current - 200.0).abs() < 0.1);
    }

    #[gpui::test]
    fn vim_uppercase_h_and_l_switch_tabs(cx: &mut TestAppContext) {
        cx.update(uic::init);
        cx.update(crate::ui::init);
        cx.update(crate::ui::quick_look::init);
        cx.update(super::init_key_bindings);
        let directory = tempfile::tempdir().unwrap();
        let mut config = RiftConfig::default();
        config.browser.vim_mode = true;
        cx.set_global(crate::config::AppConfig::new(LoadedConfig {
            path: directory.path().join("config.toml"),
            config,
        }));
        cx.set_global(super::RiftWindowRegistry::default());
        let path = directory.path().to_path_buf();
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            super::RiftApp::new(path, Arc::new(EmptyNavigation), window, cx)
        });
        cx.update(|cx| super::RiftWindowRegistry::add_window(cx, handle));
        handle
            .update(cx, |app, window, cx| {
                let directory = app.active_browser().read(cx).current_directory(cx);
                app.open_tab(directory.clone(), window, cx);
                app.open_tab(directory, window, cx);
            })
            .unwrap();
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, cx| window.draw(cx).clear());
        visual.simulate_keystrokes("shift-h");
        handle
            .update(&mut visual.cx, |app, _, _| assert_eq!(app.active_tab, 1))
            .unwrap();
        visual.simulate_keystrokes("shift-l");
        handle
            .update(&mut visual.cx, |app, _, _| assert_eq!(app.active_tab, 2))
            .unwrap();
    }

    #[gpui::test]
    fn dragging_reorders_live_cancel_restores_and_drop_commits(cx: &mut TestAppContext) {
        cx.update(uic::init);
        cx.update(crate::ui::init);
        cx.update(crate::ui::quick_look::init);
        let directory = tempfile::tempdir().unwrap();
        cx.set_global(crate::config::AppConfig::new(LoadedConfig {
            path: directory.path().join("config.toml"),
            config: RiftConfig::default(),
        }));
        cx.set_global(super::RiftWindowRegistry::default());
        let path = directory.path().to_path_buf();
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            super::RiftApp::new(path, Arc::new(EmptyNavigation), window, cx)
        });
        cx.update(|cx| super::RiftWindowRegistry::add_window(cx, handle));
        handle
            .update(cx, |app, window, cx| {
                let directory = app.active_browser().read(cx).current_directory(cx);
                app.open_tab(directory.clone(), window, cx);
                app.open_tab(directory, window, cx);
            })
            .unwrap();
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, cx| window.draw(cx).clear());
        let first = visual.debug_bounds("browser-tab-1").unwrap().center();
        let third = visual.debug_bounds("browser-tab-3").unwrap().center();
        visual.simulate_mouse_down(first, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            first + point(px(10.), px(0.)),
            Some(MouseButton::Left),
            Default::default(),
        );
        visual.simulate_mouse_move(third, Some(MouseButton::Left), Default::default());

        handle
            .update(&mut visual.cx, |app, _, _| {
                assert_eq!(
                    app.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
                    [2, 3, 1]
                );
                assert!(app.tab_strip.drag.is_some());
            })
            .unwrap();
        visual.update(|window, cx| {
            assert!(cx.stop_active_drag(window));
        });
        visual.cx.run_until_parked();
        handle
            .update(&mut visual.cx, |app, _, _| {
                assert_eq!(
                    app.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
                    [1, 2, 3]
                );
                assert!(app.tab_strip.drag.is_none());
            })
            .unwrap();

        visual
            .cx
            .executor()
            .advance_clock(Duration::from_millis(200));
        visual.cx.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear());
        let first = visual.debug_bounds("browser-tab-1").unwrap().center();
        let third = visual.debug_bounds("browser-tab-3").unwrap().center();
        visual.simulate_mouse_down(first, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            first + point(px(10.), px(0.)),
            Some(MouseButton::Left),
            Default::default(),
        );
        visual.simulate_mouse_move(third, Some(MouseButton::Left), Default::default());
        visual.simulate_mouse_up(third, MouseButton::Left, Default::default());
        visual.cx.run_until_parked();
        handle
            .update(&mut visual.cx, |app, _, _| {
                assert_eq!(
                    app.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
                    [2, 3, 1]
                );
                assert!(app.tab_strip.drag.is_none());
            })
            .unwrap();
    }
}
