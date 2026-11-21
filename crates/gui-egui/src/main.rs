//! dockerview — interface egui.

mod app;
mod details;
#[cfg(feature = "screenshot")]
mod screenshot;
mod table;
mod util;

fn main() -> eframe::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,winit=warn,eframe=warn".into()),
        )
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("dockerview")
            .with_app_id("dockerview")
            .with_inner_size([1280.0, 780.0])
            .with_min_inner_size([800.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "dockerview",
        options,
        Box::new(|cc| {
            // O core chama request_repaint a cada evento: a UI só redesenha quando algo muda.
            let ctx = cc.egui_ctx.clone();
            let core = dockerview_core::spawn_core(Some(std::sync::Arc::new(move || ctx.request_repaint())));
            Ok(Box::new(app::DockerViewApp::new(core)))
        }),
    )
}
