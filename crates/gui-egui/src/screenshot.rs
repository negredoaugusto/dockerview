//! Captura automática da janela para depuração (feature `screenshot`).
//!
//! DOCKERVIEW_SCREENSHOT=/tmp/x.png  arquivo de saída
//! DOCKERVIEW_SELECT=nome            container a selecionar (opcional)
//! DOCKERVIEW_TAB=info|logs|stats|inspect
//! DOCKERVIEW_DELAY=5                segundos até capturar

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::app::DockerViewApp;
use crate::details::DetailsTab;

pub struct Screenshot {
    path: String,
    select: Option<String>,
    tab: Option<DetailsTab>,
    delay: Duration,
    start: Instant,
    requested: bool,
    done: Arc<AtomicBool>,
}

impl Screenshot {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var("DOCKERVIEW_SCREENSHOT").ok()?;
        let tab = std::env::var("DOCKERVIEW_TAB").ok().and_then(|t| match t.as_str() {
            "info" => Some(DetailsTab::Info),
            "logs" => Some(DetailsTab::Logs),
            "stats" => Some(DetailsTab::Stats),
            "inspect" => Some(DetailsTab::Inspect),
            _ => None,
        });
        let delay = std::env::var("DOCKERVIEW_DELAY").ok().and_then(|d| d.parse().ok()).unwrap_or(5);
        Some(Self {
            path,
            select: std::env::var("DOCKERVIEW_SELECT").ok(),
            tab,
            delay: Duration::from_secs(delay),
            start: Instant::now(),
            requested: false,
            done: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn tick(app: &mut DockerViewApp, ctx: &egui::Context) {
        let Some(shot) = app.screenshot.as_mut() else { return };
        if let Some(name) = shot.select.clone()
            && let Some(id) = app.state.containers.iter().find(|c| c.name == name).map(|c| c.id.clone())
        {
            shot.select = None;
            if let Some(tab) = shot.tab {
                app.tab = tab;
            }
            app.select(Some(id));
            return;
        }
        if shot.done.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !shot.requested && shot.start.elapsed() >= shot.delay {
            shot.requested = true;
            let (path, done, ctx2) = (shot.path.clone(), shot.done.clone(), ctx.clone());
            ctx.request_screenshot(move |img| {
                if let Err(e) = save_png(&path, &img) {
                    eprintln!("falha ao salvar screenshot: {e}");
                }
                done.store(true, Ordering::SeqCst);
                ctx2.request_repaint();
            });
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

fn save_png(path: &str, img: &egui::ColorImage) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header()?;
    let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    w.write_image_data(&raw)?;
    Ok(())
}
