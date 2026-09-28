pub(crate) mod components;
pub(crate) mod file_browser;
pub(crate) mod quick_look;
mod theme;

pub(crate) fn init(cx: &mut gpui::App) {
    uic::components::toast::set_appearance(theme::toast_appearance(), cx);
}
