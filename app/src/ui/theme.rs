use gpui::{Styled, point, px, rgba};
use gpui_effects::{FrostedGlassAppearance, LiquidGlassAppearance};
use uic::components::{
    input::InputAppearance,
    modal::{Modal, ModalAppearance},
    toast::{ToastAppearance, ToastColors},
};

pub(crate) fn toast_appearance() -> ToastAppearance {
    ToastAppearance::default()
        .colors(ToastColors {
            info: rgba(0x62c8ffff).into(),
            success: rgba(0x58d68dff).into(),
            warn: rgba(0xffc857ff).into(),
            error: rgba(0xff6b73ff).into(),
            loading: rgba(0x62c8ffff).into(),
        })
        .gap(px(8.))
        .viewport_margin(px(20.))
        .max_w(px(440.))
        .px(px(14.))
        .py(px(10.))
        .rounded(px(12.))
        .border_1()
        .border_color(rgba(0xffffff24))
        .bg(rgba(0x191c28f5))
        .text_color(rgba(0xf5f3f8f2))
        .shadow(vec![
            gpui::BoxShadow::new(px(0.), px(12.), rgba(0x00000070).into())
                .blur_radius(px(32.))
                .spread_radius(px(-8.)),
            gpui::BoxShadow::new(px(0.), px(1.), rgba(0xffffff14).into()).blur_radius(px(1.)),
        ])
}

pub(crate) fn context_menu_glass() -> FrostedGlassAppearance {
    FrostedGlassAppearance {
        blur_radius: px(12.),
        saturation: 1.35,
        brightness: 0.9,
        tint: rgba(0x10162470).into(),
        edge: rgba(0xc6d6f06b).into(),
        edge_width: px(1.),
        sheen: 0.025,
        ..FrostedGlassAppearance::dark()
    }
}

pub(crate) fn info_glass() -> FrostedGlassAppearance {
    FrostedGlassAppearance {
        blur_radius: px(18.),
        saturation: 1.3,
        brightness: 0.88,
        tint: rgba(0x151a2a9c).into(),
        edge: rgba(0xbfd5f05c).into(),
        edge_width: px(1.),
        sheen: 0.024,
        ..FrostedGlassAppearance::dark()
    }
}

pub(crate) fn which_key_glass() -> FrostedGlassAppearance {
    FrostedGlassAppearance {
        blur_radius: px(16.),
        saturation: 1.32,
        brightness: 0.9,
        tint: rgba(0x111827b0).into(),
        edge: rgba(0x9edcfa52).into(),
        edge_width: px(1.),
        sheen: 0.026,
        ..FrostedGlassAppearance::dark()
    }
}

pub(crate) fn transfer_glass() -> FrostedGlassAppearance {
    FrostedGlassAppearance {
        blur_radius: px(18.),
        saturation: 1.25,
        brightness: 0.86,
        tint: rgba(0x131d30c7).into(),
        edge: rgba(0xb0d9fa42).into(),
        edge_width: px(1.),
        sheen: 0.025,
        ..FrostedGlassAppearance::dark()
    }
}

pub(crate) fn sidebar_glass() -> FrostedGlassAppearance {
    FrostedGlassAppearance {
        blur_radius: px(15.),
        saturation: 1.25,
        brightness: 0.88,
        tint: rgba(0x17102f78).into(),
        edge: rgba(0x00000000).into(),
        edge_width: px(0.),
        sheen: 0.018,
        ..FrostedGlassAppearance::dark()
    }
}

pub(crate) fn quick_look_glass() -> LiquidGlassAppearance {
    LiquidGlassAppearance::dark()
        .blur_radius(px(16.))
        .clarity(0.18)
        .refraction(px(17.))
        .thickness(px(21.))
        .dispersion(0.008)
        .tint(rgba(0x12182778).into())
        .saturation(1.12)
        .brightness(0.92)
        .highlight(0.42)
        .edge_shadow(0.13)
        .rim_width(px(0.9))
        .edge_tint_strength(0.46)
        .edge_tint_width(px(1.2))
        .edge_sample_distance(px(12.))
        .edge_tint_lift(0.3)
        .light_direction(point(-0.65, -0.8))
}

pub(crate) fn quick_look_control_glass() -> LiquidGlassAppearance {
    LiquidGlassAppearance::clear()
        .blur_radius(px(7.))
        .clarity(0.45)
        .refraction(px(7.))
        .thickness(px(8.))
        .dispersion(0.004)
        .tint(rgba(0x1d273d52).into())
        .highlight(0.38)
        .edge_shadow(0.08)
        .rim_width(px(0.7))
        .edge_tint_strength(0.34)
        .edge_tint_width(px(0.8))
        .edge_sample_distance(px(6.))
}

pub(crate) fn input_appearance() -> InputAppearance {
    InputAppearance::default()
        .placeholder(rgba(0xbfc2cc70).into())
        .focus_border(rgba(0x0a84ffd6).into())
        .caret(rgba(0x61c5f4ff).into())
        .selection(rgba(0x0a84ff4f).into())
}

pub(crate) fn modal_appearance() -> ModalAppearance {
    let appearance = ModalAppearance::default();
    appearance
        .backdrop(rgba(0x05071082).into())
        .section_border(rgba(0xffffff14).into())
        .ok_button(
            appearance
                .ok_button
                .background(rgba(0x0a84ffff).into())
                .border(rgba(0x54b1ffff).into())
                .hover_background(rgba(0x2695ffff).into())
                .radius(px(8.))
                .height(px(32.)),
        )
        .cancel_button(
            appearance
                .cancel_button
                .background(rgba(0x323540f2).into())
                .foreground(rgba(0xf4f3f7ed).into())
                .border(rgba(0xffffff20).into())
                .hover_background(rgba(0x424651f2).into())
                .radius(px(8.))
                .height(px(32.)),
        )
}

pub(crate) fn style_modal(modal: Modal) -> Modal {
    style_modal_surface(modal.appearance(modal_appearance()))
}

pub(crate) fn style_quick_look_modal(modal: Modal) -> Modal {
    let appearance = modal_appearance()
        .backdrop(rgba(0x05081270).into())
        .section_borders(false)
        .body_padding_x(px(0.))
        .body_padding_y(px(0.));
    modal
        .appearance(appearance)
        .rounded(px(20.))
        .border(px(0.))
        .bg(rgba(0x00000000))
        .shadow(Vec::new())
}

fn style_modal_surface(modal: Modal) -> Modal {
    modal
        .rounded(px(14.))
        .border_1()
        .border_color(rgba(0xffffff24))
        .bg(rgba(0x232631fa))
        .text_color(rgba(0xf4f3f7ed))
        .shadow(vec![
            gpui::BoxShadow::new(px(0.), px(20.), rgba(0x0000008a).into())
                .blur_radius(px(52.))
                .spread_radius(px(-10.)),
        ])
}
