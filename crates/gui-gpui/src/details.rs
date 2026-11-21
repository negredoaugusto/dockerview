//! Painel lateral de detalhes: visão geral (métricas + gráficos), logs e inspect.

use std::time::SystemTime;

use dockerview_core::{Action, ContainerState, ContainerSummary, LogLine, LogStream, StatsSample, format_bytes};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::chart::AreaChart;
use gpui_kit::component::input::Input;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClipboardItem, Context, FontWeight, Hsla, IntoElement, Pixels, SharedString, Window, div,
    linear_color_stop, linear_gradient, px, uniform_list,
};

use crate::app::{DockerApp, Tab, section_label};
use crate::theme::{MONO_FONT, Palette};
use crate::widgets::{chip, empty_state, icon, info_row, relative_time, state_pill};

const LOG_ROW_HEIGHT: f32 = 19.;

#[derive(Clone)]
struct ChartPoint {
    label: SharedString,
    value: f64,
}

impl DockerApp {
    pub fn render_details(&mut self, width: Pixels, p: &Palette, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(c) = self.selected.as_ref().and_then(|id| self.state.container(id)).cloned() else {
            return div().into_any_element();
        };
        if self.tab == Tab::Inspect {
            self.request_inspect(&c.id, false);
        }

        v_flex()
            .w(width)
            .h_full()
            .flex_shrink_0()
            .bg(p.sidebar)
            .border_l_1()
            .border_color(p.border)
            .child(self.render_details_header(&c, p, window, cx))
            .child(self.render_tabs(p, cx))
            .child(
                div().flex_1().min_h_0().child(match self.tab {
                    Tab::Overview => self.render_overview(&c, p),
                    Tab::Logs => self.render_logs(&c, p, cx),
                    Tab::Inspect => self.render_inspect(&c, p, cx),
                }),
            )
            .into_any_element()
    }

