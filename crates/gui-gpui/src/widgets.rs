//! Pequenos componentes visuais do dockerview.

use dockerview_core::ContainerState;
use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Div, FontWeight, Hsla, IntoElement, SharedString, div, px, relative};

use crate::theme::{MONO_FONT, Palette};

pub fn icon(name: IconName, color: Hsla) -> Icon {
    Icon::new(name).text_color(color)
}

/// Pílula de estado: ponto + texto, com fundo translúcido da cor do estado.
pub fn state_pill(state: ContainerState, p: &Palette) -> Div {
    let color = p.state(state);
    let label = match state {
        ContainerState::Running => "Rodando",
        ContainerState::Paused => "Pausado",
        ContainerState::Restarting => "Reiniciando",
        ContainerState::Created => "Criado",
        ContainerState::Exited => "Parado",
        ContainerState::Removing => "Removendo",
        ContainerState::Dead => "Morto",
        ContainerState::Unknown => "Desconhecido",
    };
    h_flex()
        .gap_1p5()
        .px_2()
        .py_0p5()
        .rounded_full()
        .bg(color.alpha(0.14))
        .text_color(color)
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .child(div().size(px(6.)).rounded_full().bg(color))
        .child(label)
}

/// Ponto de estado com "halo" para containers ativos.
pub fn state_dot(state: ContainerState, p: &Palette) -> Div {
    let color = p.state(state);
    div()
        .size(px(14.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(state.is_running(), |d| d.bg(color.alpha(0.18)))
        .child(div().size(px(7.)).rounded_full().bg(color))
}

/// Barra de progresso fina (0.0..=1.0).
pub fn meter(fraction: f32, color: Hsla, p: &Palette) -> Div {
    let f = fraction.clamp(0.0, 1.0);
    div()
        .h(px(4.))
        .w_full()
        .rounded_full()
        .bg(p.border)
        .child(div().h_full().w(relative(f.max(0.015))).rounded_full().bg(color))
}

/// Cartão de métrica do painel: ícone colorido, rótulo, valor grande e detalhe.
pub fn stat_card(
    icon_name: IconName,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    color: Hsla,
    p: &Palette,
) -> Div {
    v_flex()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .gap_2()
        .p_4()
        .rounded_xl()
        .bg(p.surface)
        .border_1()
        .border_color(p.border)
        .child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_lg()
                        .bg(color.alpha(0.14))
                        .child(icon(icon_name, color).size_4()),
                )
                .child(div().min_w_0().text_sm().text_color(p.text_muted).truncate().child(label.into())),
        )
        .child(
            div()
                .text_2xl()
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.text)
                .child(value.into()),
        )
        .child(div().text_xs().text_color(p.text_faint).truncate().child(detail.into()))
}

/// Chip pequeno monoespaçado (portas, IDs).
pub fn chip(text: impl Into<SharedString>, p: &Palette) -> Div {
    div()
        .px_1p5()
        .py_0p5()
        .rounded_md()
        .bg(p.surface_hover)
        .border_1()
        .border_color(p.border)
        .text_xs()
        .font_family(MONO_FONT)
        .text_color(p.text_muted)
        .child(text.into())
}

/// Linha "rótulo: valor" usada no painel de informações.
pub fn info_row(label: &str, value: impl IntoElement, p: &Palette) -> Div {
    h_flex()
        .items_start()
        .gap_3()
        .py_1p5()
        .border_b_1()
        .border_color(p.border)
        .child(div().w(px(90.)).flex_shrink_0().text_xs().text_color(p.text_faint).child(label.to_string()))
        .child(div().flex_1().min_w_0().text_sm().text_color(p.text).child(value))
}

pub fn empty_state(icon_name: IconName, title: &str, subtitle: &str, p: &Palette) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .child(
            div()
                .size(px(56.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_2xl()
                .bg(p.accent_soft)
                .child(icon(icon_name, p.accent).size_6()),
        )
        .child(div().text_base().font_weight(FontWeight::MEDIUM).text_color(p.text).child(title.to_string()))
        .child(div().text_sm().text_color(p.text_muted).child(subtitle.to_string()))
        .into_any_element()
}

/// "há 3 h" a partir de timestamp Unix.
pub fn relative_time(unix: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let s = (now - unix).max(0);
    match s {
        s if s < 60 => "agora".into(),
        s if s < 3600 => format!("há {} min", s / 60),
        s if s < 86_400 => format!("há {} h", s / 3600),
        s if s < 86_400 * 30 => format!("há {} d", s / 86_400),
        s if s < 86_400 * 365 => format!("há {} meses", s / (86_400 * 30)),
        s => format!("há {} anos", s / (86_400 * 365)),
    }
}
