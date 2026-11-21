//! Wrapper assíncrono sobre o bollard, convertendo tudo para os tipos de [`crate::model`].

use bollard::Docker;
use bollard::models::{ContainerSummaryStateEnum, EventMessageTypeEnum, PortSummaryTypeEnum};
use bollard::query_parameters::{
    EventsOptions, KillContainerOptions, ListContainersOptions, LogsOptions, RemoveContainerOptions,
    RestartContainerOptions, StartContainerOptions, StatsOptions, StopContainerOptions,
};
use futures_util::{Stream, StreamExt};
use std::collections::HashMap;

use crate::error::{CoreError, Result};
use crate::model::{Action, ContainerState, ContainerSummary, LogLine, LogStream, PortMapping, StatsSample};

/// Evento relevante vindo de `/events` do daemon.
#[derive(Debug, Clone)]
pub struct ContainerEvent {
    pub id: String,
    pub action: String,
}

#[derive(Clone)]
pub struct DockerClient {
    docker: Docker,
}

impl DockerClient {
    /// Conecta usando `DOCKER_HOST` ou o socket local padrão, e valida com um ping.
    pub async fn connect() -> Result<Self> {
        let docker = Docker::connect_with_defaults().map_err(CoreError::from_connect)?;
        docker.ping().await.map_err(CoreError::from_connect)?;
        Ok(Self { docker })
    }

    /// Versão do daemon, ex.: "29.0.1 (API 1.52)".
    pub async fn version(&self) -> Result<String> {
        let v = self.docker.version().await?;
        Ok(format!(
            "{} (API {})",
            v.version.as_deref().unwrap_or("?"),
            v.api_version.as_deref().unwrap_or("?")
        ))
    }

    pub async fn list_containers(&self, all: bool) -> Result<Vec<ContainerSummary>> {
        let opts = ListContainersOptions { all, ..Default::default() };
        let raw = self.docker.list_containers(Some(opts)).await?;
        let mut list: Vec<_> = raw.into_iter().map(convert_summary).collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(list)
    }

    pub async fn perform(&self, id: &str, action: Action) -> Result<()> {
        let d = &self.docker;
        match action {
            Action::Start => d.start_container(id, None::<StartContainerOptions>).await?,
            Action::Stop => d.stop_container(id, None::<StopContainerOptions>).await?,
            Action::Restart => d.restart_container(id, None::<RestartContainerOptions>).await?,
            Action::Pause => d.pause_container(id).await?,
            Action::Unpause => d.unpause_container(id).await?,
            Action::Kill => d.kill_container(id, None::<KillContainerOptions>).await?,
            Action::Remove => d.remove_container(id, None::<RemoveContainerOptions>).await?,
        }
        Ok(())
    }

    pub async fn remove(&self, id: &str, force: bool) -> Result<()> {
        let opts = RemoveContainerOptions { force, ..Default::default() };
        self.docker.remove_container(id, Some(opts)).await?;
        Ok(())
    }

    /// `docker inspect` formatado como JSON.
    pub async fn inspect_json(&self, id: &str) -> Result<String> {
        let details = self.docker.inspect_container(id, None).await?;
        Ok(serde_json::to_string_pretty(&details)?)
    }