    fn render_details_header(&self, c: &ContainerSummary, p: &Palette, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let color = p.state(c.state);
        let running = c.state.is_running();
        let paused = c.state == ContainerState::Paused;
        let entity = cx.entity();

        let action_btn = |id: &'static str, icon_name: IconName, label: &'static str, action: Action| {
            let (entity, cid) = (entity.clone(), c.id.clone());
            Button::new(id).outline().small().icon(icon_name).label(label).on_click(move |_, _, cx| {
                entity.update(cx, |this, _| this.perform(&cid, action));
            })
        };
        let kill_btn = {
            let (entity, cid) = (entity.clone(), c.id.clone());
            Button::new("d-kill").ghost().small().icon(IconName::Zap).tooltip("Kill (SIGKILL)").on_click(move |_, _, cx| {
                entity.update(cx, |this, _| this.perform(&cid, Action::Kill));
            })
        };

        v_flex()
            .gap_3()
            .p_4()
            .border_b_1()
            .border_color(p.border)
            // Faixa de cor do estado no topo do painel.
            .bg(linear_gradient(180., linear_color_stop(color.alpha(0.10), 0.), linear_color_stop(p.sidebar, 1.)))
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        div()
                            .size(px(40.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_xl()
                            .bg(color.alpha(0.15))
                            .border_1()
                            .border_color(color.alpha(0.3))
                            .child(icon(IconName::Container, color).size_5()),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).text_color(p.text).truncate().child(c.name.clone()))
                            .child(h_flex().gap_2().child(state_pill(c.state, p)).child(chip(c.short_id().to_string(), p))),
                    )
                    .child(
                        Button::new("close-details")
                            .ghost()
                            .small()
                            .icon(IconName::X)
                            .tooltip("Fechar")
                            .on_click(cx.listener(|this, _, _, cx| this.select(None, cx))),
                    ),
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .flex_wrap()
                    .when(!running && !paused, |d| d.child(action_btn("d-start", IconName::Play, "Iniciar", Action::Start)))
                    .when(running, |d| {
                        d.child(action_btn("d-stop", IconName::Square, "Parar", Action::Stop))
                            .child(action_btn("d-restart", IconName::RotateCw, "Reiniciar", Action::Restart))
                            .child(action_btn("d-pause", IconName::Pause, "Pausar", Action::Pause))
                    })
                    .when(paused, |d| {
                        d.child(action_btn("d-unpause", IconName::Play, "Retomar", Action::Unpause))
                            .child(action_btn("d-stop", IconName::Square, "Parar", Action::Stop))
                    })
                    .child(div().flex_1())
                    .when(running || paused, |d| d.child(kill_btn))
                    .child({
                        let cid = c.id.clone();
                        Button::new("d-remove")
                            .danger()
                            .small()
                            .icon(IconName::Trash)
                            .tooltip("Remover container")
                            .on_click(cx.listener(move |this, _, window, cx| this.confirm_remove(cid.clone(), window, cx)))
                    }),
            )
    }

    fn render_tabs(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = |id: &'static str, t: Tab, icon_name: IconName, label: &'static str| {
            let active = self.tab == t;
            h_flex()
                .id(id)
                .flex_1()
                .justify_center()
                .gap_1p5()
                .py_1p5()
                .rounded_md()
                .text_sm()
                .cursor_pointer()
                .when(active, |d| d.bg(p.surface).text_color(p.text).font_weight(FontWeight::MEDIUM).shadow_sm())
                .when(!active, |d| d.text_color(p.text_muted).hover(|s| s.text_color(p.text)))
                .child(icon(icon_name, if active { p.accent } else { p.text_faint }).size_4())
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tab = t;
                    if t == Tab::Logs {
                        this.log_follow = true;
                        this.scroll_logs_to_bottom(cx);
                    }
                    cx.notify();
                }))
        };
        div().px_4().pt_3().child(
            h_flex()
                .p_1()
                .gap_1()
                .rounded_lg()
                .bg(p.surface_hover)
                .border_1()
                .border_color(p.border)
                .child(tab("tab-overview", Tab::Overview, IconName::LayoutDashboard, "Visão geral"))
                .child(tab("tab-logs", Tab::Logs, IconName::ScrollText, "Logs"))
                .child(tab("tab-inspect", Tab::Inspect, IconName::Braces, "Inspect")),
        )
    }

    fn render_overview(&self, c: &ContainerSummary, p: &Palette) -> AnyElement {
        let history = self.state.stats.get(&c.id).filter(|_| c.state.is_running());
        let latest = history.and_then(|h| h.latest()).copied();

        let mut body = v_flex().id("overview").size_full().overflow_y_scroll().p_4().gap_4();

        if let (Some(h), Some(s)) = (history, latest) {
            let samples: Vec<(SystemTime, StatsSample)> = h.timed().map(|(t, s)| (t, *s)).collect();
            body = body
                .child(
                    h_flex()
                        .gap_2()
                        .child(mini_metric(IconName::Cpu, "CPU do host", format!("{:.1}%", s.cpu_host_percent()), p.accent, p))
                        .child(mini_metric(IconName::MemoryStick, "Memória", format_bytes(s.mem_usage), p.cyan, p))
                        .child(mini_metric(IconName::Hash, "PIDs", s.pids.to_string(), p.pink, p)),
                )
                .child(chart_card(
                    "CPU (% do host)",
                    format!("{:.2}% · {:.2} de {} núcleos", s.cpu_host_percent(), s.cores_used(), s.online_cpus),
                    series(&samples, |s| s.cpu_host_percent()),
                    p.accent,
                    p,
                ))
                .child(chart_card(
                    "Memória",
                    format!("{} de {}", format_bytes(s.mem_usage), format_bytes(s.mem_limit)),
                    series(&samples, |s| s.mem_usage as f64 / 1_048_576.0),
                    p.cyan,
                    p,
                ))
                .child(
                    h_flex()
                        .gap_2()
                        .child(mini_metric(IconName::ArrowDown, "Rede RX", format_bytes(s.net_rx), p.green, p))
                        .child(mini_metric(IconName::ArrowUp, "Rede TX", format_bytes(s.net_tx), p.amber, p)),
                );
        } else {
            body = body.child(
                h_flex()
                    .gap_3()
                    .p_4()
                    .rounded_xl()
                    .bg(p.surface)
                    .border_1()
                    .border_color(p.border)
                    .child(icon(if c.state.is_running() { IconName::LoaderCircle } else { IconName::Power }, p.text_faint).size_5())
                    .child(div().text_sm().text_color(p.text_muted).child(if c.state.is_running() {
                        "Coletando estatísticas…"
                    } else {
                        "Inicie o container para ver CPU, memória e rede em tempo real."
                    })),
            );
        }

        let ports: Vec<String> = c.ports.iter().map(ToString::to_string).collect();
        body = body.child(section_label("DETALHES", p)).child(
            v_flex()
                .px_1()
                .child(info_row("Imagem", div().font_family(MONO_FONT).child(c.image.clone()), p))
                .child(info_row("Comando", div().font_family(MONO_FONT).text_xs().child(c.command.clone()), p))
                .child(info_row("Status", c.status.clone(), p))
                .child(info_row("Criado", relative_time(c.created), p))
                .child(info_row(
                    "Portas",
                    if ports.is_empty() {
                        div().text_color(p.text_faint).child("nenhuma").into_any_element()
                    } else {
                        h_flex().gap_1().flex_wrap().children(ports.into_iter().map(|t| chip(t, p))).into_any_element()
                    },
                    p,
                ))
                .when_some(c.compose_project.clone(), |d, proj| d.child(info_row("Compose", proj, p)))
                .child(info_row("ID", div().font_family(MONO_FONT).text_xs().child(c.id.clone()), p)),
        );
        body.into_any_element()
    }

    pub fn visible_log_count(&self, cx: &gpui_kit::App) -> usize {
        self.filtered_logs(cx).len()
    }

    fn filtered_logs(&self, cx: &gpui_kit::App) -> Vec<LogLine> {
        let Some(id) = &self.selected else { return vec![] };
        let Some(buf) = self.state.logs.get(id) else { return vec![] };
        let filter = self.log_filter.read(cx).value().to_lowercase();
        buf.lines()
            .iter()
            .filter(|l| match l.stream {
                LogStream::Stderr => self.show_stderr,
                _ => self.show_stdout,
            })
            .filter(|l| filter.is_empty() || l.text.to_lowercase().contains(&filter))
            .cloned()
            .collect()
    }

    fn render_logs(&self, c: &ContainerSummary, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let lines = self.filtered_logs(cx);
        let buf = self.state.logs.get(&c.id);
        let total = buf.map_or(0, |b| b.len());
        let ended = buf.is_some_and(|b| b.ended);
        let pal = *p;

        let toggle = |id: &'static str, label: &'static str, on: bool, color: Hsla, f: fn(&mut DockerApp)| {
            h_flex()
                .id(id)
                .gap_1p5()
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .text_xs()
                .cursor_pointer()
                .when(on, |d| d.border_color(color.alpha(0.5)).bg(color.alpha(0.12)).text_color(p.text))
                .when(!on, |d| d.border_color(p.border).text_color(p.text_faint))
                .child(div().size(px(6.)).rounded_full().bg(if on { color } else { p.text_faint }))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    f(this);
                    cx.notify();
                }))
        };

        let view = uniform_list("logs", lines.len(), move |range, _, _| {
            range
                .map(|i| {
                    let l = &lines[i];
                    let err = l.stream == LogStream::Stderr;
                    h_flex()
                        .w_full()
                        .h(px(LOG_ROW_HEIGHT))
                        .px_3()
                        .gap_3()
                        .when(err, |d| d.bg(pal.red.alpha(0.08)).border_l_2().border_color(pal.red.alpha(0.6)))
                        .child(div().w(px(36.)).flex_shrink_0().text_right().text_color(pal.log_text.alpha(0.3)).child((i + 1).to_string()))
                        .child(
                            div()
                                .flex_1()
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_color(if err { pal.log_err } else { pal.log_text })
                                .child(l.text.clone()),
                        )
                })
                .collect()
        })
        .track_scroll(&self.log_scroll)
        .size_full();

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.log_filter).small().prefix(icon(IconName::ListFilter, p.text_faint).size_3p5()).cleanable(true)))
                    .child(toggle("t-stdout", "stdout", self.show_stdout, p.blue, |a| a.show_stdout = !a.show_stdout))
                    .child(toggle("t-stderr", "stderr", self.show_stderr, p.red, |a| a.show_stderr = !a.show_stderr)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .rounded_xl()
                    .overflow_hidden()
                    .bg(p.log_bg)
                    .border_1()
                    .border_color(if p.dark { p.border } else { p.log_bg })
                    .font_family(MONO_FONT)
                    .text_xs()
                    // Barra estilo terminal.
                    .child(
                        h_flex()
                            .px_3()
                            .py_2()
                            .gap_2()
                            .border_b_1()
                            .border_color(gpui_kit::white().alpha(0.06))
                            .child(div().size(px(9.)).rounded_full().bg(gpui_kit::rgb(0xff5f57)))
                            .child(div().size(px(9.)).rounded_full().bg(gpui_kit::rgb(0xfebc2e)))
                            .child(div().size(px(9.)).rounded_full().bg(gpui_kit::rgb(0x28c840)))
                            .child(div().flex_1().text_center().text_color(p.log_text.alpha(0.5)).child(format!("{} — logs", c.name)))
                            .child(
                                Button::new("log-follow")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::ArrowDownToLine)
                                    .tooltip(if self.log_follow { "Seguindo (clique para pausar)" } else { "Seguir novas linhas" })
                                    .when(self.log_follow, |b| b.primary())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.log_follow = !this.log_follow;
                                        if this.log_follow {
                                            this.scroll_logs_to_bottom(cx);
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("log-clear").ghost().xsmall().icon(IconName::Eraser).tooltip("Limpar").on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(b) = this.selected.as_ref().and_then(|id| this.state.logs.get_mut(id)) {
                                        b.clear();
                                    }
                                    cx.notify();
                                })),
                            )
                            .child(
                                Button::new("log-reload").ghost().xsmall().icon(IconName::RefreshCw).tooltip("Recarregar").on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(id) = this.selected.clone() {
                                        this.subscribe_logs(id);
                                    }
                                    cx.notify();
                                })),
                            ),
                    )
                    .child(
                        div().flex_1().min_h_0().py_1().child(if total == 0 {
                            div()
                                .p_4()
                                .text_color(p.log_text.alpha(0.4))
                                .child(if c.state.is_running() { "$ aguardando saída…" } else { "$ sem logs" })
                                .into_any_element()
                        } else {
                            view.into_any_element()
                        }),
                    ),
            )
            .child(
                h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(p.text_faint)
                    .child(format!("{} de {} linhas", self.visible_log_count(cx), total))
                    .when(ended, |d| {
                        d.child(h_flex().gap_1().text_color(p.amber).child(icon(IconName::CircleAlert, p.amber).size_3()).child("stream encerrado"))
                    }),
            )
            .into_any_element()
    }

    fn render_inspect(&self, c: &ContainerSummary, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(json) = self.state.inspect.get(&c.id).cloned() else {
            return empty_state(IconName::LoaderCircle, "Carregando…", "Consultando docker inspect", p);
        };
        let lines: Vec<String> = json.lines().map(str::to_string).collect();
        let count = lines.len();
        let pal = *p;
        let json_copy = json.clone();
        let cid = c.id.clone();

        let view = uniform_list("inspect", count, move |range, _, _| {
            range
                .map(|i| {
                    h_flex()
                        .h(px(LOG_ROW_HEIGHT))
                        .px_3()
                        .gap_3()
                        .child(div().w(px(36.)).flex_shrink_0().text_right().text_color(pal.log_text.alpha(0.3)).child((i + 1).to_string()))
                        .child(json_line(&lines[i], &pal))
                })
                .collect()
        })
        .track_scroll(&self.inspect_scroll)
        .size_full();

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .justify_between()
                    .child(div().text_xs().text_color(p.text_faint).child(format!("{count} linhas · docker inspect")))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(Button::new("insp-copy").outline().small().icon(IconName::Copy).label("Copiar").on_click(move |_, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(json_copy.clone()));
                                window.push_notification(gpui_kit::component::notification::Notification::info("JSON copiado"), cx);
                            }))
                            .child(
                                Button::new("insp-refresh")
                                    .outline()
                                    .small()
                                    .icon(IconName::RefreshCw)
                                    .on_click(cx.listener(move |this, _, _, _| this.request_inspect(&cid, true))),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .py_2()
                    .rounded_xl()
                    .bg(p.log_bg)
                    .font_family(MONO_FONT)
                    .text_xs()
                    .child(view),
            )
            .into_any_element()
    }
}

