//! Testes de integração contra um daemon Docker real.
//!
//! Ignorados por padrão. Rode com:
//!     cargo test -p dockerview-core -- --ignored
//!
//! Cada teste cria containers `alpine` com nome único e label `dockerview.test=1`,
//! removidos automaticamente ao final (mesmo em caso de falha).

use std::collections::HashMap;
use std::time::Duration;

use bollard::Docker;
use bollard::models::{ContainerCreateBody, HostConfig, PortBinding};
use bollard::query_parameters::{CreateContainerOptions, CreateImageOptions, RemoveContainerOptions};
use dockerview_core::{Action, Command, ContainerState, DockerClient, Event, LogStream, spawn_core};
use futures_util::StreamExt;
use tokio::time::timeout;

const IMAGE: &str = "alpine:latest";
const WAIT: Duration = Duration::from_secs(20);

/// Container de teste removido com `force` no `Drop`.
struct TestContainer {
    id: String,
    name: String,
}

impl Drop for TestContainer {
    fn drop(&mut self) {
        let id = self.id.clone();
        // Drop é síncrono: usa um runtime próprio numa thread separada.
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async {
                if let Ok(d) = Docker::connect_with_defaults() {
                    let opts = RemoveContainerOptions { force: true, ..Default::default() };
                    let _ = d.remove_container(&id, Some(opts)).await;
                }
            });
        })
        .join();
    }
}

fn unique_name(test: &str) -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    format!("dockerview-test-{test}-{}-{nanos}", std::process::id())
}

async fn ensure_image(docker: &Docker) {
    if docker.inspect_image(IMAGE).await.is_ok() {
        return;
    }
    let opts = CreateImageOptions { from_image: Some(IMAGE.into()), ..Default::default() };
    let mut pull = docker.create_image(Some(opts), None, None);
    while let Some(r) = pull.next().await {
        r.expect("falha ao baixar alpine");
    }
}

/// Cria (sem iniciar) um container rodando `script` em `sh -c`.
async fn create(test: &str, script: &str, host_config: Option<HostConfig>) -> TestContainer {
    let docker = Docker::connect_with_defaults().unwrap();
    ensure_image(&docker).await;
    let name = unique_name(test);
    let body = ContainerCreateBody {
        image: Some(IMAGE.into()),
        cmd: Some(vec!["sh".into(), "-c".into(), script.into()]),
        labels: Some(HashMap::from([("dockerview.test".into(), "1".into())])),
        host_config,
        ..Default::default()
    };
    let opts = CreateContainerOptions { name: Some(name.clone()), ..Default::default() };
    let resp = docker.create_container(Some(opts), body).await.expect("criar container");
    TestContainer { id: resp.id, name }
}

/// Script que fica rodando e reage a SIGTERM imediatamente (stop rápido).
const LONG_RUNNING: &str = "trap 'exit 0' TERM; while true; do sleep 0.2; done";

async fn client() -> DockerClient {
    DockerClient::connect().await.expect("Docker indisponível")
}

async fn state_of(client: &DockerClient, id: &str) -> Option<ContainerState> {
    client.list_containers(true).await.unwrap().into_iter().find(|c| c.id == id).map(|c| c.state)
}

