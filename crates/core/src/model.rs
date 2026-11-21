//! Tipos próprios do dockerview, independentes do bollard e de qualquer UI.

use std::fmt;

pub type ContainerId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerState {
    Created,
    Running,
    Paused,
    Restarting,
    Exited,
    Removing,
    Dead,
    Unknown,
}

impl ContainerState {
    pub fn is_running(self) -> bool {
        matches!(self, ContainerState::Running | ContainerState::Restarting)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ContainerState::Created => "created",
            ContainerState::Running => "running",
            ContainerState::Paused => "paused",
            ContainerState::Restarting => "restarting",
            ContainerState::Exited => "exited",
            ContainerState::Removing => "removing",
            ContainerState::Dead => "dead",
            ContainerState::Unknown => "unknown",
        }
    }
}

impl fmt::Display for ContainerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortMapping {
    pub ip: Option<String>,
    pub private_port: u16,
    pub public_port: Option<u16>,
    pub protocol: String,
}

impl fmt::Display for PortMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.public_port {
            Some(public) => {
                let ip = self.ip.as_deref().unwrap_or("0.0.0.0");
                write!(f, "{ip}:{public}->{}/{}", self.private_port, self.protocol)
            }
            None => write!(f, "{}/{}", self.private_port, self.protocol),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContainerSummary {
    pub id: ContainerId,
    pub name: String,
    pub image: String,
    pub command: String,
    pub state: ContainerState,
    /// Texto de status do Docker, ex.: "Up 3 hours".
    pub status: String,
    /// Timestamp Unix de criação.
    pub created: i64,
    pub ports: Vec<PortMapping>,
    /// Projeto do Docker Compose, se houver.
    pub compose_project: Option<String>,
}

impl ContainerSummary {
    pub fn short_id(&self) -> &str {
        &self.id[..self.id.len().min(12)]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
    Console,
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub stream: LogStream,
    pub text: String,
}

/// Uma amostra de estatísticas já calculada.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StatsSample {
    /// CPU no padrão do `docker stats`: 100% = um núcleo inteiro (pode passar de 100%).
    pub cpu_percent: f64,
    /// Núcleos disponíveis para o container (normalmente os do host).
    pub online_cpus: u32,
    pub mem_usage: u64,
    pub mem_limit: u64,
    pub mem_percent: f64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub pids: u64,
}

impl StatsSample {
    /// CPU como fração da capacidade total do host: 0–100%.
    pub fn cpu_host_percent(&self) -> f64 {
        self.cpu_percent / f64::from(self.online_cpus.max(1))
    }

    /// Núcleos em uso (ex.: 1.18 = pouco mais de um núcleo).
    pub fn cores_used(&self) -> f64 {
        self.cpu_percent / 100.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Kill,
    Remove,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Restart => "restart",
            Action::Pause => "pause",
            Action::Unpause => "unpause",
            Action::Kill => "kill",
            Action::Remove => "remove",
        })
    }
}

/// Formata bytes em unidades binárias (KiB, MiB...).
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn cpu_normalized_to_host() {
        let s = StatsSample { cpu_percent: 118.0, online_cpus: 16, ..Default::default() };
        assert!((s.cpu_host_percent() - 7.375).abs() < 1e-9);
        assert!((s.cores_used() - 1.18).abs() < 1e-9);
        // Sem informação de núcleos, assume 1 (não divide por zero).
        assert_eq!(StatsSample { cpu_percent: 50.0, ..Default::default() }.cpu_host_percent(), 50.0);
    }

    #[test]
    fn port_display() {
        let p = PortMapping { ip: None, private_port: 80, public_port: Some(8080), protocol: "tcp".into() };
        assert_eq!(p.to_string(), "0.0.0.0:8080->80/tcp");
    }
}
