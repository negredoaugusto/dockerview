//! Entidade raiz da aplicação: estado, assinaturas do core e layout geral.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use dockerview_core::{
    Action, AppState, Command, Connection, ContainerId, CoreHandle, Event, Filter, format_bytes,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Sizable as _, Theme, TitleBar, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Entity, FontWeight, IntoElement, Render, SharedString, Subscription, Task,
    UniformListScrollHandle, Window, div, px,
};

use crate::theme::{self, Palette};
use crate::widgets::{icon, stat_card};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Logs,
    Inspect,
}

pub struct DockerApp {
    core: CoreHandle,
    pub state: AppState,
    pub filter: Filter,
    pub search: Entity<InputState>,
    pub log_filter: Entity<InputState>,
    pub selected: Option<ContainerId>,
    pub tab: Tab,
    pub log_scroll: UniformListScrollHandle,
    pub inspect_scroll: UniformListScrollHandle,
    pub log_follow: bool,
    pub show_stdout: bool,
    pub show_stderr: bool,
    stats_subs: HashSet<ContainerId>,
    logs_sub: Option<ContainerId>,
    last_log_resubscribe: Instant,
    pub inspect_requested: HashSet<ContainerId>,
    /// Depuração: seleciona este container (DOCKERVIEW_SELECT) quando ele aparecer.
    pending_select: Option<String>,
    _subscriptions: Vec<Subscription>,
    _pump: Task<()>,
}

