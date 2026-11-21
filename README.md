# dockerview

App desktop em Rust para visualizar e gerenciar containers Docker.

O núcleo (`dockerview-core`) não depende de nenhuma interface gráfica. Sobre ele
existem duas interfaces, [egui](https://github.com/emilk/egui) e
[GPUI](https://www.gpui.rs/), além de uma CLI para testar o núcleo sem GUI.

## Funcionalidades

- Lista de containers atualizada em tempo real (via `/events` do Docker), com
  filtro por estado (todos, rodando, parados) e busca por nome, imagem ou ID
- Ações: start, stop, restart, pause/unpause, kill e remove (com confirmação)
- Logs em streaming, com filtro e rolagem automática
- Estatísticas ao vivo: CPU (% do host), memória, rede e PIDs, com gráficos
- `inspect` em JSON, com opção de copiar
- Totais de CPU e memória dos containers em execução
- Reconexão automática quando o daemon cai ou reinicia
- Tema claro/escuro (interface GPUI)

## Estrutura

```
crates/
├── core/       dockerview-core: cliente Docker (bollard), serviço por canais, estado de apresentação
├── cli/        dockerview-cli: binário de teste do core
├── gui-egui/   dockerview-egui: interface egui (eframe)
└── gui-gpui/   dockerview-gpui: interface GPUI (gpui-kit)
```

O núcleo roda o cliente Docker num runtime tokio em thread própria. As interfaces
conversam com ele apenas por canais: enviam `Command` e recebem `Event`. Os eventos
são aplicados ao `AppState`, que as interfaces só desenham.

## Requisitos

- Rust 1.88 ou mais recente (edition 2024)
- Docker em execução, com acesso ao socket (`/var/run/docker.sock`). Para não
  precisar de `sudo`, adicione seu usuário ao grupo `docker`.

A conexão usa `DOCKER_HOST` se estiver definida, ou o socket local padrão.

## Uso

```sh
# Interface GPUI
cargo run --release -p dockerview-gpui

# Interface egui
cargo run --release -p dockerview-egui

# CLI
cargo run -p dockerview-cli -- ls --all
cargo run -p dockerview-cli -- logs <id> --follow
cargo run -p dockerview-cli -- stats <id>
cargo run -p dockerview-cli -- --help
```

### Variáveis de ambiente

| Variável | Efeito |
|---|---|
| `DOCKER_HOST` | Endereço do daemon (ex.: `unix:///run/user/1000/docker.sock`) |
| `DOCKERVIEW_THEME` | `light` ou `dark` (GPUI; padrão `dark`) |
| `RUST_LOG` | Nível de log (ex.: `debug`) |

## Testes

```sh
# Testes unitários
cargo test --workspace

# Testes de integração contra um daemon real (criam e removem containers alpine)
cargo test -p dockerview-core -- --ignored
```

## Licença

[MIT](LICENSE). As fontes Geist em `crates/gui-gpui/assets/fonts` estão sob a
[SIL Open Font License](crates/gui-gpui/assets/fonts/OFL.txt).
