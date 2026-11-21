use std::collections::HashSet;
use std::time::{Duration, Instant};

use dockerview_core::{
    Action, AppState, Command, Connection, ContainerId, CoreHandle, Filter, NotificationKind,
};
use egui::{Color32, RichText};

use crate::details::{DetailsTab, LogView};
use crate::util::ERROR_COLOR;

pub struct DockerViewApp {
    pub core: CoreHandle,
    pub state: AppState,
    pub filter: Filter,
    pub search: String,
    pub selected: Option<ContainerId>,
    pub tab: DetailsTab,
    pub log_view: LogView,
    /// Pedido de confirmação de remoção: (id, force).
    pub confirm_remove: Option<(ContainerId, bool)>,
    /// Containers com stats assinados (todos os que estão rodando).
    stats_subs: HashSet<ContainerId>,
    /// Container cujos logs estão sendo seguidos.
    logs_sub: Option<ContainerId>,
    last_log_resubscribe: Instant,
    pub inspect_requested: HashSet<ContainerId>,
    #[cfg(feature = "screenshot")]
    pub screenshot: Option<crate::screenshot::Screenshot>,
}

impl DockerViewApp {
    pub fn new(core: CoreHandle) -> Self {
        Self {
            core,
            state: AppState::default(),
            filter: Filter::All,
            search: String::new(),
            selected: None,
            tab: DetailsTab::Info,
            log_view: LogView::default(),
            confirm_remove: None,
            stats_subs: HashSet::new(),
            logs_sub: None,
            last_log_resubscribe: Instant::now() - Duration::from_secs(60),
            inspect_requested: HashSet::new(),
            #[cfg(feature = "screenshot")]
            screenshot: crate::screenshot::Screenshot::from_env(),
        }
    }

    pub fn send(&self, cmd: Command) {
        self.core.send(cmd);
    }

    pub fn perform(&self, id: &str, action: Action) {
        self.send(Command::Perform { id: id.to_string(), action });
    }

    pub fn select(&mut self, id: Option<ContainerId>) {
        if self.selected != id {
            self.selected = id;
            self.log_view.reset_scroll();
        }
    }

    /// Consome todos os eventos pendentes do core.
    fn pump_events(&mut self) {
        let mut containers_changed = false;
        while let Ok(ev) = self.core.rx.try_recv() {
            containers_changed |= matches!(ev, dockerview_core::Event::Containers(_));
            if let dockerview_core::Event::Inspected { id, .. } = &ev {
                self.inspect_requested.remove(id);
            }
            self.state.apply(ev);
        }
        if containers_changed {
            self.sync_stats_subscriptions();
            if let Some(sel) = &self.selected
                && self.state.container(sel).is_none()
            {
                self.select(None);
            }
        }
        self.sync_log_subscription();
    }

    /// Mantém stats assinados exatamente para os containers rodando.
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

    /// Segue os logs do container selecionado; reassina se ele voltar a rodar.
    fn sync_log_subscription(&mut self) {
        if self.logs_sub != self.selected {
            if let Some(old) = self.logs_sub.take() {
                self.core.send(Command::UnsubscribeLogs(old));
            }
            if let Some(id) = &self.selected {
                self.subscribe_logs(id.clone());
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

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("🐳 dockerview").strong());
            ui.separator();

            for (filter, label) in [(Filter::All, "Todos"), (Filter::Running, "Rodando"), (Filter::Stopped, "Parados")] {
                let text = format!("{label} ({})", self.state.count(filter));
                ui.selectable_value(&mut self.filter, filter, text);
            }
            ui.separator();

            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("🔍 Buscar por nome, imagem ou ID")
                    .desired_width(280.0),
            );
            if !self.search.is_empty() && ui.small_button("✖").on_hover_text("Limpar busca").clicked() {
                self.search.clear();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⟳").on_hover_text("Recarregar").clicked() {
                    self.send(Command::Refresh);
                }
                match &self.state.connection {
                    Connection::Connecting => {
                        ui.spinner();
                        ui.label("Conectando…");
                    }
                    Connection::Connected { version } => {
                        ui.label(RichText::new(format!("Docker {version}")).weak());
                        crate::util::status_dot(ui, Color32::from_rgb(80, 200, 120), 5.0);
                    }
                    Connection::Disconnected { reason } => {
                        ui.label(RichText::new("desconectado").color(ERROR_COLOR)).on_hover_text(reason);
                        crate::util::status_dot(ui, ERROR_COLOR, 5.0);
                    }
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let running = self.state.count(Filter::Running);
            ui.label(format!("{} containers · {running} ativos", self.state.containers.len()));
            if let Some(n) = self.state.notifications.back() {
                ui.separator();
                let age = n.at.elapsed();
                let color = match n.kind {
                    NotificationKind::Error => ERROR_COLOR,
                    NotificationKind::Info => ui.visuals().text_color(),
                };
                let text = RichText::new(&n.text).color(color);
                // Erros ficam visíveis; informações somem após alguns segundos.
                if n.kind == NotificationKind::Error || age < Duration::from_secs(6) {
                    ui.label(text).on_hover_ui(|ui| {
                        for n in self.state.notifications.iter().rev().take(15) {
                            let c = if n.kind == NotificationKind::Error { ERROR_COLOR } else { ui.visuals().text_color() };
                            ui.label(RichText::new(&n.text).color(c));
                        }
                    });
                    if n.kind == NotificationKind::Info {
                        ui.ctx().request_repaint_after(Duration::from_secs(6).saturating_sub(age));
                    }
                }
            }
        });
    }

    fn remove_dialog(&mut self, ui: &mut egui::Ui) {
        let Some((id, mut force)) = self.confirm_remove.clone() else { return };
        let name = self.state.name_of(&id).to_string();
        let running = self.state.container(&id).is_some_and(|c| c.state.is_running());
        let mut close = false;
        let mut confirm = false;

        let modal = egui::Modal::new(egui::Id::unique("confirm_remove")).show(ui.ctx(), |ui| {
            ui.set_width(360.0);
            ui.heading("Remover container");
            ui.add_space(6.0);
            ui.label(format!("Remover \"{name}\"? Esta ação não pode ser desfeita."));
            if running {
                ui.add_space(4.0);
                ui.checkbox(&mut force, "Forçar (o container está rodando)");
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let can = !running || force;
                if ui.add_enabled(can, egui::Button::new(RichText::new("Remover").color(ERROR_COLOR))).clicked() {
                    confirm = true;
                }
                if ui.button("Cancelar").clicked() {
                    close = true;
                }
            });
        });

        if confirm {
            self.send(Command::Remove { id: id.clone(), force });
            self.confirm_remove = None;
        } else if close || modal.should_close() {
            self.confirm_remove = None;
        } else {
            self.confirm_remove = Some((id, force));
        }
    }
}

impl eframe::App for DockerViewApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump_events();
        #[cfg(feature = "screenshot")]
        crate::screenshot::Screenshot::tick(self, &ui.ctx().clone());

        egui::Panel::top("top_bar").show(ui, |ui| {
            ui.add_space(4.0);
            self.top_bar(ui);
            ui.add_space(2.0);
        });
        egui::Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui));

        if self.selected.is_some() {
            let default = (ui.available_width() * 0.45).clamp(360.0, 720.0);
            egui::Panel::right("details")
                .resizable(true)
                .default_size(default)
                .size_range(360.0..=1100.0)
                .show(ui, |ui| self.details_panel(ui));
        }

        egui::CentralPanel::default().show(ui, |ui| self.containers_table(ui));

        self.remove_dialog(ui);
    }
}
