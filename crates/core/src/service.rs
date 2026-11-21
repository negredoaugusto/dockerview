//! Serviço que roda o cliente Docker num runtime tokio em thread própria e conversa
//! com a UI apenas por canais. Funciona com qualquer executor (egui, GPUI, CLI):
//! - a UI envia [`Command`] por `CoreHandle::tx`;
//! - a UI recebe [`Event`] por `CoreHandle::rx` (`try_recv` síncrono ou `recv_async`).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::task::JoinHandle;

use crate::client::DockerClient;
use crate::model::{Action, ContainerId, ContainerSummary, LogLine, LogStream, StatsSample};

#[derive(Debug, Clone)]
pub enum Command {
    /// Recarrega a lista de containers.
    Refresh,
    /// Mostrar também containers parados (padrão: `true`).
    SetShowAll(bool),
    Perform { id: ContainerId, action: Action },
    Remove { id: ContainerId, force: bool },
    /// Começa a seguir os logs; `tail` = linhas anteriores ("all" ou número).
    SubscribeLogs { id: ContainerId, tail: String },
    UnsubscribeLogs(ContainerId),
    SubscribeStats(ContainerId),
    UnsubscribeStats(ContainerId),
    Inspect(ContainerId),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum Event {
    Connected { version: String },
    Disconnected { reason: String },
    Containers(Vec<ContainerSummary>),
    LogLine { id: ContainerId, line: LogLine },
    LogsEnded { id: ContainerId },
    Stats { id: ContainerId, sample: StatsSample },
    Inspected { id: ContainerId, json: String },
    ActionDone { id: ContainerId, action: Action },
    Error(String),
}

/// Callback chamado após cada evento enviado (ex.: `ctx.request_repaint()` no egui).
pub type Notifier = Arc<dyn Fn() + Send + Sync>;

pub struct CoreHandle {
    pub tx: flume::Sender<Command>,
    pub rx: flume::Receiver<Event>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CoreHandle {
    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }
}

impl Drop for CoreHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Inicia o núcleo numa thread com runtime tokio próprio.
pub fn spawn_core(notifier: Option<Notifier>) -> CoreHandle {
    let (cmd_tx, cmd_rx) = flume::unbounded();
    let (ev_tx, ev_rx) = flume::unbounded();
    let events = EventSink { tx: ev_tx, notifier };

    let thread = std::thread::Builder::new()
        .name("dockerview-core".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("falha ao criar runtime tokio");
            rt.block_on(run(cmd_rx, events));
        })
        .expect("falha ao criar thread do core");

    CoreHandle { tx: cmd_tx, rx: ev_rx, thread: Some(thread) }
}

#[derive(Clone)]
struct EventSink {
    tx: flume::Sender<Event>,
    notifier: Option<Notifier>,
}

impl EventSink {
    fn send(&self, ev: Event) -> bool {
        let ok = self.tx.send(ev).is_ok();
        if let Some(n) = &self.notifier {
            n();
        }
        ok
    }

    /// Eventos enviados que a UI ainda não consumiu.
    fn backlog(&self) -> usize {
        self.tx.len()
    }
}

/// Acima disso, novas linhas de log são descartadas em vez de crescer a fila sem
/// limite (ex.: janela minimizada que não drena o canal + container muito verboso).
const MAX_PENDING_EVENTS: usize = 10_000;

const MAX_BACKOFF: Duration = Duration::from_secs(10);

async fn run(cmd_rx: flume::Receiver<Command>, events: EventSink) {
    let mut state = State { show_all: true, logs: HashMap::new(), stats: HashMap::new() };
    let mut backoff = Duration::from_millis(500);

    loop {
        // Fase de conexão: tenta conectar, mas continua atendendo Shutdown/SetShowAll.
        let client = match DockerClient::connect().await {
            Ok(c) => c,
            Err(e) => {
                events.send(Event::Disconnected { reason: e.to_string() });
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    cmd = cmd_rx.recv_async() => match cmd {
                        Ok(Command::Shutdown) | Err(_) => return,
                        Ok(Command::SetShowAll(v)) => state.show_all = v,
                        Ok(_) => { events.send(Event::Error("sem conexão com o Docker".into())); }
                    }
                }
                backoff = (backoff * 2).min(MAX_BACKOFF);
                continue;
            }
        };
        backoff = Duration::from_millis(500);

        let version = client.version().await.unwrap_or_else(|_| "?".into());
        tracing::info!(%version, "conectado ao Docker");
        events.send(Event::Connected { version });
        refresh(&client, &state, &events).await;

        // Reativa assinaturas de stats que existiam antes de uma desconexão.
        let stat_ids: Vec<_> = state.stats.keys().cloned().collect();
        for id in stat_ids {
            state.subscribe_stats(&client, &events, id);
        }

        let mut docker_events = Box::pin(client.container_events());
        let mut refresh_tick = tokio::time::interval(Duration::from_secs(5));
        refresh_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let reason = loop {
            tokio::select! {
                cmd = cmd_rx.recv_async() => {
                    let Ok(cmd) = cmd else { return };
                    if matches!(cmd, Command::Shutdown) {
                        state.abort_all();
                        return;
                    }
                    state.handle(&client, &events, cmd).await;
                }
                ev = docker_events.next() => match ev {
                    Some(Ok(ev)) => {
                        tracing::debug!(id = %ev.id, action = %ev.action, "evento docker");
                        // exec_*, health_status etc. não mudam a lista.
                        if !ev.action.starts_with("exec_") {
                            refresh(&client, &state, &events).await;
                        }
                    }
                    Some(Err(e)) => break e.to_string(),
                    None => break "stream de eventos encerrado".to_string(),
                },
                // Atualização periódica: o texto de status ("Up 5 minutes") muda com o tempo.
                _ = refresh_tick.tick() => refresh(&client, &state, &events).await,
            }
        };

        tracing::warn!(%reason, "desconectado do Docker");
        state.abort_running();
        events.send(Event::Disconnected { reason });
    }
}

