//! Núcleo do dockerview: acesso ao Docker independente de interface gráfica.
//!
//! - [`DockerClient`]: API assíncrona direta (usada pela CLI e pelo serviço).
//! - [`spawn_core`]: serviço em thread própria com canais [`Command`] / [`Event`],
//!   pensado para as UIs (egui, GPUI).

pub mod client;
pub mod error;
pub mod model;
pub mod service;
pub mod state;
pub mod stats;

pub use client::{ContainerEvent, DockerClient};
pub use error::{CoreError, Result};
pub use model::*;
pub use state::{AppState, Connection, Filter, LogBuffer, Notification, NotificationKind, StatsHistory, Totals};
pub use service::{Command, CoreHandle, Event, Notifier, spawn_core};
