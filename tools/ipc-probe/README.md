# Prova de gRPC local do GameNode

Experimento isolado da arquitetura futura. Não é o daemon do produto, não contém
funcionalidades de jogos e não acessa banco, credenciais ou o IPC do Tauri.
Seu workspace e `Cargo.lock` próprios evitam alterar as dependências do desktop.

## Mecanismo testado

- Serviço Protobuf `Probe.Ping`, gerado por Prost e servido por Tonic.
- HTTP/2 gRPC sobre Unix Domain Socket, sem abrir porta TCP. A URI HTTP no conector
  é apenas a origem exigida pelo cliente; o conector abre somente `UnixStream`.
- Diretório privado `0700`, socket, lock e arquivo de credencial `0600`.
- Verificação de UID do peer no servidor e no cliente.
- Token aleatório de 256 bits por instância, lido do arquivo privado e enviado como
  metadata `authorization`. Não é o token social do GameNode e não é impresso.
- Comparação do token com `subtle`, rejeição gRPC `Unauthenticated` e timeout de
  três segundos para conexão/chamada.
- Lock exclusivo antes de substituir socket/credencial, rotação do token no restart
  e limpeza ao encerrar normalmente. Não há troca automática de um owner ativo.

O desenho parte do [exemplo UDS do Tonic](https://github.com/hyperium/tonic/blob/master/examples/src/uds/server.rs),
com autenticação e testes próprios para este experimento.

## Executar

Requer Rust compatível com o projeto, `protoc`, Cargo e Unix. O teste foi realizado
em Linux x86-64 com Rust 1.99.1, Tonic 0.14.9 e Prost 0.14.4.

Na raiz do repositório:

```bash
cargo build --manifest-path tools/ipc-probe/Cargo.toml --locked
cargo test --manifest-path tools/ipc-probe/Cargo.toml --locked
cargo clippy --manifest-path tools/ipc-probe/Cargo.toml --locked --all-targets -- -D warnings
```

No primeiro terminal, inicie o servidor:

```bash
./tools/ipc-probe/target/debug/gamenode-ipc-probe serve "/tmp/gamenode-ipc-probe-$UID"
```

No segundo terminal, como o mesmo usuário:

```bash
./tools/ipc-probe/target/debug/gamenode-ipc-probe ping "/tmp/gamenode-ipc-probe-$UID"
./tools/ipc-probe/target/debug/gamenode-ipc-probe ping-without-token "/tmp/gamenode-ipc-probe-$UID"
./tools/ipc-probe/target/debug/gamenode-ipc-probe ping-wrong-token "/tmp/gamenode-ipc-probe-$UID"
```

O primeiro comando retorna `OK transport=grpc-over-uds uid=...`; os dois seguintes
devem retornar erro `Unauthenticated` e código de saída diferente de zero.
Outro `serve` no mesmo diretório deve falhar por lock. Use Ctrl+C no servidor para
encerrar. Não copie ou imprima `instance.token` para executar estes testes.

## Evidência e limites

Validação em 2026-11-07:

- Quatro testes de integração passaram: autenticação e permissões, exclusão e
  rotação após restart, recusa de caminhos inseguros e recuperação de socket órfão.
- Servidor e clientes também foram executados como processos separados: chamada
  válida aceita, credenciais ausente/incorreta recusadas, segundo owner recusado e
  encerramento por SIGINT com limpeza de socket e credencial.
- Clippy passou com `-D warnings`.

Isso demonstra viabilidade da combinação Rust/Tonic + UDS neste Linux. Não comprova
compatibilidade Windows, macOS ou SDKs em outras linguagens; esses alvos não foram
testados. Também não valida streaming, reconexão, carga ou toda a API de produção.

Este token autentica posse de um segredo acessível ao mesmo usuário do SO. Não
distingue uma CLI oficial de outro processo com o mesmo acesso e não constitui
sandbox, permissão por plugin ou autenticação do usuário na plataforma social.
O processo com acesso ao diretório pode ler a credencial. O protótipo não fornece
proteção contra um invasor com os mesmos privilégios ou validação de assinatura do
executável. A checagem de UID diferente existe no código, mas não foi exercitada com
uma segunda conta do SO neste ambiente.

Os caminhos são fornecidos explicitamente e devem estar sob um pai confiável.
Descoberta do endpoint, endurecimento contra corridas de filesystem, limites de
conexões, rotação em sessão ativa e entrega de credenciais a plugins são decisões
de produção ainda pendentes. O lock aqui é do experimento: a exclusão mútua real
entre Tauri e daemon continua uma tarefa da migração.