impl DockerApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let core = dockerview_core::spawn_core(None);

        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Buscar containers…"));
        let log_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filtrar logs…"));
        let subs = vec![
            cx.subscribe(&search, |_, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&log_filter, |_, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];

        // Ponte core → UI: aguarda eventos no executor do GPUI e aplica em lote.
        let rx = core.rx.clone();
        let pump = cx.spawn_in(window, async move |this, cx| {
            while let Ok(first) = rx.recv_async().await {
                let mut batch = vec![first];
                batch.extend(rx.try_iter());
                let alive = this.update_in(cx, |app, window, cx| {
                    app.apply_events(batch, window, cx);
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });

        Self {
            core,
            state: AppState::default(),
            filter: Filter::All,
            search,
            log_filter,
            selected: None,
            tab: match std::env::var("DOCKERVIEW_TAB").as_deref() {
                Ok("logs") => Tab::Logs,
                Ok("inspect") => Tab::Inspect,
                _ => Tab::Overview,
            },
            log_scroll: UniformListScrollHandle::new(),
            inspect_scroll: UniformListScrollHandle::new(),
            log_follow: true,
            show_stdout: true,
            show_stderr: true,
            stats_subs: HashSet::new(),
            logs_sub: None,
            last_log_resubscribe: Instant::now() - Duration::from_secs(60),
            inspect_requested: HashSet::new(),
            pending_select: std::env::var("DOCKERVIEW_SELECT").ok(),
            _subscriptions: subs,
            _pump: pump,
        }
    }

    /// Lateral recolhida (só ícones) quando o painel de detalhes divide uma janela estreita.
    pub fn sidebar_width(&self, window: &Window) -> gpui_kit::Pixels {
        if self.selected.is_some() && window.viewport_size().width < px(1250.) { px(64.) } else { px(220.) }
    }

    pub fn details_width(&self, window: &Window) -> gpui_kit::Pixels {
        if self.selected.is_some() { px(480.).min(window.viewport_size().width * 0.45).max(px(380.)) } else { px(0.) }
    }

    pub fn send(&self, cmd: Command) {
        self.core.send(cmd);
    }

    pub fn perform(&self, id: &str, action: Action) {
        self.send(Command::Perform { id: id.to_string(), action });
    }

    pub fn select(&mut self, id: Option<ContainerId>, cx: &mut Context<Self>) {
        if self.selected != id {
            self.selected = id;
            self.log_follow = true;
            self.sync_log_subscription();
            cx.notify();
        }
    }

    fn apply_events(&mut self, events: Vec<Event>, window: &mut Window, cx: &mut Context<Self>) {
        let mut containers_changed = false;
        let mut new_logs = false;
        for ev in events {
            match &ev {
                Event::Containers(_) => containers_changed = true,
                Event::LogLine { id, .. } if self.selected.as_ref() == Some(id) => new_logs = true,
                Event::Inspected { id, .. } => {
                    self.inspect_requested.remove(id);
                }
                Event::ActionDone { id, action } => {
                    let name = self.state.name_of(id).to_string();
                    let msg = match action {
                        Action::Start => format!("{name} iniciado"),
                        Action::Stop => format!("{name} parado"),
                        Action::Restart => format!("{name} reiniciado"),
                        Action::Pause => format!("{name} pausado"),
                        Action::Unpause => format!("{name} retomado"),
                        Action::Kill => format!("{name} finalizado"),
                        Action::Remove => format!("{name} removido"),
                    };
                    window.push_notification(Notification::success(msg), cx);
                }
                Event::Error(msg) => window.push_notification(Notification::error(msg.clone()), cx),
                _ => {}
            }
            self.state.apply(ev);
        }

        if containers_changed {
            if let Some(name) = self.pending_select.clone()
                && let Some(id) = self.state.containers.iter().find(|c| c.name == name).map(|c| c.id.clone())
            {
                self.pending_select = None;
                self.selected = Some(id);
            }
            self.sync_stats_subscriptions();
            if let Some(sel) = &self.selected
                && self.state.container(sel).is_none()
            {
                self.selected = None;
            }
        }
        self.sync_log_subscription();

        if new_logs && self.log_follow {
            self.scroll_logs_to_bottom(cx);
        }
    }

    pub fn scroll_logs_to_bottom(&self, cx: &mut Context<Self>) {
        let count = self.visible_log_count(cx);
        if count > 0 {
            self.log_scroll.scroll_to_item(count - 1, gpui_kit::ScrollStrategy::Bottom);
        }
    }

    fn sync_stats_subscriptions(&mut self) {
        let running = self.state.running_ids();
        for id in running.difference(&self.stats_subs) {
            self.core.send(Command::SubscribeStats(id.clone()));
        }
        for id in self.stats_subs.difference(&running) {
            self.core.send(Command::UnsubscribeStats(id.clone()));
        }
        self.stats_subs = running;
    }

    fn sync_log_subscription(&mut self) {
        if self.logs_sub != self.selected {
            if let Some(old) = self.logs_sub.take() {
                self.core.send(Command::UnsubscribeLogs(old));
            }
            if let Some(id) = self.selected.clone() {
                self.subscribe_logs(id);
            }
            return;
        }
        let Some(id) = self.selected.clone() else { return };
        let ended = self.state.logs.get(&id).is_some_and(|b| b.ended);
        let running = self.state.container(&id).is_some_and(|c| c.state.is_running());
        if ended && running && self.last_log_resubscribe.elapsed() > Duration::from_secs(2) {
            self.subscribe_logs(id);
        }
    }

    pub fn subscribe_logs(&mut self, id: ContainerId) {
        if let Some(buf) = self.state.logs.get_mut(&id) {
            buf.clear();
            buf.ended = false;
        }
        self.core.send(Command::SubscribeLogs { id: id.clone(), tail: "500".into() });
        self.logs_sub = Some(id);
        self.last_log_resubscribe = Instant::now();
    }

    pub fn request_inspect(&mut self, id: &str, force: bool) {
        if force || (!self.state.inspect.contains_key(id) && !self.inspect_requested.contains(id)) {
            self.inspect_requested.insert(id.to_string());
            self.send(Command::Inspect(id.to_string()));
        }
    }

    /// Abre o diálogo de confirmação de remoção.
    pub fn confirm_remove(&mut self, id: ContainerId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = self.state.container(&id).cloned() else { return };
        let running = c.state.is_running() || c.state == dockerview_core::ContainerState::Paused;
        let tx = self.core.tx.clone();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (id, tx) = (id.clone(), tx.clone());
            let desc = if running {
                format!("\"{}\" está em execução e será finalizado à força. Esta ação não pode ser desfeita.", c.name)
            } else {
                format!("\"{}\" será removido permanentemente.", c.name)
            };
            alert
                .title(format!("Remover {}?", c.name))
                .description(desc)
                .ok_text("Remover")
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .cancel_text("Cancelar")
                .show_cancel(true)
                .on_ok(move |_, _, _| {
                    let _ = tx.send(Command::Remove { id: id.clone(), force: running });
                    true
                })
        });
    }

    fn render_title_bar(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (conn_color, conn_text, conn_icon) = match &self.state.connection {
            Connection::Connecting => (p.amber, "Conectando…".to_string(), IconName::LoaderCircle),
            Connection::Connected { version } => (p.green, format!("Docker {version}"), IconName::Plug),
            Connection::Disconnected { .. } => (p.red, "Desconectado".to_string(), IconName::Unplug),
        };
        let is_dark = Theme::global(cx).is_dark();

        TitleBar::new().child(
            h_flex()
                .w_full()
                .pr_2()
                .justify_between()
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .size(px(22.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_md()
                                .bg(gpui_kit::linear_gradient(
                                    135.,
                                    gpui_kit::linear_color_stop(p.accent, 0.),
                                    gpui_kit::linear_color_stop(p.cyan, 1.),
                                ))
                                .child(icon(IconName::Ship, gpui_kit::white()).size_3p5()),
                        )
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(p.text).child("dockerview")),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .px_2()
                                .py_0p5()
                                .rounded_full()
                                .border_1()
                                .border_color(p.border)
                                .text_xs()
                                .text_color(p.text_muted)
                                .child(icon(conn_icon, conn_color).size_3())
                                .child(conn_text),
                        )
                        .child(
                            Button::new("theme-toggle")
                                .ghost()
                                .small()
                                .icon(if is_dark { IconName::Sun } else { IconName::Moon })
                                .tooltip(if is_dark { "Tema claro" } else { "Tema escuro" })
                                .on_click(|_, _, cx| theme::toggle(cx)),
                        ),
                ),
        )
    }

    fn render_sidebar(&self, p: &Palette, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = self.sidebar_width(window);
        let compact = width < px(100.);
        let item = |id: &'static str, icon_name: IconName, label: &'static str, filter: Filter, count: usize, color| {
            let active = self.filter == filter;
            h_flex()
                .id(id)
                .gap_2p5()
                .px_2p5()
                .py_1p5()
                .rounded_lg()
                .cursor_pointer()
                .text_sm()
                .when(compact, |d| d.justify_center().py_2())
                .when(active, |d| d.bg(p.accent_soft).text_color(p.accent_text).font_weight(FontWeight::MEDIUM))
                .when(!active, |d| d.text_color(p.text_muted).hover(|s| s.bg(p.surface_hover).text_color(p.text)))
                .child(icon(icon_name, if active { p.accent_text } else { color }).size_4())
                .when(!compact, |d| {
                    d.child(div().flex_1().child(label)).child(
                        div()
                            .px_1p5()
                            .rounded_md()
                            .text_xs()
                            .font_family(theme::MONO_FONT)
                            .bg(if active { p.accent.alpha(0.2) } else { p.surface_hover })
                            .child(count.to_string()),
                    )
                })
                .when(compact, |d| d.tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(format!("{label} ({count})")).build(window, cx)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.filter = filter;
                    cx.notify();
                }))
        };

        let running = self.state.count(Filter::Running);
        let stopped = self.state.count(Filter::Stopped);

        v_flex()
            .w(width)
            .h_full()
            .flex_shrink_0()
            .bg(p.sidebar)
            .border_r_1()
            .border_color(p.border)
            .p_3()
            .gap_1()
            .when(!compact, |d| d.child(section_label("CONTAINERS", p)))
            .child(item("nav-all", IconName::Boxes, "Todos", Filter::All, self.state.containers.len(), p.text_muted))
            .child(item("nav-running", IconName::Activity, "Ativos", Filter::Running, running, p.green))
            .child(item("nav-stopped", IconName::CircleStop, "Parados", Filter::Stopped, stopped, p.gray))
            .child(div().flex_1())
            .when(!compact, |d| d.child(self.render_engine_card(p)))
    }

    fn render_engine_card(&self, p: &Palette) -> impl IntoElement {
        let (cpu, mem) = self.totals();
        v_flex()
            .gap_2()
            .p_3()
            .rounded_xl()
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
            .child(
                h_flex()
                    .gap_2()
                    .child(icon(IconName::Server, p.accent).size_4())
                    .child(div().text_xs().font_weight(FontWeight::MEDIUM).text_color(p.text).child("Docker Engine")),
            )
            .child(
                div().text_xs().text_color(p.text_faint).child(match &self.state.connection {
                    Connection::Connected { version } => version.clone(),
                    Connection::Connecting => "conectando…".into(),
                    Connection::Disconnected { reason } => reason.clone(),
                }),
            )
            .child(
                h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(p.text_muted)
                    .child(format!("CPU {cpu:.1}%"))
                    .child(format!("RAM {}", format_bytes(mem))),
            )
    }

    /// Soma de CPU e memória dos containers em execução.
    pub fn totals(&self) -> (f64, u64) {
        self.state
            .containers
            .iter()
            .filter(|c| c.state.is_running())
            .filter_map(|c| self.state.stats.get(&c.id).and_then(|h| h.latest()))
            .fold((0.0, 0), |(cpu, mem), s| (cpu + s.cpu_percent, mem + s.mem_usage))
    }

    fn render_header(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, subtitle) = match self.filter {
            Filter::All => ("Todos os containers", "Visão geral de tudo que existe no seu Docker"),
            Filter::Running => ("Ativos", "Containers rodando ou pausados"),
            Filter::Stopped => ("Parados", "Containers que terminaram ou nunca iniciaram"),
        };
        h_flex()
            .gap_4()
            .justify_between()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).text_color(p.text).truncate().child(title))
                    .child(div().text_sm().text_color(p.text_muted).truncate().child(subtitle)),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_2()
                    .child(
                        div().w(if self.selected.is_some() { px(200.) } else { px(260.) }).child(
                            Input::new(&self.search)
                                .prefix(icon(IconName::Search, p.text_faint).size_4())
                                .cleanable(true),
                        ),
                    )
                    .child(
                        Button::new("refresh")
                            .outline()
                            .icon(IconName::RefreshCw)
                            .tooltip("Recarregar")
                            .on_click(cx.listener(|this, _, _, _| this.send(Command::Refresh))),
                    ),
            )
    }

    fn render_summary(&self, p: &Palette) -> impl IntoElement {
        let total = self.state.containers.len();
        let running = self.state.containers.iter().filter(|c| c.state.is_running()).count();
        let paused =
            self.state.containers.iter().filter(|c| c.state == dockerview_core::ContainerState::Paused).count();
        let stopped = total - running - paused;
        let (cpu, mem) = self.totals();
        let mem_limit = self.state.stats.values().filter_map(|h| h.latest()).map(|s| s.mem_limit).max().unwrap_or(0);

        h_flex()
            .gap_3()
            .child(stat_card(
                IconName::Activity,
                "Rodando",
                running.to_string(),
                format!("de {total} containers"),
                p.green,
                p,
            ))
            .child(stat_card(
                IconName::CirclePause,
                "Pausados · parados",
                format!("{paused} / {stopped}"),
                "prontos para iniciar",
                p.amber,
                p,
            ))
            .child(stat_card(IconName::Cpu, "CPU total", format!("{cpu:.1}%"), "soma dos containers ativos", p.accent, p))
            .child(stat_card(
                IconName::MemoryStick,
                "Memória total",
                format_bytes(mem),
                if mem_limit > 0 { format!("limite do host {}", format_bytes(mem_limit)) } else { "—".into() },
                p.cyan,
                p,
            ))
    }
}

pub fn section_label(text: &'static str, p: &Palette) -> impl IntoElement {
    div()
        .px_2p5()
        .pt_2()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.text_faint)
        .child(text)
}

impl Render for DockerApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        let width = window.viewport_size().width;
        let details_open = self.selected.is_some();
        let details_width = self.details_width(window);
        let query: SharedString = self.search.read(cx).value();

        v_flex()
            .size_full()
            .bg(p.bg)
            .text_color(p.text)
            .font_family(theme::UI_FONT)
            .child(self.render_title_bar(&p, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(self.render_sidebar(&p, window, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .p_5()
                            .gap_5()
                            .child(self.render_header(&p, cx))
                            .when(!details_open || width > px(1250.), |d| d.child(self.render_summary(&p)))
                            .child(self.render_list(&query, &p, window, cx)),
                    )
                    .when(details_open, |d| d.child(self.render_details(details_width, &p, window, cx))),
            )
    }
}