/// Uma linha de JSON formatado com cores simples: chave, string, número, literal.
fn json_line(line: &str, p: &Palette) -> impl IntoElement {
    let indent = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();
    let key_color = if p.dark { p.accent_text } else { gpui_kit::rgb(0xa5a6ff).into() };
    let value_color = |v: &str| -> Hsla {
        let v = v.trim_end_matches(',');
        if v.starts_with('"') {
            gpui_kit::rgb(0x7ee2b8).into()
        } else if v == "true" || v == "false" || v == "null" {
            gpui_kit::rgb(0xf472b6).into()
        } else if v.parse::<f64>().is_ok() {
            gpui_kit::rgb(0xfbbf24).into()
        } else {
            p.log_text
        }
    };

    let mut row = h_flex().whitespace_nowrap().child(div().w(px(indent as f32 * 6.6)));
    if let Some(rest) = trimmed.strip_prefix('"')
        && let Some(end) = rest.find("\":")
    {
        let key = &trimmed[..end + 2];
        let value = rest[end + 2..].trim_start();
        row = row.child(div().text_color(key_color).child(key.to_string())).child(div().text_color(p.log_text.alpha(0.5)).child(": "));
        if !value.is_empty() {
            row = row.child(div().text_color(value_color(value)).overflow_hidden().text_ellipsis().child(value.to_string()));
        }
    } else {
        row = row.child(div().text_color(value_color(trimmed)).child(trimmed.to_string()));
    }
    row
}

