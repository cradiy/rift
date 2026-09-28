mod tabs;

use std::{process::ExitCode, sync::Arc};

use gpui::{
    App, AppContext, Bounds, Entity, Global, InteractiveElement, ParentElement, Pixels, Render,
    SharedString, Styled, Window, WindowBounds, WindowHandle, WindowId, WindowOptions, div,
    prelude::FluentBuilder, px, size,
};
use gpui_platform::application;
use rift_core::ports::{FileSystem, NavigationSource};
use rift_fs::{LocalFileSystem, SystemNavigationSource};
use uic::assets::LucideAssets;
use uic::components::{context_menu, modal, toast};

use crate::{
    config::AppConfig,
    logging,
    presentation::{NavigationController, SharedFileClipboard},
};

use self::tabs::{BrowserTab, TabStripState};

const DEFAULT_WINDOW_WIDTH: f32 = 1280.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 820.0;

#[derive(Default)]
struct RiftWindowRegistry {
    windows: Vec<WindowHandle<RiftApp>>,
    next_tab_id: u64,
}

impl Global for RiftWindowRegistry {}

impl RiftWindowRegistry {
    fn allocate_tab_id(cx: &mut App) -> u64 {
        let registry = cx.global_mut::<Self>();
        registry.next_tab_id = registry.next_tab_id.wrapping_add(1);
        registry.next_tab_id
    }

    fn add_window(cx: &mut App, handle: WindowHandle<RiftApp>) {
        cx.global_mut::<Self>().windows.push(handle);
    }

    fn handles(cx: &App) -> Vec<WindowHandle<RiftApp>> {
        cx.global::<Self>().windows.clone()
    }
}

fn window_options(bounds: Bounds<Pixels>) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        app_id: Some("rift".to_owned()),
        ..Default::default()
    }
}

fn find_rift_window(id: WindowId, cx: &App) -> Option<WindowHandle<RiftApp>> {
    RiftWindowRegistry::handles(cx)
        .into_iter()
        .find(|handle| handle.window_id() == id)
}

fn open_detached_window(
    cx: &mut App,
    tab: BrowserTab,
    navigation: Entity<NavigationController>,
    file_system: Arc<dyn FileSystem>,
    clipboard: SharedFileClipboard,
    bounds: Bounds<Pixels>,
) -> WindowHandle<RiftApp> {
    let handle = cx
        .open_window(window_options(bounds), move |window, cx| {
            window.set_window_title("Rift");
            cx.new(move |cx| {
                RiftApp::from_transferred_tab(tab, navigation, file_system, clipboard, window, cx)
            })
        })
        .expect("failed to open detached Rift tab");
    RiftWindowRegistry::add_window(cx, handle);
    handle
}

pub fn run() -> ExitCode {
    let loaded_config = match rift_config::LoadedConfig::load_default() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("Unable to load Rift configuration: {error:#}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = logging::initialize(&loaded_config) {
        eprintln!("Unable to initialize Rift logging: {error:#}");
        return ExitCode::FAILURE;
    }
    log::info!("loaded configuration from {}", loaded_config.path.display());
    let navigation_source = match SystemNavigationSource::new() {
        Ok(source) => source,
        Err(error) => {
            log::error!("unable to initialize system navigation: {error}");
            eprintln!("Unable to initialize Rift system navigation: {error}");
            return ExitCode::FAILURE;
        }
    };

    application()
        .with_assets(LucideAssets::new())
        .run(move |cx: &mut App| {
            cx.set_global(AppConfig::new(loaded_config.clone()));
            uic::init(cx);
            crate::ui::init(cx);
            crate::ui::quick_look::init(cx);
            crate::ui::file_browser::init_key_bindings(cx);
            tabs::init_key_bindings(cx);
            cx.set_global(RiftWindowRegistry::default());
            let bounds = Bounds::centered(
                None,
                size(px(DEFAULT_WINDOW_WIDTH), px(DEFAULT_WINDOW_HEIGHT)),
                cx,
            );
            let navigation_source = Arc::new(navigation_source.clone());
            let initial_directory = navigation_source.initial_directory().to_path_buf();
            let handle = cx
                .open_window(window_options(bounds), move |window, cx| {
                    window.set_window_title("Rift");
                    cx.new(move |cx| {
                        RiftApp::new(initial_directory, navigation_source.clone(), window, cx)
                    })
                })
                .expect("failed to open Rift");
            RiftWindowRegistry::add_window(cx, handle);
            cx.activate(true);
        });
    ExitCode::SUCCESS
}

struct RiftApp {
    tabs: Vec<BrowserTab>,
    tab_strip: TabStripState,
    active_tab: usize,
    id: WindowId,
    navigation: Entity<NavigationController>,
    file_system: Arc<dyn FileSystem>,
    clipboard: SharedFileClipboard,
}

impl RiftApp {
    fn new(
        initial_directory: std::path::PathBuf,
        navigation_source: Arc<dyn NavigationSource>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let navigation = cx.new(|cx| NavigationController::new(navigation_source, cx));
        let mut app = Self {
            tabs: Vec::new(),
            tab_strip: TabStripState::default(),
            active_tab: 0,
            id: window.window_handle().window_id(),
            navigation,
            file_system: Arc::new(LocalFileSystem),
            clipboard: SharedFileClipboard::default(),
        };
        app.open_tab(initial_directory, window, cx);
        app
    }
}

impl Render for RiftApp {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let font_family = cx
            .global::<AppConfig>()
            .loaded()
            .config
            .fonts
            .family
            .clone();
        let browser = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.browser.clone());
        if let Some(browser) = &browser {
            window.set_window_title(&format!("{} — Rift", browser.read(cx).tab_title(cx)));
        } else {
            window.set_window_title("Rift");
        }
        let sidebar_visible = browser
            .as_ref()
            .is_some_and(|browser| browser.read(cx).sidebar_visible());
        let sidebar_is_animating = browser
            .as_ref()
            .is_some_and(|browser| browser.read(cx).sidebar_is_animating());
        let root = div()
            .key_context("RiftTabs")
            .on_action(cx.listener(Self::new_tab_action))
            .on_action(cx.listener(Self::close_tab_action))
            .on_action(cx.listener(Self::next_tab_action))
            .on_action(cx.listener(Self::previous_tab_action))
            .on_drag_move::<tabs::TabDrag>(cx.listener(Self::tab_drag_moved))
            .on_drop(cx.listener(Self::dropped_as_window))
            .size_full()
            .relative()
            .bg(gpui::rgba(0x20222dfc));
        let root = if let Some(font_family) = font_family {
            root.font_family(SharedString::from(font_family))
        } else {
            root
        };
        let root = root.when_some(browser, |root, browser| {
            root.child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .bottom_0()
                    .left_0()
                    .overflow_hidden()
                    .flex()
                    .child(browser),
            )
        });
        let root =
            if tabs::tabs_are_visible(self.tabs.len()) || self.tab_strip.native_payload.is_some() {
                root.child(self.render_tab_bar(sidebar_visible, sidebar_is_animating, window, cx))
            } else {
                root
            };
        root.child(toast::layer(cx))
            .child(context_menu::layer(cx))
            .child(modal::layer(cx))
    }
}