struct State {
    show_all: bool,
    logs: HashMap<ContainerId, JoinHandle<()>>,
    stats: HashMap<ContainerId, JoinHandle<()>>,
}

impl State {
    async fn handle(&mut self, client: &DockerClient, events: &EventSink, cmd: Command) {
        match cmd {
            Command::Refresh => refresh(client, self, events).await,
            Command::SetShowAll(v) => {
                self.show_all = v;
                refresh(client, self, events).await;
            }
            Command::Perform { id, action } => {
                // Ações podem demorar (stop espera até 10s): rodam em background.
                let (client, events) = (client.clone(), events.clone());
                tokio::spawn(async move {
                    match client.perform(&id, action).await {
                        Ok(()) => events.send(Event::ActionDone { id, action }),
                        Err(e) => events.send(Event::Error(format!("{action} {}: {e}", short(&id)))),
                    };
                });
            }
            Command::Remove { id, force } => {
                self.stop_logs(&id);
                self.stop_stats(&id);
                let (client, events) = (client.clone(), events.clone());
                tokio::spawn(async move {
                    match client.remove(&id, force).await {
                        Ok(()) => events.send(Event::ActionDone { id, action: Action::Remove }),
                        Err(e) => events.send(Event::Error(format!("remove {}: {e}", short(&id)))),
                    };
                });
            }
            Command::SubscribeLogs { id, tail } => {
                self.stop_logs(&id);
                let (client, ev) = (client.clone(), events.clone());
                let task_id = id.clone();
                let handle = tokio::spawn(async move {
                    let mut stream = Box::pin(client.logs(&task_id, true, &tail));
                    // Linhas descartadas porque a UI não estava consumindo a fila.
                    let mut skipped: u64 = 0;
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(line) => {
                                if ev.backlog() >= MAX_PENDING_EVENTS {
                                    skipped += 1;
                                    continue;
                                }
                                if skipped > 0 {
                                    let text = format!("… {skipped} linhas descartadas (interface não acompanhou)");
                                    let note = LogLine { stream: LogStream::Console, text };
                                    ev.send(Event::LogLine { id: task_id.clone(), line: note });
                                    skipped = 0;
                                }
                                if !ev.send(Event::LogLine { id: task_id.clone(), line }) {
                                    return;
                                }
                            }
                            Err(e) => {
                                ev.send(Event::Error(format!("logs {}: {e}", short(&task_id))));
                                break;
                            }
                        }
                    }
                    ev.send(Event::LogsEnded { id: task_id });
                });
                self.logs.insert(id, handle);
            }
            Command::UnsubscribeLogs(id) => self.stop_logs(&id),
            Command::SubscribeStats(id) => self.subscribe_stats(client, events, id),
            Command::UnsubscribeStats(id) => self.stop_stats(&id),
            Command::Inspect(id) => match client.inspect_json(&id).await {
                Ok(json) => {
                    events.send(Event::Inspected { id, json });
                }
                Err(e) => {
                    events.send(Event::Error(format!("inspect {}: {e}", short(&id))));
                }
            },
            Command::Shutdown => unreachable!("tratado no loop principal"),
        }
    }

    fn subscribe_stats(&mut self, client: &DockerClient, events: &EventSink, id: ContainerId) {
        self.stop_stats(&id);
        let (client, ev) = (client.clone(), events.clone());
        let task_id = id.clone();
        let handle = tokio::spawn(async move {
            let mut stream = Box::pin(client.stats(&task_id));
            while let Some(Ok(sample)) = stream.next().await {
                if !ev.send(Event::Stats { id: task_id.clone(), sample }) {
                    return;
                }
            }
        });
        self.stats.insert(id, handle);
    }

    fn stop_logs(&mut self, id: &str) {
        if let Some(h) = self.logs.remove(id) {
            h.abort();
        }
    }

    fn stop_stats(&mut self, id: &str) {
        if let Some(h) = self.stats.remove(id) {
            h.abort();
        }
    }

    /// Aborta as tasks em execução mas lembra quais stats estavam assinados.
    fn abort_running(&mut self) {
        self.logs.drain().for_each(|(_, h)| h.abort());
        self.stats.values().for_each(|h| h.abort());
    }

    fn abort_all(&mut self) {
        self.logs.drain().for_each(|(_, h)| h.abort());
        self.stats.drain().for_each(|(_, h)| h.abort());
    }
}

async fn refresh(client: &DockerClient, state: &State, events: &EventSink) {
    match client.list_containers(state.show_all).await {
        Ok(list) => events.send(Event::Containers(list)),
        Err(e) => events.send(Event::Error(format!("listar containers: {e}"))),
    };
}

fn short(id: &str) -> &str {
    crate::model::short_id_of(id)
}
