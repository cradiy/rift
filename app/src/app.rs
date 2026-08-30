use std::{process::ExitCode, sync::Arc};

use gpui::{
    App, AppContext, Bounds, Entity, ParentElement, Render, SharedString, Styled, Window,
    WindowBounds, WindowOptions, div, px, size,
};
use gpui_platform::application;
use rift_core::application::{BrowserMessage, BrowserState};
use rift_core::ports::NavigationSource;
use rift_fs::{LocalFileSystem, SystemNavigationSource};
use uic::assets::LucideAssets;
use uic::components::{context_menu, modal, toast};

use crate::{
    config::AppConfig,
    logging,
    presentation::{BrowserController, NavigationController},
    ui::file_browser::FileBrowser,
};

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
            cx.set_global(AppConfig(loaded_config.clone()));
            uic::init(cx);
            crate::ui::quick_look::init(cx);
            crate::ui::file_browser::init_key_bindings(cx);
            let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
            let navigation_source = Arc::new(navigation_source.clone());
            let initial_directory = navigation_source.initial_directory().to_path_buf();
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: None,
                    app_id: Some("rift".to_owned()),
                    ..Default::default()
                },
                move |window, cx| {
                    window.set_window_title("Rift");
                    cx.new(move |cx| {
                        RiftApp::new(initial_directory, navigation_source.clone(), window, cx)
                    })
                },
            )
            .expect("failed to open Rift");
            cx.activate(true);
        });
    ExitCode::SUCCESS
}

struct RiftApp {
    browser: Entity<FileBrowser>,
}

impl RiftApp {
    fn new(
        initial_directory: std::path::PathBuf,
        navigation_source: Arc<dyn NavigationSource>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let controller = cx.new(|cx| {
            let mut controller = BrowserController::new(
                BrowserState::new(initial_directory),
                Arc::new(LocalFileSystem),
            );
            controller.dispatch(BrowserMessage::Refresh, cx);
            controller
        });
        let navigation = cx.new(|cx| NavigationController::new(navigation_source, cx));
        let browser = cx.new(|cx| FileBrowser::new(controller, navigation, window, cx));
        Self { browser }
    }
}

impl Render for RiftApp {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let font_family = cx.global::<AppConfig>().0.config.fonts.family.clone();
        let root = div().size_full();
        let root = if let Some(font_family) = font_family {
            root.font_family(SharedString::from(font_family))
        } else {
            root
        };
        root.child(self.browser.clone())
            .child(toast::layer(cx))
            .child(context_menu::layer(cx))
            .child(modal::layer(cx))
    }
}
