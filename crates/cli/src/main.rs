//! CLI para validar o `dockerview-core` sem interface gráfica.

use anyhow::Result;
use clap::{Parser, Subcommand};
use dockerview_core::{Action, Command, DockerClient, Event, LogStream, format_bytes, spawn_core};
use futures_util::StreamExt;

#[derive(Parser)]
#[command(name = "dockerview-cli", about = "Ferramenta de teste do núcleo do dockerview")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Versão do daemon Docker
    Version,
    /// Lista containers
    #[command(alias = "ls")]
    List {
        /// Inclui containers parados
        #[arg(short, long)]
        all: bool,
    },
    Start { id: String },
    Stop { id: String },
    Restart { id: String },
    Pause { id: String },
    Unpause { id: String },
    Kill { id: String },
    /// Remove um container
    Rm {
        id: String,
        #[arg(short, long)]
        force: bool,
    },
    /// Mostra logs
    Logs {
        id: String,
        #[arg(short, long)]
        follow: bool,
        #[arg(long, default_value = "100")]
        tail: String,
    },
    /// Estatísticas em tempo real (Ctrl+C para sair)
    Stats {
        id: String,
        /// Apenas uma amostra
        #[arg(long)]
        once: bool,
    },
    /// `docker inspect` em JSON
    Inspect { id: String },
    /// Usa o serviço com canais (o mesmo das UIs) e imprime os eventos
    Watch {
        /// Também assina stats deste container
        #[arg(long)]
        stats: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    if let Cmd::Watch { stats } = cli.cmd {
        return watch(stats).await;
    }

    let client = DockerClient::connect().await?;
    match cli.cmd {
        Cmd::Version => println!("{}", client.version().await?),
        Cmd::List { all } => list(&client, all).await?,
        Cmd::Start { id } => act(&client, &id, Action::Start).await?,
        Cmd::Stop { id } => act(&client, &id, Action::Stop).await?,
        Cmd::Restart { id } => act(&client, &id, Action::Restart).await?,
        Cmd::Pause { id } => act(&client, &id, Action::Pause).await?,
        Cmd::Unpause { id } => act(&client, &id, Action::Unpause).await?,
        Cmd::Kill { id } => act(&client, &id, Action::Kill).await?,
        Cmd::Rm { id, force } => {
            client.remove(&id, force).await?;
            println!("removido: {id}");
        }
        Cmd::Logs { id, follow, tail } => {
            let mut stream = Box::pin(client.logs(&id, follow, &tail));
            while let Some(line) = stream.next().await {
                let line = line?;
                match line.stream {
                    LogStream::Stderr => eprintln!("{}", line.text),
                    _ => println!("{}", line.text),
                }
            }
        }
        Cmd::Stats { id, once } => {
            println!("{:>8}  {:>22}  {:>7}  {:>10}  {:>10}  {:>5}", "CPU %", "MEM", "MEM %", "NET RX", "NET TX", "PIDS");
            let print = |s: dockerview_core::StatsSample| {
                println!(
                    "{:>7.2}%  {:>22}  {:>6.2}%  {:>10}  {:>10}  {:>5}",
                    s.cpu_percent,
                    format!("{} / {}", format_bytes(s.mem_usage), format_bytes(s.mem_limit)),
                    s.mem_percent,
                    format_bytes(s.net_rx),
                    format_bytes(s.net_tx),
                    s.pids
                )
            };
            if once {
                print(client.stats_once(&id).await?);
            } else {
                let mut stream = Box::pin(client.stats(&id));
                while let Some(s) = stream.next().await {
                    print(s?);
                }
            }
        }
        Cmd::Inspect { id } => println!("{}", client.inspect_json(&id).await?),
        Cmd::Watch { .. } => unreachable!(),
    }
    Ok(())
}

async fn list(client: &DockerClient, all: bool) -> Result<()> {
    let containers = client.list_containers(all).await?;
    println!("{:<12}  {:<24}  {:<28}  {:<10}  {:<20}  PORTS", "ID", "NAME", "IMAGE", "STATE", "STATUS");
    for c in containers {
        let ports: Vec<_> = c.ports.iter().map(ToString::to_string).collect();
        println!(
            "{:<12}  {:<24}  {:<28}  {:<10}  {:<20}  {}",
            c.short_id(),
            truncate(&c.name, 24),
            truncate(&c.image, 28),
            c.state,
            truncate(&c.status, 20),
            ports.join(", ")
        );
    }
    Ok(())
}

async fn act(client: &DockerClient, id: &str, action: Action) -> Result<()> {
    client.perform(id, action).await?;
    println!("{action}: {id}");
    Ok(())
}

async fn watch(stats: Option<String>) -> Result<()> {
    let core = spawn_core(None);
    if let Some(id) = stats {
        core.send(Command::SubscribeStats(id));
    }
    println!("observando eventos (Ctrl+C para sair)...");
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            ev = core.rx.recv_async() => match ev? {
                Event::Containers(list) => {
                    let running = list.iter().filter(|c| c.state.is_running()).count();
                    println!("[containers] {} total, {running} rodando", list.len());
                }
                Event::Stats { id, sample } => println!(
                    "[stats {}] cpu {:.2}% mem {}",
                    &id[..id.len().min(12)],
                    sample.cpu_percent,
                    format_bytes(sample.mem_usage)
                ),
                other => println!("{other:?}"),
            }
        }
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}