/// Espera até o container atingir `expected` (as transições do Docker não são instantâneas).
async fn wait_state(client: &DockerClient, id: &str, expected: ContainerState) {
    let result = timeout(WAIT, async {
        loop {
            if state_of(client, id).await == Some(expected) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    assert!(result.is_ok(), "container não chegou ao estado {expected}; atual: {:?}", state_of(client, id).await);
}

#[tokio::test]
#[ignore = "requer Docker"]
async fn connects_and_reports_version() {
    let c = client().await;
    let v = c.version().await.unwrap();
    assert!(v.contains("API"), "versão inesperada: {v}");
}

#[tokio::test]
#[ignore = "requer Docker"]
async fn full_lifecycle() {
    let c = client().await;
    let t = create("lifecycle", LONG_RUNNING, None).await;

    let listed = c.list_containers(true).await.unwrap().into_iter().find(|x| x.id == t.id).unwrap();
    assert_eq!(listed.name, t.name);
    assert_eq!(listed.image, IMAGE);
    assert_eq!(listed.state, ContainerState::Created);

    // Container parado não aparece sem `all`.
    assert!(!c.list_containers(false).await.unwrap().iter().any(|x| x.id == t.id));

    c.perform(&t.id, Action::Start).await.unwrap();
    wait_state(&c, &t.id, ContainerState::Running).await;
    assert!(c.list_containers(false).await.unwrap().iter().any(|x| x.id == t.id));

    c.perform(&t.id, Action::Pause).await.unwrap();
    wait_state(&c, &t.id, ContainerState::Paused).await;

    c.perform(&t.id, Action::Unpause).await.unwrap();
    wait_state(&c, &t.id, ContainerState::Running).await;

    c.perform(&t.id, Action::Restart).await.unwrap();
    wait_state(&c, &t.id, ContainerState::Running).await;

    c.perform(&t.id, Action::Stop).await.unwrap();
    wait_state(&c, &t.id, ContainerState::Exited).await;

    c.remove(&t.id, false).await.unwrap();
    assert_eq!(state_of(&c, &t.id).await, None);
}

#[tokio::test]
#[ignore = "requer Docker"]
async fn removing_running_container_requires_force() {
    let c = client().await;
    let t = create("force", LONG_RUNNING, None).await;
    c.perform(&t.id, Action::Start).await.unwrap();
    wait_state(&c, &t.id, ContainerState::Running).await;

    assert!(c.remove(&t.id, false).await.is_err());
    c.remove(&t.id, true).await.unwrap();
    assert_eq!(state_of(&c, &t.id).await, None);
}

#[tokio::test]
#[ignore = "requer Docker"]
async fn logs_separate_stdout_and_stderr() {
    let c = client().await;
    let t = create("logs", "echo out-1; echo err-1 >&2; echo out-2", None).await;
    c.perform(&t.id, Action::Start).await.unwrap();
    wait_state(&c, &t.id, ContainerState::Exited).await;

    let lines: Vec<_> = c.logs(&t.id, false, "all").collect::<Vec<_>>().await.into_iter().map(Result::unwrap).collect();
    let out: Vec<_> = lines.iter().filter(|l| l.stream == LogStream::Stdout).map(|l| l.text.as_str()).collect();
    let err: Vec<_> = lines.iter().filter(|l| l.stream == LogStream::Stderr).map(|l| l.text.as_str()).collect();
    assert_eq!(out, ["out-1", "out-2"]);
    assert_eq!(err, ["err-1"]);

    // tail limita às últimas N linhas.
    let tail: Vec<_> = c.logs(&t.id, false, "1").collect::<Vec<_>>().await;
    assert_eq!(tail.len(), 1);
}

#[tokio::test]
#[ignore = "requer Docker"]
async fn stats_of_running_container() {
    let c = client().await;
    let t = create("stats", LONG_RUNNING, None).await;
    c.perform(&t.id, Action::Start).await.unwrap();
    wait_state(&c, &t.id, ContainerState::Running).await;

    let s = timeout(WAIT, c.stats_once(&t.id)).await.unwrap().unwrap();
    assert!(s.pids >= 1, "pids = {}", s.pids);
    assert!(s.mem_usage > 0 && s.mem_limit > 0, "{s:?}");
    assert!((0.0..=100.0 * 1024.0).contains(&s.cpu_percent), "{s:?}");
}

#[tokio::test]
#[ignore = "requer Docker"]
async fn ports_are_reported() {
    let c = client().await;
    let host_config = HostConfig {
        port_bindings: Some(HashMap::from([(
            "80/tcp".to_string(),
            Some(vec![PortBinding { host_ip: Some("127.0.0.1".into()), host_port: Some(String::new()) }]),
        )])),
        ..Default::default()
    };
    let t = create("ports", LONG_RUNNING, Some(host_config)).await;
    c.perform(&t.id, Action::Start).await.unwrap();
    wait_state(&c, &t.id, ContainerState::Running).await;

    let info = c.list_containers(false).await.unwrap().into_iter().find(|x| x.id == t.id).unwrap();
    let port = info.ports.iter().find(|p| p.private_port == 80).expect("porta 80 não listada");
    assert_eq!(port.protocol, "tcp");
    assert_eq!(port.ip.as_deref(), Some("127.0.0.1"));
    assert!(port.public_port.is_some_and(|p| p > 0));
}

#[tokio::test]
#[ignore = "requer Docker"]
async fn inspect_returns_json() {
    let c = client().await;
    let t = create("inspect", "true", None).await;
    let json = c.inspect_json(&t.id).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["Id"], t.id.as_str());
    assert_eq!(v["Config"]["Labels"]["dockerview.test"], "1");
}

#[tokio::test]
#[ignore = "requer Docker"]
async fn unknown_container_is_an_error() {
    let c = client().await;
    assert!(c.perform("dockerview-nao-existe", Action::Start).await.is_err());
    assert!(c.inspect_json("dockerview-nao-existe").await.is_err());
}

#[tokio::test]
#[ignore = "requer Docker"]
async fn events_stream_reports_start() {
    let c = client().await;
    let t = create("events", LONG_RUNNING, None).await;
    // Streams do bollard são preguiçosos: a requisição /events só sai no primeiro poll.
    // Por isso o stream é consumido numa task desde já, antes do start.
    let (tx, rx) = flume::unbounded();
    let events = c.container_events();
    let reader = tokio::spawn(async move {
        let mut events = Box::pin(events);
        while let Some(ev) = events.next().await {
            if tx.send(ev.unwrap()).is_err() {
                break;
            }
        }
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    c.perform(&t.id, Action::Start).await.unwrap();

    let found = timeout(WAIT, async {
        while let Ok(ev) = rx.recv_async().await {
            if ev.id == t.id && ev.action == "start" {
                return true;
            }
        }
        false
    })
    .await;
    reader.abort();
    assert_eq!(found, Ok(true));
}

/// Recebe eventos do serviço até `pred` retornar `Some`.
async fn expect_event<T>(rx: &flume::Receiver<Event>, mut pred: impl FnMut(Event) -> Option<T>) -> T {
    timeout(WAIT, async {
        loop {
            if let Some(v) = pred(rx.recv_async().await.expect("core encerrou")) {
                return v;
            }
        }
    })
    .await
    .expect("evento esperado não chegou")
}

#[tokio::test]
#[ignore = "requer Docker"]
async fn service_end_to_end() {
    let t = create("service", "echo ola-do-container; trap 'exit 0' TERM; while true; do sleep 0.2; done", None).await;

    let notified = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let n = notified.clone();
    let core = spawn_core(Some(std::sync::Arc::new(move || {
        n.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    })));

    expect_event(&core.rx, |e| matches!(e, Event::Connected { .. }).then_some(())).await;
    expect_event(&core.rx, |e| match e {
        Event::Containers(list) => list.iter().any(|c| c.id == t.id).then_some(()),
        _ => None,
    })
    .await;

    core.send(Command::Perform { id: t.id.clone(), action: Action::Start });
    expect_event(&core.rx, |e| match e {
        Event::ActionDone { id, action: Action::Start } if id == t.id => Some(()),
        Event::Error(msg) => panic!("erro: {msg}"),
        _ => None,
    })
    .await;

    // A lista é atualizada sozinha pelo stream de eventos.
    expect_event(&core.rx, |e| match e {
        Event::Containers(list) => list.iter().any(|c| c.id == t.id && c.state == ContainerState::Running).then_some(()),
        _ => None,
    })
    .await;

    core.send(Command::SubscribeLogs { id: t.id.clone(), tail: "all".into() });
    expect_event(&core.rx, |e| match e {
        Event::LogLine { id, line } if id == t.id && line.text == "ola-do-container" => Some(()),
        _ => None,
    })
    .await;

    core.send(Command::SubscribeStats(t.id.clone()));
    let sample = expect_event(&core.rx, |e| match e {
        Event::Stats { id, sample } if id == t.id => Some(sample),
        _ => None,
    })
    .await;
    assert!(sample.pids >= 1);

    core.send(Command::Inspect(t.id.clone()));
    expect_event(&core.rx, |e| match e {
        Event::Inspected { id, json } if id == t.id => json.contains(&t.id).then_some(()),
        _ => None,
    })
    .await;

    core.send(Command::Perform { id: t.id.clone(), action: Action::Stop });
    expect_event(&core.rx, |e| match e {
        Event::LogsEnded { id } if id == t.id => Some(()),
        _ => None,
    })
    .await;

    core.send(Command::Remove { id: t.id.clone(), force: true });
    expect_event(&core.rx, |e| match e {
        Event::Containers(list) => (!list.iter().any(|c| c.id == t.id)).then_some(()),
        _ => None,
    })
    .await;

    assert!(notified.load(std::sync::atomic::Ordering::Relaxed) > 0, "notifier nunca foi chamado");
    drop(core); // Shutdown + join da thread não pode travar.
}
