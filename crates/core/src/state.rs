//! Estado de apresentação independente de UI. As interfaces (egui, GPUI) aplicam os
//! [`Event`]s do serviço aqui e apenas renderizam o resultado.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use crate::model::{Action, ContainerId, ContainerState, ContainerSummary, LogLine, StatsSample};
use crate::service::Event;

pub const DEFAULT_LOG_CAPACITY: usize = 5_000;
pub const DEFAULT_STATS_CAPACITY: usize = 120;
const MAX_NOTIFICATIONS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Connected { version: String },
    Disconnected { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Running,
    Stopped,
}

impl Filter {
    pub fn matches(self, state: ContainerState) -> bool {
        match self {
            Filter::All => true,
            Filter::Running => state.is_running() || state == ContainerState::Paused,
            Filter::Stopped => !(state.is_running() || state == ContainerState::Paused),
        }
    }
}

/// Buffer circular de logs.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    lines: VecDeque<LogLine>,
    capacity: usize,
    /// Linhas descartadas por exceder a capacidade.
    pub dropped: usize,
    /// O stream terminou (container parou ou erro).
    pub ended: bool,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self { lines: VecDeque::with_capacity(capacity.min(1024)), capacity, dropped: 0, ended: false }
    }

    pub fn push(&mut self, line: LogLine) {
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
            self.dropped += 1;
        }
        self.lines.push_back(line);
    }

    pub fn lines(&self) -> &VecDeque<LogLine> {
        &self.lines
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.dropped = 0;
    }
}

/// Histórico de amostras de estatísticas para gráficos.
#[derive(Debug, Clone)]
pub struct StatsHistory {
    samples: VecDeque<StatsSample>,
    capacity: usize,
}

impl StatsHistory {
    pub fn new(capacity: usize) -> Self {
        Self { samples: VecDeque::with_capacity(capacity), capacity }
    }

    pub fn push(&mut self, s: StatsSample) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(s);
    }

    pub fn latest(&self) -> Option<&StatsSample> {
        self.samples.back()
    }

    pub fn samples(&self) -> &VecDeque<StatsSample> {
        &self.samples
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Uso agregado dos containers em execução.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Totals {
    /// Fração da CPU total do host (0–100%).
    pub cpu_host_percent: f64,
    pub cores_used: f64,
    pub host_cores: u32,
    pub mem_usage: u64,
    pub mem_limit: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Info,
    Error,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub kind: NotificationKind,
    pub text: String,
    pub at: Instant,
}

#[derive(Debug)]
pub struct AppState {
    pub connection: Connection,
    pub containers: Vec<ContainerSummary>,
    pub logs: HashMap<ContainerId, LogBuffer>,
    pub stats: HashMap<ContainerId, StatsHistory>,
    pub inspect: HashMap<ContainerId, String>,
    pub notifications: VecDeque<Notification>,
    pub log_capacity: usize,
    pub stats_capacity: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            connection: Connection::Connecting,
            containers: Vec::new(),
            logs: HashMap::new(),
            stats: HashMap::new(),
            inspect: HashMap::new(),
            notifications: VecDeque::new(),
            log_capacity: DEFAULT_LOG_CAPACITY,
            stats_capacity: DEFAULT_STATS_CAPACITY,
        }
    }
}