/// Pontos do gráfico com a hora local de cada amostra no eixo X (ex.: 12:31:05).
fn series(samples: &[(SystemTime, StatsSample)], f: fn(&StatsSample) -> f64) -> Vec<ChartPoint> {
    samples
        .iter()
        .map(|(t, s)| ChartPoint { label: clock(*t).into(), value: f(s) })
        .collect()
}

fn clock(t: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t).format("%H:%M:%S").to_string()
}

fn chart_card(title: &'static str, value: String, data: Vec<ChartPoint>, color: Hsla, p: &Palette) -> impl IntoElement {
    let n = data.len().max(2);
    v_flex()
        .gap_2()
        .p_4()
        .rounded_xl()
        .bg(p.surface)
        .border_1()
        .border_color(p.border)
        .child(
            h_flex()
                .justify_between()
                .child(div().text_sm().text_color(p.text_muted).child(title))
                .child(div().text_sm().font_family(MONO_FONT).font_weight(FontWeight::MEDIUM).text_color(color).child(value)),
        )
        .child(
            div().h(px(130.)).child(
                AreaChart::new(data)
                    .id(title)
                    .x(|d: &ChartPoint| d.label.clone())
                    .y(|d: &ChartPoint| d.value)
                    .stroke(color)
                    .fill(linear_gradient(180., linear_color_stop(color.alpha(0.35), 0.), linear_color_stop(color.alpha(0.0), 1.)))
                    .linear()
                    .tick_margin((n / 4).max(1))
                    .appear(false),
            ),
        )
}

fn mini_metric(icon_name: IconName, label: &'static str, value: String, color: Hsla, p: &Palette) -> impl IntoElement {
    h_flex()
        .flex_1()
        .gap_2p5()
        .p_3()
        .rounded_xl()
        .bg(p.surface)
        .border_1()
        .border_color(p.border)
        .child(
            div()
                .size(px(30.))
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .rounded_lg()
                .bg(color.alpha(0.14))
                .child(icon(icon_name, color).size_4()),
        )
        .child(
            v_flex()
                .min_w_0()
                .child(div().text_xs().text_color(p.text_faint).child(label))
                .child(div().text_sm().font_family(MONO_FONT).font_weight(FontWeight::MEDIUM).text_color(p.text).truncate().child(value)),
        )
}
