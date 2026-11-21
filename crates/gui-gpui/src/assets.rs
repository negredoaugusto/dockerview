//! Fonte de assets: ícones Lucide escolhidos + ícones padrão dos componentes.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

gpui_kit::assets::icon_assets!(
    pub AppIcons,
    [
        Activity, ArrowDown, ArrowDownToLine, ArrowUp, Box, Boxes, Braces, CircleAlert, CircleCheck,
        CirclePause, CircleStop, Clock, Container, Copy, Cpu, Ellipsis, Eraser, Gauge, Globe, Hash,
        Info, Layers, LayoutDashboard, ListFilter, LoaderCircle, MemoryStick, Monitor, Moon, Network,
        PanelRightClose, Pause, Play, Plug, Power, RefreshCw, RotateCw, ScrollText, Search, Server,
        Ship, Skull, Sparkles, Square, Sun, Tag, Terminal, TextWrap, Timer, Unplug, WifiOff, X, Zap,
        Trash,
    ]
);

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = AppIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(AppIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