impl AppState {
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Connected { version } => {
                if !matches!(self.connection, Connection::Connected { .. }) {
                    self.notify(NotificationKind::Info, format!("conectado ao Docker {version}"));
                }
                self.connection = Connection::Connected { version };
            }
            Event::Disconnected { reason } => {
                if self.connection != (Connection::Disconnected { reason: reason.clone() }) {
                    self.notify(NotificationKind::Error, format!("desconectado: {reason}"));
                }
                self.connection = Connection::Disconnected { reason };
            }
            Event::Containers(list) => {
                let alive: HashSet<&str> = list.iter().map(|c| c.id.as_str()).collect();
                self.logs.retain(|id, _| alive.contains(id.as_str()));
                self.stats.retain(|id, _| alive.contains(id.as_str()));
                self.inspect.retain(|id, _| alive.contains(id.as_str()));
                self.containers = list;
            }
            Event::LogLine { id, line } => {
                let cap = self.log_capacity;
                let buf = self.logs.entry(id).or_insert_with(|| LogBuffer::new(cap));
                buf.ended = false;
                buf.push(line);
            }
            Event::LogsEnded { id } => {
                let cap = self.log_capacity;
                self.logs.entry(id).or_insert_with(|| LogBuffer::new(cap)).ended = true;
            }
            Event::Stats { id, sample } => {
                let cap = self.stats_capacity;
                self.stats.entry(id).or_insert_with(|| StatsHistory::new(cap)).push(sample);
            }
            Event::Inspected { id, json } => {
                self.inspect.insert(id, json);
            }
            Event::ActionDone { id, action } => {
                let name = self.name_of(&id).to_string();
                let verb = match action {
                    Action::Start => "iniciado",
                    Action::Stop => "parado",
                    Action::Restart => "reiniciado",
                    Action::Pause => "pausado",
                    Action::Unpause => "retomado",
                    Action::Kill => "finalizado (kill)",
                    Action::Remove => "removido",
                };
                self.notify(NotificationKind::Info, format!("{name} {verb}"));
            }
            Event::Error(msg) => self.notify(NotificationKind::Error, msg),
        }
    }

    pub fn notify(&mut self, kind: NotificationKind, text: String) {
        if self.notifications.len() == MAX_NOTIFICATIONS {
            self.notifications.pop_front();
        }
        self.notifications.push_back(Notification { kind, text, at: Instant::now() });
    }

    pub fn container(&self, id: &str) -> Option<&ContainerSummary> {
        self.containers.iter().find(|c| c.id == id)
    }

    /// Nome do container, ou o ID curto se não estiver na lista.
    pub fn name_of<'a>(&'a self, id: &'a str) -> &'a str {
        self.container(id).map(|c| c.name.as_str()).unwrap_or(&id[..id.len().min(12)])
    }

    /// Containers que passam no filtro e cuja busca casa com nome, imagem ou ID.
    pub fn filtered<'a>(&'a self, filter: Filter, query: &str) -> Vec<&'a ContainerSummary> {
        let q = query.trim().to_lowercase();
        self.containers
            .iter()
            .filter(|c| filter.matches(c.state))
            .filter(|c| {
                q.is_empty()
                    || c.name.to_lowercase().contains(&q)
                    || c.image.to_lowercase().contains(&q)
                    || c.id.starts_with(&q)
            })
            .collect()
    }

    /// IDs de containers em execução (para assinar stats).
    pub fn running_ids(&self) -> HashSet<ContainerId> {
        self.containers.iter().filter(|c| c.state.is_running()).map(|c| c.id.clone()).collect()
    }

    /// Totais dos containers em execução: (CPU % do host, núcleos em uso, núcleos do host, memória).
    pub fn totals(&self) -> Totals {
        let mut t = Totals::default();
        for s in self
            .containers
            .iter()
            .filter(|c| c.state.is_running())
            .filter_map(|c| self.stats.get(&c.id).and_then(|h| h.latest()))
        {
            t.cores_used += s.cores_used();
            t.host_cores = t.host_cores.max(s.online_cpus);
            t.mem_usage += s.mem_usage;
            t.mem_limit = t.mem_limit.max(s.mem_limit);
        }
        t.cpu_host_percent = if t.host_cores > 0 { t.cores_used * 100.0 / f64::from(t.host_cores) } else { 0.0 };
        t
    }

    pub fn count(&self, filter: Filter) -> usize {
        self.containers.iter().filter(|c| filter.matches(c.state)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LogStream;

    fn container(id: &str, name: &str, state: ContainerState) -> ContainerSummary {
        ContainerSummary {
            id: id.into(),
            name: name.into(),
            image: "alpine".into(),
            command: String::new(),
            state,
            status: String::new(),
            created: 0,
            ports: vec![],
            compose_project: None,
        }
    }

    fn line(t: &str) -> LogLine {
        LogLine { stream: LogStream::Stdout, text: t.into() }
    }

    #[test]
    fn log_buffer_is_circular() {
        let mut b = LogBuffer::new(2);
        b.push(line("a"));
        b.push(line("b"));
        b.push(line("c"));
        let texts: Vec<_> = b.lines().iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["b", "c"]);
        assert_eq!(b.dropped, 1);
    }

    #[test]
    fn filter_and_search() {
        let mut s = AppState::default();
        s.apply(Event::Containers(vec![
            container("aaa111", "web", ContainerState::Running),
            container("bbb222", "db", ContainerState::Exited),
            container("ccc333", "cache", ContainerState::Paused),
        ]));
        let names = |v: Vec<&ContainerSummary>| v.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(s.filtered(Filter::Running, "")), ["web", "cache"]);
        assert_eq!(names(s.filtered(Filter::Stopped, "")), ["db"]);
        assert_eq!(names(s.filtered(Filter::All, "DB")), ["db"]);
        assert_eq!(names(s.filtered(Filter::All, "ccc")), ["cache"]);
        assert_eq!(s.running_ids(), HashSet::from(["aaa111".to_string()]));
    }

    #[test]
    fn removed_containers_drop_cached_data() {
        let mut s = AppState::default();
        s.apply(Event::Containers(vec![container("a", "web", ContainerState::Running)]));
        s.apply(Event::LogLine { id: "a".into(), line: line("x") });
        s.apply(Event::Stats { id: "a".into(), sample: StatsSample::default() });
        assert_eq!(s.logs["a"].len(), 1);
        s.apply(Event::Containers(vec![]));
        assert!(s.logs.is_empty() && s.stats.is_empty());
    }

    #[test]
    fn totals_normalize_cpu_by_host_cores() {
        let mut s = AppState::default();
        s.apply(Event::Containers(vec![
            container("a", "worker", ContainerState::Running),
            container("b", "web", ContainerState::Running),
            container("c", "old", ContainerState::Exited),
        ]));
        let sample = |cpu, mem| StatsSample { cpu_percent: cpu, online_cpus: 16, mem_usage: mem, mem_limit: 1000, ..Default::default() };
        s.apply(Event::Stats { id: "a".into(), sample: sample(118.0, 10) });
        s.apply(Event::Stats { id: "b".into(), sample: sample(42.0, 5) });
        // Stats de container parado não entram no total.
        s.apply(Event::Stats { id: "c".into(), sample: sample(999.0, 999) });
        let t = s.totals();
        assert!((t.cores_used - 1.6).abs() < 1e-9);
        assert_eq!(t.host_cores, 16);
        assert!((t.cpu_host_percent - 10.0).abs() < 1e-9); // 160% / 16 núcleos
        assert_eq!(t.mem_usage, 15);
    }

    #[test]
    fn action_done_uses_container_name() {
        let mut s = AppState::default();
        s.apply(Event::Containers(vec![container("abc", "web", ContainerState::Running)]));
        s.apply(Event::ActionDone { id: "abc".into(), action: Action::Stop });
        assert_eq!(s.notifications.back().unwrap().text, "web parado");
    }

    #[test]
    fn repeated_disconnect_notifies_once() {
        let mut s = AppState::default();
        for _ in 0..3 {
            s.apply(Event::Disconnected { reason: "x".into() });
        }
        assert_eq!(s.notifications.len(), 1);
    }
}
