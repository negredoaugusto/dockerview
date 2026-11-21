//! Lista de containers em cartões.

use dockerview_core::{Action, ContainerState, ContainerSummary, format_bytes};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, FontWeight, IntoElement, Pixels, Window, div, px};

use crate::app::DockerApp;
use crate::theme::{MONO_FONT, Palette};
use crate::widgets::{chip, empty_state, meter, state_dot, state_pill};

/// Colunas opcionais, mostradas conforme a largura disponível.
struct Layout {
    actions: bool,
    status: bool,
    metrics: bool,
    ports: bool,
    /// Maior uso de memória entre os containers (escala da barra de RAM).
    max_mem: u64,
}

impl Layout {
    fn for_width(w: Pixels, max_mem: u64) -> Self {
        Self { actions: w > px(400.), status: w > px(520.), metrics: w > px(660.), ports: w > px(880.), max_mem }
    }
}

impl DockerApp {
    pub fn render_list(&self, query: &str, p: &Palette, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<ContainerSummary> = self.state.filtered(self.filter, query).into_iter().cloned().collect();

        if rows.is_empty() {
            let (title, sub) = if self.state.containers.is_empty() {
                ("Nenhum container", "Crie um container com `docker run` e ele aparece aqui.")
            } else {
                ("Nada encontrado", "Nenhum container corresponde ao filtro ou à busca.")
            };
            return div().flex_1().child(empty_state(IconName::Container, title, sub, p)).into_any_element();
        }

        let viewport = window.viewport_size().width;
        let details = self.details_width(window);
        let sidebar = self.sidebar_width(window);
        let max_mem = self.state.stats.values().filter_map(|h| h.latest()).map(|s| s.mem_usage).max().unwrap_or(0);
        let layout = Layout::for_width(viewport - sidebar - px(40.) - details, max_mem);

        v_flex()
            .id("container-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap_2()
            .children(rows.iter().enumerate().map(|(i, c)| self.render_row(i, c, &layout, p, cx)))
            .into_any_element()
    }

    fn render_row(&self, ix: usize, c: &ContainerSummary, layout: &Layout, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected.as_deref() == Some(c.id.as_str());
        let stats = self.state.stats.get(&c.id).and_then(|h| h.latest()).copied().filter(|_| c.state.is_running());
        let id = c.id.clone();

        h_flex()
            .id(("row", ix))
            .gap_4()
            .px_4()
            .py_3()
            .rounded_xl()
            .border_1()
            .cursor_pointer()
            .when(selected, |d| d.bg(p.surface_selected).border_color(p.accent.alpha(0.6)))
            .when(!selected, |d| d.bg(p.surface).border_color(p.border).hover(|s| s.bg(p.surface_hover).border_color(p.border_strong)))
            .on_click(cx.listener(move |this, _, _, cx| {
                let new = if this.selected.as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
                this.select(new, cx);
            }))
            .child(state_dot(c.state, p))
            // Nome + imagem
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(140.))
                    .gap_0p5()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).text_color(p.text).truncate().child(c.name.clone()))
                    .child(
                        h_flex()
                            .gap_1p5()
                            .text_xs()
                            .text_color(p.text_faint)
                            .child(div().font_family(MONO_FONT).truncate().child(c.image.clone()))
                            .when_some(c.compose_project.clone(), |d, proj| {
                                d.child(
                                    div()
                                        .px_1()
                                        .rounded_sm()
                                        .bg(p.pink.alpha(0.12))
                                        .text_color(p.pink)
                                        .child(proj),
                                )
                            }),
                    ),
            )
            .when(layout.status, |d| {
                d.child(
                    v_flex()
                        .w(px(150.))
                        .flex_shrink_0()
                        .gap_1()
                        .child(div().flex().child(state_pill(c.state, p)))
                        .child(div().text_xs().text_color(p.text_faint).truncate().child(c.status.clone())),
                )
            })
            .when(layout.metrics, |d| {
                d.child(metric_cell("CPU", stats.map(|s| format!("{:.1}%", s.cpu_percent)), stats.map(|s| (s.cpu_percent / 100.0) as f32), p.accent, p))
                    .child(metric_cell(
                        "RAM",
                        stats.map(|s| format_bytes(s.mem_usage)),
                        stats.map(|s| if layout.max_mem > 0 { s.mem_usage as f32 / layout.max_mem as f32 } else { 0.0 }),
                        p.cyan,
                        p,
                    ))
            })
            .when(layout.ports, |d| {
                let published: Vec<String> = c
                    .ports
                    .iter()
                    .filter_map(|pt| pt.public_port.map(|pp| format!("{pp}→{}", pt.private_port)))
                    .collect();
                d.child(
                    h_flex()
                        .w(px(150.))
                        .flex_shrink_0()
                        .gap_1()
                        .flex_wrap()
                        .children(published.into_iter().take(3).map(|t| chip(t, p))),
                )
            })
            .when(layout.actions, |d| d.child(row_actions(ix, c, cx)))
            .into_any_element()
    }
}

fn metric_cell(label: &'static str, value: Option<String>, fraction: Option<f32>, color: gpui_kit::Hsla, p: &Palette) -> impl IntoElement {
    v_flex()
        .w(px(96.))
        .flex_shrink_0()
        .gap_1p5()
        .child(
            h_flex()
                .justify_between()
                .text_xs()
                .child(div().text_color(p.text_faint).child(label))
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_color(if value.is_some() { p.text } else { p.text_faint })
                        .child(value.unwrap_or_else(|| "—".into())),
                ),
        )
        .child(meter(fraction.unwrap_or(0.0), color, p))
}

fn row_actions(ix: usize, c: &ContainerSummary, cx: &mut Context<DockerApp>) -> impl IntoElement {
    let mut buttons: Vec<(IconName, &'static str, Option<Action>)> = Vec::new();
    match c.state {
        ContainerState::Running | ContainerState::Restarting => {
            buttons.push((IconName::Square, "Parar", Some(Action::Stop)));
            buttons.push((IconName::Pause, "Pausar", Some(Action::Pause)));
            buttons.push((IconName::RotateCw, "Reiniciar", Some(Action::Restart)));
        }
        ContainerState::Paused => {
            buttons.push((IconName::Play, "Retomar", Some(Action::Unpause)));
            buttons.push((IconName::Square, "Parar", Some(Action::Stop)));
        }
        _ => buttons.push((IconName::Play, "Iniciar", Some(Action::Start))),
    }
    buttons.push((IconName::Trash, "Remover", None));

    h_flex().gap_0p5().flex_shrink_0().children(buttons.into_iter().enumerate().map(|(bi, (icon_name, tip, action))| {
        let id = c.id.clone();
        let entity = cx.entity();
        Button::new(("act", ix * 10 + bi))
            .ghost()
            .small()
            .icon(icon_name)
            .tooltip(tip)
            .on_click(move |_, window, cx: &mut App| {
                cx.stop_propagation();
                entity.update(cx, |this, cx| match action {
                    Some(a) => this.perform(&id, a),
                    None => this.confirm_remove(id.clone(), window, cx),
                });
            })
    }))
}
