use std::time::{SystemTime, UNIX_EPOCH};

use dockerview_core::ContainerState;
use egui::Color32;

pub fn state_color(state: ContainerState) -> Color32 {
    match state {
        ContainerState::Running => Color32::from_rgb(80, 200, 120),
        ContainerState::Paused => Color32::from_rgb(230, 180, 60),
        ContainerState::Restarting | ContainerState::Created => Color32::from_rgb(90, 160, 230),
        ContainerState::Removing | ContainerState::Dead => Color32::from_rgb(230, 80, 80),
        ContainerState::Exited | ContainerState::Unknown => Color32::from_gray(130),
    }
}

pub const ERROR_COLOR: Color32 = Color32::from_rgb(235, 100, 100);

/// "há 3 horas" a partir de um timestamp Unix.
pub fn relative_time(unix: i64) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let secs = (now - unix).max(0);
    let (n, unit) = match secs {
        s if s < 60 => return "agora".into(),
        s if s < 3600 => (s / 60, "min"),
        s if s < 86_400 => (s / 3600, "h"),
        s if s < 86_400 * 30 => (s / 86_400, "d"),
        s if s < 86_400 * 365 => (s / (86_400 * 30), "meses"),
        s => (s / (86_400 * 365), "anos"),
    };
    format!("há {n} {unit}")
}

/// Círculo colorido indicando estado (não depende de glifos da fonte).
pub fn status_dot(ui: &mut egui::Ui, color: Color32, radius: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(radius * 2.0 + 2.0, radius * 2.0 + 2.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), radius, color);
    resp
}
