use gpui::Global;
use rift_config::LoadedConfig;

#[derive(Clone)]
pub(crate) struct AppConfig(pub(crate) LoadedConfig);

impl Global for AppConfig {}
