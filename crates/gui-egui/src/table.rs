use dockerview_core::{Action, ContainerState, ContainerSummary, StatsSample, format_bytes};
use egui::{RichText, Sense};
use egui_extras::{Column, TableBuilder};

use crate::app::DockerViewApp;
use crate::util::{relative_time, state_color, status_dot};

/// Ação disparada por um botão da linha (aplicada depois de desenhar a tabela).
enum RowAction {
    Perform(String, Action),
    Remove(String),
}

/// Colunas da tabela, em ordem de exibição.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Col {
    State,
    Name,
    Image,
    Status,
    Cpu,
    Memory,
    Ports,
    Actions,
}

impl Col {
    fn title(self) -> &'static str {
        match self {
            Col::State => "",
            Col::Name => "Nome",
            Col::Image => "Imagem",
            Col::Status => "Status",
            Col::Cpu => "CPU",
            Col::Memory => "Memória",
            Col::Ports => "Portas",
            Col::Actions => "Ações",
        }
    }

    /// Largura mínima usada para decidir se a coluna cabe.
    fn min_width(self) -> f32 {
        match self {
            Col::State => 18.0,
            Col::Name => 140.0,
            Col::Image => 120.0,
            Col::Status => 120.0,
            Col::Cpu => 60.0,
            Col::Memory => 80.0,
            Col::Ports => 110.0,
            Col::Actions => 140.0,
        }
    }

    fn column(self, is_last: bool) -> Column {
        if is_last {
            return Column::remainder().at_least(self.min_width());
        }
        match self {
            Col::State => Column::exact(18.0),
            Col::Name => Column::initial(200.0).at_least(100.0).clip(true),
            Col::Image => Column::initial(180.0).at_least(80.0).clip(true),
            Col::Status => Column::initial(150.0).at_least(80.0).clip(true),
            Col::Cpu => Column::initial(65.0).at_least(55.0),
            Col::Memory => Column::initial(90.0).at_least(70.0),
            Col::Ports => Column::initial(150.0).at_least(60.0).clip(true),
            Col::Actions => Column::initial(140.0).at_least(140.0),
        }
    }
}

/// Escolhe as colunas que cabem, por prioridade, mantendo a ordem de exibição.
fn visible_columns(width: f32) -> Vec<Col> {
    const ORDER: [Col; 8] = [Col::State, Col::Name, Col::Image, Col::Status, Col::Cpu, Col::Memory, Col::Ports, Col::Actions];
    const PRIORITY: [Col; 8] = [Col::State, Col::Name, Col::Actions, Col::Status, Col::Cpu, Col::Memory, Col::Image, Col::Ports];
    let mut chosen = Vec::new();
    let mut used = 0.0;
    for col in PRIORITY {
        let w = col.min_width() + 16.0; // + espaçamento
        if used + w <= width || chosen.len() < 2 {
            used += w;
            chosen.push(col);
        }
    }
    ORDER.into_iter().filter(|c| chosen.contains(c)).collect()
}

