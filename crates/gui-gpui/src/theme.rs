//! Paleta própria do dockerview aplicada sobre o tema do gpui-component.
//! Duas variantes: "Midnight" (escuro) e "Paper" (claro), com acento índigo.

use dockerview_core::ContainerState;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, px, rgb};

pub const UI_FONT: &str = "Geist";
pub const MONO_FONT: &str = "Geist Mono";

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Cores semânticas usadas pelas views (além das do tema base).
#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Hsla,
    pub sidebar: Hsla,
    pub surface: Hsla,
    pub surface_hover: Hsla,
    pub surface_selected: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_faint: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub accent_text: Hsla,
    pub green: Hsla,
    pub amber: Hsla,
    pub red: Hsla,
    pub blue: Hsla,
    pub gray: Hsla,
    pub cyan: Hsla,
    pub pink: Hsla,
    pub log_bg: Hsla,
    pub log_text: Hsla,
    pub log_err: Hsla,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: c(0x0b0d14),
            sidebar: c(0x0f121b),
            surface: c(0x141824),
            surface_hover: c(0x1a1f2e),
            surface_selected: c(0x1e2236),
            border: c(0x222839),
            border_strong: c(0x2e3549),
            text: c(0xe6e9f2),
            text_muted: c(0x9aa3b8),
            text_faint: c(0x5f6880),
            accent: c(0x7c7cff),
            accent_soft: c(0x7c7cff).alpha(0.16),
            accent_text: c(0xa5a6ff),
            green: c(0x34d399),
            amber: c(0xfbbf24),
            red: c(0xf87171),
            blue: c(0x60a5fa),
            gray: c(0x6b7280),
            cyan: c(0x22d3ee),
            pink: c(0xf472b6),
            log_bg: c(0x080a10),
            log_text: c(0xc9d1e3),
            log_err: c(0xff8a8a),
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            bg: c(0xf6f7fb),
            sidebar: c(0xeef0f7),
            surface: c(0xffffff),
            surface_hover: c(0xf3f4fa),
            surface_selected: c(0xeef0ff),
            border: c(0xe3e6ef),
            border_strong: c(0xd2d6e4),
            text: c(0x161a26),
            text_muted: c(0x5b6478),
            text_faint: c(0x9aa1b2),
            accent: c(0x5b5bf0),
            accent_soft: c(0x5b5bf0).alpha(0.10),
            accent_text: c(0x4545d6),
            green: c(0x059669),
            amber: c(0xd97706),
            red: c(0xdc2626),
            blue: c(0x2563eb),
            gray: c(0x8b93a5),
            cyan: c(0x0891b2),
            pink: c(0xdb2777),
            log_bg: c(0x10131c),
            log_text: c(0xd5dbe8),
            log_err: c(0xff8a8a),
        }
    }

    pub fn get(cx: &App) -> Self {
        if Theme::global(cx).is_dark() { Self::dark() } else { Self::light() }
    }

    pub fn state(&self, state: ContainerState) -> Hsla {
        match state {
            ContainerState::Running => self.green,
            ContainerState::Paused => self.amber,
            ContainerState::Restarting | ContainerState::Created => self.blue,
            ContainerState::Removing | ContainerState::Dead => self.red,
            ContainerState::Exited | ContainerState::Unknown => self.gray,
        }
    }
}

/// Modo inicial: variável DOCKERVIEW_THEME=light|dark, senão escuro.
pub fn preferred_mode() -> ThemeMode {
    match std::env::var("DOCKERVIEW_THEME").as_deref() {
        Ok("light") => ThemeMode::Light,
        _ => ThemeMode::Dark,
    }
}

/// Troca o modo e reaplica a paleta e as fontes (Theme::change recarrega as cores padrão).
pub fn apply(mode: ThemeMode, cx: &mut App) {
    Theme::change(mode, None, cx);
    let p = if mode.is_dark() { Palette::dark() } else { Palette::light() };
    Theme::update(cx, |t| {
        t.font_family = UI_FONT.into();
        t.mono_font_family = MONO_FONT.into();
        t.font_size = px(14.);
        t.mono_font_size = px(12.5);
        t.radius = px(8.);
        t.radius_lg = px(12.);

        t.background = p.bg;
        t.foreground = p.text;
        t.border = p.border;
        t.muted = p.surface_hover;
        t.muted_foreground = p.text_muted;
        t.primary = p.accent;
        t.primary_hover = p.accent.opacity(0.9);
        t.primary_active = p.accent.opacity(0.8);
        t.primary_foreground = c(0xffffff);
        t.secondary = p.surface_hover;
        t.secondary_hover = p.surface_selected;
        t.secondary_foreground = p.text;
        t.accent = p.surface_hover;
        t.accent_foreground = p.text;
        t.input = p.border_strong;
        t.ring = p.accent;
        t.popover = p.surface;
        t.popover_foreground = p.text;
        t.title_bar = p.sidebar;
        t.title_bar_border = p.border;
        t.selection = p.accent.alpha(0.35);
        t.chart_1 = p.accent;
        t.chart_2 = p.green;
        t.chart_3 = p.cyan;
        t.chart_grid = p.border;
        t.danger = p.red;
        t.success = p.green;
        t.warning = p.amber;
        t.info = p.blue;
    });
}

pub fn toggle(cx: &mut App) {
    let mode = if Theme::global(cx).is_dark() { ThemeMode::Light } else { ThemeMode::Dark };
    apply(mode, cx);
}