    /// Stream de logs. `tail` = número de linhas anteriores (ou "all").
    pub fn logs(&self, id: &str, follow: bool, tail: &str) -> impl Stream<Item = Result<LogLine>> + Send + 'static {
        let opts = LogsOptions { follow, stdout: true, stderr: true, tail: tail.to_string(), ..Default::default() };
        self.docker.logs(id, Some(opts)).map(|item| {
            let out = item?;
            let (stream, bytes) = match out {
                bollard::container::LogOutput::StdOut { message } => (LogStream::Stdout, message),
                bollard::container::LogOutput::StdErr { message } => (LogStream::Stderr, message),
                bollard::container::LogOutput::Console { message }
                | bollard::container::LogOutput::StdIn { message } => (LogStream::Console, message),
            };
            let text = String::from_utf8_lossy(&bytes).trim_end_matches(['\n', '\r']).to_string();
            Ok(LogLine { stream, text: truncate_line(text) })
        })
    }

    /// Stream de estatísticas já calculadas (~1 amostra/segundo).
    pub fn stats(&self, id: &str) -> impl Stream<Item = Result<StatsSample>> + Send + 'static {
        let opts = StatsOptions { stream: true, one_shot: false };
        self.docker
            .stats(id, Some(opts))
            .map(|item| Ok(crate::stats::compute(&item?)))
    }

    /// Uma única amostra de estatísticas (útil para a tabela).
    pub async fn stats_once(&self, id: &str) -> Result<StatsSample> {
        // `one_shot` não preenche precpu_stats, então pegamos a 2ª amostra do stream.
        let mut s = Box::pin(self.stats(id)).skip(1);
        match s.next().await {
            Some(r) => r,
            None => Ok(StatsSample::default()),
        }
    }

    /// Stream de eventos de containers (start, stop, die, destroy...).
    pub fn container_events(&self) -> impl Stream<Item = Result<ContainerEvent>> + Send + 'static {
        let filters = HashMap::from([("type".to_string(), vec!["container".to_string()])]);
        let opts = EventsOptions { filters: Some(filters), ..Default::default() };
        self.docker.events(Some(opts)).filter_map(|item| async move {
            match item {
                Ok(ev) if ev.typ == Some(EventMessageTypeEnum::CONTAINER) => Some(Ok(ContainerEvent {
                    id: ev.actor.and_then(|a| a.id).unwrap_or_default(),
                    action: ev.action.unwrap_or_default(),
                })),
                Ok(_) => None,
                Err(e) => Some(Err(e.into())),
            }
        })
    }
}

/// Tamanho máximo de uma linha de log guardada. Um container pode emitir linhas
/// gigantes; sem limite, 5000 linhas no buffer poderiam ocupar gigabytes.
pub const MAX_LOG_LINE_BYTES: usize = 16 * 1024;

fn truncate_line(mut text: String) -> String {
    if text.len() > MAX_LOG_LINE_BYTES {
        let mut end = MAX_LOG_LINE_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let extra = text.len() - end;
        text.truncate(end);
        text.push_str(&format!(" … [+{extra} bytes truncados]"));
    }
    text
}

fn convert_summary(c: bollard::models::ContainerSummary) -> ContainerSummary {
    let name = c
        .names
        .as_ref()
        .and_then(|n| n.first())
        .map(|n| n.trim_start_matches('/').to_string())
        .unwrap_or_default();

    let state = match c.state {
        Some(ContainerSummaryStateEnum::CREATED) => ContainerState::Created,
        Some(ContainerSummaryStateEnum::RUNNING) => ContainerState::Running,
        Some(ContainerSummaryStateEnum::PAUSED) => ContainerState::Paused,
        Some(ContainerSummaryStateEnum::RESTARTING) => ContainerState::Restarting,
        Some(ContainerSummaryStateEnum::EXITED) => ContainerState::Exited,
        Some(ContainerSummaryStateEnum::REMOVING) => ContainerState::Removing,
        Some(ContainerSummaryStateEnum::DEAD) => ContainerState::Dead,
        _ => ContainerState::Unknown,
    };

    let mut ports: Vec<PortMapping> = c
        .ports
        .unwrap_or_default()
        .into_iter()
        .map(|p| PortMapping {
            ip: p.ip.filter(|ip| !ip.is_empty()),
            private_port: p.private_port,
            public_port: p.public_port,
            protocol: match p.typ {
                Some(PortSummaryTypeEnum::UDP) => "udp".into(),
                Some(PortSummaryTypeEnum::SCTP) => "sctp".into(),
                _ => "tcp".into(),
            },
        })
        .collect();
    ports.sort_by_key(|p| (p.private_port, p.public_port));
    // Docker lista a mesma porta para IPv4 e IPv6; mantemos uma só.
    ports.dedup_by(|a, b| a.private_port == b.private_port && a.public_port == b.public_port && a.protocol == b.protocol);

    ContainerSummary {
        id: c.id.unwrap_or_default(),
        compose_project: c.labels.as_ref().and_then(|l| l.get("com.docker.compose.project").cloned()),
        name,
        image: c.image.unwrap_or_default(),
        command: c.command.unwrap_or_default(),
        state,
        status: c.status.unwrap_or_default(),
        created: c.created.unwrap_or_default(),
        ports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_lines_are_truncated_on_char_boundary() {
        let short = "ok".to_string();
        assert_eq!(truncate_line(short.clone()), short);

        let long = "é".repeat(MAX_LOG_LINE_BYTES); // 2 bytes cada
        let out = truncate_line(long);
        assert!(out.len() < MAX_LOG_LINE_BYTES + 64);
        assert!(out.ends_with("bytes truncados]"));
    }
}
