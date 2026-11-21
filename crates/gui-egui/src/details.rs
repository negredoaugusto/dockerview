use dockerview_core::{Action, Command, ContainerState, ContainerSummary, LogStream, format_bytes};
use egui::{Color32, RichText, TextStyle, TextWrapMode};
use egui_plot::{Legend, Line, Plot, PlotPoints};

use crate::app::DockerViewApp;
use crate::util::{ERROR_COLOR, relative_time, state_color};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailsTab {
    Info,
    Logs,
    Stats,
    Inspect,
}

pub struct LogView {
    pub filter: String,
    pub stdout: bool,
    pub stderr: bool,
    pub follow: bool,
    pub wrap: bool,
}

impl Default for LogView {
    fn default() -> Self {
        Self { filter: String::new(), stdout: true, stderr: true, follow: true, wrap: false }
    }
}

impl LogView {
    pub fn reset_scroll(&mut self) {
        self.follow = true;
    }
}

impl DockerViewApp {
    pub fn details_panel(&mut self, ui: &mut egui::Ui) {
        let Some(c) = self.selected.as_ref().and_then(|id| self.state.container(id)).cloned() else {
            return;
        };

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            crate::util::status_dot(ui, state_color(c.state), 6.0);
            ui.heading(RichText::new(&c.name).strong());
            ui.label(RichText::new(c.state.as_str()).color(state_color(c.state)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✖").on_hover_text("Fechar").clicked() {
                    self.select(None);
                }
            });
        });
        ui.horizontal(|ui| self.detail_actions(ui, &c));
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            for (tab, label) in [
                (DetailsTab::Info, "Info"),
                (DetailsTab::Logs, "Logs"),
                (DetailsTab::Stats, "Stats"),
                (DetailsTab::Inspect, "Inspect"),
            ] {
                ui.selectable_value(&mut self.tab, tab, label);
            }
        });
        ui.separator();

        match self.tab {
            DetailsTab::Info => info_tab(ui, &c),
            DetailsTab::Logs => self.logs_tab(ui, &c),
            DetailsTab::Stats => self.stats_tab(ui, &c),
            DetailsTab::Inspect => self.inspect_tab(ui, &c),
        }
    }

    fn detail_actions(&mut self, ui: &mut egui::Ui, c: &ContainerSummary) {
        let running = c.state.is_running();
        let paused = c.state == ContainerState::Paused;
        let act = |ui: &mut egui::Ui, enabled: bool, label: &str, a: Action| {
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                self.perform(&c.id, a);
            }
        };
        act(ui, !running && !paused, "⏵ Iniciar", Action::Start);
        act(ui, running || paused, "⏹ Parar", Action::Stop);
        act(ui, running || paused, "⟳ Reiniciar", Action::Restart);
        if paused {
            act(ui, true, "⏵ Retomar", Action::Unpause);
        } else {
            act(ui, running, "⏸ Pausar", Action::Pause);
        }
        act(ui, running || paused, "☠ Kill", Action::Kill);
        if ui.button(RichText::new("🗑 Remover").color(ERROR_COLOR)).clicked() {
            self.confirm_remove = Some((c.id.clone(), false));
        }
    }

    fn logs_tab(&mut self, ui: &mut egui::Ui, c: &ContainerSummary) {
        let view = &mut self.log_view;
        let mut resubscribe = false;
        let mut clear = false;

        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut view.filter).hint_text("Filtrar…").desired_width(160.0));
            ui.toggle_value(&mut view.stdout, "stdout");
            ui.toggle_value(&mut view.stderr, "stderr");
            ui.toggle_value(&mut view.follow, "Seguir").on_hover_text("Rolar automaticamente");
            ui.toggle_value(&mut view.wrap, "Quebrar linhas");
            if ui.small_button("🗋").on_hover_text("Limpar").clicked() {
                clear = true;
            }
            if ui.small_button("⟳").on_hover_text("Recarregar logs").clicked() {
                resubscribe = true;
            }
        });

        let buf = self.state.logs.get(&c.id);
        let filter = view.filter.to_lowercase();
        let lines: Vec<_> = buf
            .map(|b| {
                b.lines()
                    .iter()
                    .filter(|l| match l.stream {
                        LogStream::Stderr => view.stderr,
                        _ => view.stdout,
                    })
                    .filter(|l| filter.is_empty() || l.text.to_lowercase().contains(&filter))
                    .collect()
            })
            .unwrap_or_default();

        ui.horizontal(|ui| {
            let total = buf.map_or(0, |b| b.len());
            ui.label(RichText::new(format!("{} de {total} linhas", lines.len())).small().weak());
            if let Some(b) = buf {
                if b.dropped > 0 {
                    ui.label(RichText::new(format!("({} antigas descartadas)", b.dropped)).small().weak());
                }
                if b.ended {
                    ui.label(RichText::new("· stream encerrado").small().color(Color32::from_rgb(230, 180, 60)));
                }
            }
        });

        let row_height = ui.text_style_height(&TextStyle::Monospace);
        let wrap = view.wrap;
        let mut scroll = if wrap { egui::ScrollArea::vertical() } else { egui::ScrollArea::both() };
        scroll = scroll.id_salt("logs").auto_shrink(false).stick_to_bottom(view.follow);

        egui::Frame::new().fill(ui.visuals().extreme_bg_color).inner_margin(6.0).corner_radius(4.0).show(ui, |ui| {
            if lines.is_empty() {
                ui.label(RichText::new("Sem logs.").weak());
                ui.allocate_space(ui.available_size());
                return;
            }
            let draw = |ui: &mut egui::Ui, line: &dockerview_core::LogLine| {
                let mut text = RichText::new(&line.text).monospace();
                if line.stream == LogStream::Stderr {
                    text = text.color(ERROR_COLOR);
                }
                let mode = if wrap { TextWrapMode::Wrap } else { TextWrapMode::Extend };
                ui.add(egui::Label::new(text).wrap_mode(mode));
            };
            if wrap {
                // Altura variável: sem virtualização.
                scroll.show(ui, |ui| lines.iter().for_each(|l| draw(ui, l)));
            } else {
                // Linhas de altura fixa: desenha só as visíveis.
                scroll.show_rows(ui, row_height, lines.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for l in &lines[range] {
                        draw(ui, l);
                    }
                });
            }
        });

        if clear && let Some(b) = self.state.logs.get_mut(&c.id) {
            b.clear();
        }
        if resubscribe {
            self.subscribe_logs(c.id.clone());
        }
    }

    fn stats_tab(&mut self, ui: &mut egui::Ui, c: &ContainerSummary) {
        let Some(hist) = self.state.stats.get(&c.id).filter(|_| c.state.is_running()) else {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                let msg = if c.state.is_running() { "Aguardando estatísticas…" } else { "Container não está rodando." };
                ui.label(RichText::new(msg).weak());
            });
            return;
        };
        let Some(s) = hist.latest().copied() else { return };

        egui::Grid::new("stats_grid").num_columns(4).spacing([24.0, 6.0]).show(ui, |ui| {
            metric(ui, "CPU do host", format!("{:.2}% ({:.2} de {} núcleos)", s.cpu_host_percent(), s.cores_used(), s.online_cpus));
            metric(ui, "Memória", format!("{} / {} ({:.1}%)", format_bytes(s.mem_usage), format_bytes(s.mem_limit), s.mem_percent));
            ui.end_row();
            metric(ui, "Rede RX", format_bytes(s.net_rx));
            metric(ui, "Rede TX", format_bytes(s.net_tx));
            ui.end_row();
            metric(ui, "Processos", s.pids.to_string());
            ui.end_row();
        });
        ui.add_space(8.0);

        let n = hist.samples().len() as f64;
        let points = |f: fn(&dockerview_core::StatsSample) -> f64| -> PlotPoints<'static> {
            hist.samples().iter().enumerate().map(|(i, s)| [i as f64 - n + 1.0, f(s)]).collect()
        };
        let x_range = -(hist.capacity() as f64)..=0.0;
        let height = ((ui.available_height() - 40.0) / 2.0).max(120.0);

        ui.label(RichText::new("CPU (% do host)").strong());
        Plot::new("cpu_plot")
            .height(height)
            .include_x(*x_range.start())
            .include_x(0.0)
            .include_y(0.0)
            .include_y(1.0)
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .x_axis_formatter(|m, _| format!("{}s", m.value))
            .y_axis_formatter(|m, _| format!("{:.0}%", m.value))
            .show(ui, |p| {
                p.line(Line::new("CPU", points(|s| s.cpu_host_percent())).color(Color32::from_rgb(90, 160, 230)).fill(0.0));
            });

        ui.label(RichText::new("Memória (MiB)").strong());
        Plot::new("mem_plot")
            .height(height)
            .include_x(*x_range.start())
            .include_x(0.0)
            .include_y(0.0)
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .legend(Legend::default())
            .x_axis_formatter(|m, _| format!("{}s", m.value))
            .show(ui, |p| {
                p.line(
                    Line::new("Uso", points(|s| s.mem_usage as f64 / 1_048_576.0))
                        .color(Color32::from_rgb(80, 200, 120))
                        .fill(0.0),
                );
            });
    }

    fn inspect_tab(&mut self, ui: &mut egui::Ui, c: &ContainerSummary) {
        let mut refresh = false;
        ui.horizontal(|ui| {
            if ui.button("⟳ Atualizar").clicked() {
                refresh = true;
            }
            if let Some(json) = self.state.inspect.get(&c.id)
                && ui.button("🗐 Copiar").clicked()
            {
                ui.ctx().copy_text(json.clone());
            }
        });

        if refresh || (!self.state.inspect.contains_key(&c.id) && !self.inspect_requested.contains(&c.id)) {
            self.inspect_requested.insert(c.id.clone());
            self.send(Command::Inspect(c.id.clone()));
        }

        match self.state.inspect.get(&c.id) {
            Some(json) => {
                egui::ScrollArea::both().id_salt("inspect").auto_shrink(false).show(ui, |ui| {
                    // Label selecionável, sem quebra de linha (rola na horizontal).
                    ui.add(egui::Label::new(RichText::new(json).monospace()).wrap_mode(TextWrapMode::Extend).selectable(true));
                });
            }
            None => {
                ui.spinner();
            }
        }
    }
}

fn metric(ui: &mut egui::Ui, label: &str, value: String) {
    ui.label(RichText::new(label).weak());
    ui.label(RichText::new(value).monospace().strong());
}

fn info_tab(ui: &mut egui::Ui, c: &ContainerSummary) {
    egui::Grid::new("info_grid").num_columns(2).spacing([16.0, 8.0]).striped(true).show(ui, |ui| {
        let mut row = |k: &str, v: String| {
            ui.label(RichText::new(k).weak());
            ui.add(egui::Label::new(RichText::new(v).monospace()).wrap_mode(TextWrapMode::Wrap));
            ui.end_row();
        };
        row("ID", c.id.clone());
        row("Imagem", c.image.clone());
        row("Comando", c.command.clone());
        row("Estado", c.state.to_string());
        row("Status", c.status.clone());
        row("Criado", relative_time(c.created));
        let ports: Vec<String> = c.ports.iter().map(ToString::to_string).collect();
        row("Portas", if ports.is_empty() { "—".into() } else { ports.join("\n") });
        if let Some(p) = &c.compose_project {
            row("Compose", p.clone());
        }
    });
}