impl DockerViewApp {
    pub fn containers_table(&mut self, ui: &mut egui::Ui) {
        let rows: Vec<ContainerSummary> = self.state.filtered(self.filter, &self.search).into_iter().cloned().collect();

        if rows.is_empty() {
            ui.centered_and_justified(|ui| {
                let msg = if self.state.containers.is_empty() {
                    "Nenhum container encontrado."
                } else {
                    "Nenhum container corresponde ao filtro."
                };
                ui.label(RichText::new(msg).weak());
            });
            return;
        }

        let cols = visible_columns(ui.available_width());
        let compact = !cols.contains(&Col::Image);
        let mut clicked: Option<String> = None;
        let mut action: Option<RowAction> = None;
        // No modo compacto a imagem aparece numa 2ª linha abaixo do nome.
        let row_height = if compact { 40.0 } else { 28.0 };

        let mut table = TableBuilder::new(ui)
            .id_salt(compact) // larguras salvas separadas por layout
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        for (i, col) in cols.iter().enumerate() {
            table = table.column(col.column(i == cols.len() - 1));
        }

        table
            .header(24.0, |mut h| {
                for col in &cols {
                    h.col(|ui| {
                        ui.strong(col.title());
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, rows.len(), |mut row| {
                    let c = &rows[row.index()];
                    row.set_selected(self.selected.as_deref() == Some(c.id.as_str()));
                    let stats = self.state.stats.get(&c.id).and_then(|h| h.latest()).copied().filter(|_| c.state.is_running());

                    for col in &cols {
                        row.col(|ui| {
                            if let Some(a) = cell(ui, *col, c, stats, compact) {
                                action = Some(a);
                            }
                        });
                    }

                    if row.response().clicked() {
                        clicked = Some(c.id.clone());
                    }
                });
            });

        match action {
            Some(RowAction::Perform(id, a)) => self.perform(&id, a),
            Some(RowAction::Remove(id)) => self.confirm_remove = Some((id, false)),
            None => {
                if let Some(id) = clicked {
                    // Clicar no selecionado fecha o painel de detalhes.
                    let new = if self.selected.as_deref() == Some(id.as_str()) { None } else { Some(id) };
                    self.select(new);
                }
            }
        }
    }
}

fn cell(ui: &mut egui::Ui, col: Col, c: &ContainerSummary, stats: Option<StatsSample>, compact: bool) -> Option<RowAction> {
    let dash = |ui: &mut egui::Ui| {
        ui.label(RichText::new("—").weak());
    };
    match col {
        Col::State => {
            status_dot(ui, state_color(c.state), 5.0).on_hover_text(c.state.as_str());
        }
        Col::Name => {
            let hover = format!(
                "ID: {}\nImagem: {}\nCriado {}{}",
                c.short_id(),
                c.image,
                relative_time(c.created),
                c.compose_project.as_ref().map(|p| format!("\nCompose: {p}")).unwrap_or_default()
            );
            if compact {
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    ui.label(RichText::new(&c.name).strong());
                    ui.label(RichText::new(&c.image).small().weak());
                })
                .response
                .on_hover_text(hover);
            } else {
                ui.label(RichText::new(&c.name).strong()).on_hover_text(hover);
            }
        }
        Col::Image => {
            ui.label(&c.image);
        }
        Col::Status => {
            ui.label(RichText::new(&c.status).weak());
        }
        Col::Cpu => match stats {
            Some(s) => {
                ui.monospace(format!("{:.1}%", s.cpu_host_percent()))
                    .on_hover_text(format!("{:.2} de {} núcleos", s.cores_used(), s.online_cpus));
            }
            None => dash(ui),
        },
        Col::Memory => match stats {
            Some(s) => {
                ui.monospace(format_bytes(s.mem_usage));
            }
            None => dash(ui),
        },
        Col::Ports => {
            let published: Vec<String> =
                c.ports.iter().filter_map(|p| p.public_port.map(|pp| format!("{pp}→{}", p.private_port))).collect();
            let all: Vec<String> = c.ports.iter().map(ToString::to_string).collect();
            if !all.is_empty() {
                ui.label(published.join(", ")).on_hover_text(all.join("\n"));
            }
        }
        Col::Actions => return action_buttons(ui, c),
    }
    None
}

fn action_buttons(ui: &mut egui::Ui, c: &ContainerSummary) -> Option<RowAction> {
    // Slots fixos (iniciar/parar · pausar/retomar · reiniciar · remover): cada ícone
    // fica sempre na mesma coluna, independente do estado do container.
    type Slot = Option<(&'static str, &'static str, Option<Action>)>;
    let slots: [Slot; 4] = match c.state {
        ContainerState::Running | ContainerState::Restarting => [
            Some(("⏹", "Parar", Some(Action::Stop))),
            Some(("⏸", "Pausar", Some(Action::Pause))),
            Some(("⟳", "Reiniciar", Some(Action::Restart))),
            Some(("🗑", "Remover", None)),
        ],
        ContainerState::Paused => [
            Some(("⏹", "Parar", Some(Action::Stop))),
            Some(("⏵", "Retomar", Some(Action::Unpause))),
            None,
            Some(("🗑", "Remover", None)),
        ],
        _ => [Some(("⏵", "Iniciar", Some(Action::Start))), None, None, Some(("🗑", "Remover", None))],
    };

    let mut out = None;
    let slot_size = egui::vec2(24.0, 20.0);
    for slot in slots {
        match slot {
            None => {
                // Botão invisível do mesmo tamanho: ocupa exatamente o espaço de um real.
                ui.add_visible(false, |ui: &mut egui::Ui| ui.add_sized(slot_size, egui::Button::new("⏸").small()));
            }
            Some((icon, tip, action)) => {
                if ui.add_sized(slot_size, egui::Button::new(icon).small()).on_hover_text(tip).clicked() {
                    out = Some(match action {
                        Some(a) => RowAction::Perform(c.id.clone(), a),
                        None => RowAction::Remove(c.id.clone()),
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_adapt_to_width() {
        let narrow = visible_columns(420.0);
        assert!(narrow.contains(&Col::Name) && narrow.contains(&Col::Actions));
        assert!(!narrow.contains(&Col::Image));

        let wide = visible_columns(2000.0);
        assert_eq!(wide.len(), 8);
        assert!(wide.first() == Some(&Col::State) && wide.last() == Some(&Col::Actions));
    }
}
