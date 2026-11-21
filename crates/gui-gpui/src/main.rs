//! dockerview — interface GPUI (gpui-kit).

mod app;
mod assets;
mod details;
mod list;
mod theme;
mod widgets;

use std::borrow::Cow;

use gpui_kit::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,gpui=warn,blade=warn,naga=warn".into()),
        )
        .init();

    gpui_kit::application().with_assets(assets::AppAssets).run(|cx: &mut App| {
        gpui_kit::init(cx);
        load_fonts(cx);
        theme::apply(theme::preferred_mode(), cx);

        let bounds = Bounds::centered(None, size(px(1320.), px(820.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui_kit::component::TitleBar::title_bar_options()),
            window_min_size: Some(size(px(860.), px(520.))),
            app_id: Some("dockerview-gpui".into()),
            ..Default::default()
        };

        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| app::DockerApp::new(window, cx)))
            .expect("falha ao abrir janela");
        cx.activate(true);
    });
}

/// Fontes embutidas no binário: Geist (UI) e Geist Mono (logs/JSON).
fn load_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-Bold.ttf")),
    ];
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        tracing::warn!("falha ao carregar fontes embutidas: {e}");
    }
}
