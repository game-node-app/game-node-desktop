# Proposta técnica — primeiro marco Core/Daemon + CLI

**Status:** proposta para revisão, 2026-11-07; não implementada nem aprovada como
contrato definitivo. As decisões confirmadas permanecem no
[planejamento arquitetural](plugin-architecture-plan.md). Este documento é a fonte
da proposta técnica deste marco; não substitui as specs do produto estável.

## Recorte e dados de teste

A primeira plataforma é Linux, decisão confirmada. Windows fica para validação
posterior, mantendo separação entre domínio e adaptadores de transporte/SO.
O usuário utilizará sua própria conta GameNode nos testes, também confirmado.

Proposta para os dados locais: iniciar com um perfil de desenvolvimento separado,
autenticado com essa conta pelo navegador. Usar uma cópia dos dados existentes
quando necessário para testar compatibilidade; preservar o perfil original.
Usar a conta real não implica autorização para testes automatizados criarem ou
alterarem jogos remotos. Neste marco não há RPC de adição/status/restore.

O daemon será o único proprietário de banco, cache e credenciais REST. CLI e handler
de callback receberão somente dados necessários à interação. Preservar os adaptadores
atuais de armazenamento de credenciais e as regras de cache por conta. Não copiar
segredos para fixtures, logs ou exemplos. O uso de perfil separado é proposta;
o reaproveitamento direto do perfil atual ainda não foi decidido.

## Serviços e mensagens propostos

Namespace `gamenode.client.v1`. Tipos gerados serão convertidos na borda do daemon;
as máquinas de estado existentes não importarão Protobuf. Nomes abaixo são a
proposta de RPCs, não código existente.

| Serviço | RPC | Entrada | Saída |
| --- | --- | --- | --- |
| DaemonService | GetStatus | Empty | DaemonStatus |
| DaemonService | Stop | Empty | StopReply |
| AuthService | GetStatus | Empty | AuthStatus |
| AuthService | StartLogin | StartLoginRequest | Operation |
| AuthService | SubmitCallback | SubmitCallbackRequest | CallbackReply |
| GameLibraryService | ListInstallations | Empty | InstallationSnapshot |
| GameLibraryService | StartScan | Empty | Operation |
| OperationService | GetOperation | OperationRef | Operation |
| OperationService | WatchOperation | OperationRef | stream Operation |

`daemon start` é bootstrap local da CLI, não RPC de um serviço ainda desligado.
`SubmitCallback` é usado pelo handler, sem comando público que incentive copiar a
URI no terminal. A validação existente de alvo, tamanho, correlação de tentativa e
uso único do callback permanece no daemon; autenticação IPC não a substitui.

| Mensagem | Campos e significado |
| --- | --- |
| DaemonStatus | `instance_id` opaco e novo a cada execução, `version`, `api_major=1`, `lifecycle` (`ready` ou `stopping`), plataforma. Não contém tokens. |
| StopReply | `accepted`. Recusa por trabalho não interrompível gera erro de precondição; aceite não significa processo já encerrado. A CLI confirma a saída do processo antes de anunciar parada concluída. |
| StartLoginRequest | `provider`: Google, Discord ou Twitter, conforme provedores atuais. A CLI solicita escolha em terminal interativo se omitido; sem terminal, exige `--provider`. |
| AuthStatus | `state`, `purpose` e `stage` quando aplicáveis; resumo de conta apenas quando verificada, erro de restauração separado. Sem cookies ou credenciais REST. |
| SubmitCallbackRequest | `uri` sensível, limitada e validada pelo parser atual. Nunca devolver seu conteúdo em erros. |
| CallbackReply | `accepted`: callback entregue ao fluxo válido; a confirmação final do login vem da operação após verificar a sessão. |
| InstallationSnapshot | `instance_id`, estado de acesso local/remoto, contexto de conta/cache, `entries` e último resultado por provider. Snapshot consistente, sem filtros ou seleção da GUI. |
| Installation | `entry_key`, `provider`, `store_id`, título detectado e título exibido, diretório de instalação, match, estado de execução e playtime disponíveis no modelo atual. |
| Match | `oneof`: matched (`game_id`, origem escolhida/automática), no_game, ambiguous (`candidate_ids`) ou unmatched. Não agrupar instalações nem normalizar títulos novamente. |
| OperationRef | Par `instance_id` e `operation_id`; evita confundir uma referência antiga após restart. |
| Operation | Referência, `kind` (`login` ou `scan`), `state` (`queued`, `running`, `completed`), `revision`, horários e resultado terminal opcional. |
| OperationResult | `outcome` (`succeeded`, `partial`, `failed`, `cancelled` ou `expired`), erro estruturado quando aplicável e resultado específico de login/scan. |
| ScanResult | Resultado por provider, quantidade encontrada e estado de persistência (`confirmed`, `failed` ou `not_applied`), com erros estruturados. Não inventar contagens de arquivos ignorados que o scanner não fornece. |

Campos opcionais usam presença explícita; enums têm valor zero `UNSPECIFIED`.
IDs numéricos do catálogo são `uint64` no transporte e strings decimais no JSON.
Chaves locais permanecem `<provider>:<store-id>`; identificadores de operação são
opacos. Não expor diretamente mapas internos de campos de provider nem o snapshot
inteiro do Tauri.

## Autenticação, biblioteca e operações

`AuthStatus` preserva os estados atuais: starting, signed_out, pending, verifying,
signed_in, restore_failed e signing_out. `pending` distingue preparing e
awaiting_browser; `verifying` distingue login e restauração. Indisponibilidade do
servidor não será convertida em rejeição de credencial.

O login conserva o TTL atual de dez minutos. Callback aceito não significa login
concluído: aguardar sessão verificada e tratar falha de armazenamento conforme a
máquina existente. Uma segunda tentativa ativa retorna a mesma operação se for do
mesmo provider; tentativa conflitante exige esperar a atual terminar, sem abrir
dois fluxos concorrentes por acidente. Esse tratamento de concorrência é proposta
da API e deve ser verificado contra os testes existentes.

Listar e escanear respeitam a ativação de biblioteca já existente: conta restaurada
em cache pode permitir a projeção local durante verificação/indisponibilidade;
sem contexto de conta disponível, retornar precondição `LOGIN_REQUIRED` e orientar
o login. Não inventar conta local, não buscar a biblioteca remota inteira e não
confundir lista ainda carregando com biblioteca vazia. Incluir `access`
unverified/offline/verified e contexto de cache na resposta.

Operações de scan preservam a coordenação existente: um ciclo ativo e, quando
necessário, no máximo um adicional pendente. Pedidos que convergem para o mesmo
ciclo recebem a mesma referência; não prometer um scan independente por chamada.
Resultados antigos ou de conta anterior continuam ignorados pelas regras atuais.

`WatchOperation` envia primeiro o estado atual e depois revisões crescentes, até
enviar o resultado terminal e fechar. Não é um log de todos os eventos: o cliente
reconectado pode receber só o snapshot mais recente. Um consumidor lento não
bloqueia o Engine; pode perder revisões intermediárias, preservando o resultado
terminal ou recebendo desconexão para consultar novamente.

Scan termina somente após confirmação de persistência dos resultados por provider.
Falha de um provider não remove seu cache anterior; resultados válidos de outros
providers são preservados. Falha de persistência nunca vira sucesso. Scan vazio
ou parcial que o scanner considera conclusão normal conserva essa semântica:
`partial` só indica falhas que o mecanismo atual efetivamente reporta. Scan
concluído não promete que downloads de capas, matching remoto ou sincronização
social posteriores tenham terminado.

Reter as últimas 100 operações concluídas em memória; não descartar operações
ativas. Referência desconhecida, expirada por retenção ou de outra instância retorna
`NOT_FOUND`, sem reiniciar a operação. Reiniciar o daemon invalida esse histórico,
preservando os dados e mecanismos duráveis existentes.

Desconectar ou usar Ctrl+C durante espera de scan não cancela a operação. Para
login, a CLI encerra a espera e a tentativa continua até conclusão ou TTL; não
acrescentar RPC de cancelamento neste marco. Não haverá repetição automática de
`StartLogin`/`StartScan` após resposta perdida: informar aceitação incerta, evitando
duplicar efeitos. Consultas podem ser repetidas após reconectar.

## Erros, conexão e limites propostos

Erros anteriores à aceitação usam status gRPC e detalhes tipados `ErrorDetail`
(`code` estável, `message` sanitizada, `retryable`, provider opcional). Após aceite,
falhas de negócio ficam em `OperationResult`; um RPC de consulta bem-sucedido pode
retornar uma operação que falhou. A CLI traduz o resultado em código de saída.

| Status gRPC | Exemplos de códigos do produto |
| --- | --- |
| INVALID_ARGUMENT | INVALID_PROVIDER, INVALID_OPERATION_REF, INVALID_CALLBACK |
| UNAUTHENTICATED | LOCAL_CREDENTIAL_REJECTED; não confundir com login social ausente |
| FAILED_PRECONDITION | LOGIN_REQUIRED, LIBRARY_LOADING, LOGIN_IN_PROGRESS, STOP_BLOCKED, API_INCOMPATIBLE |
| NOT_FOUND | OPERATION_NOT_FOUND |
| UNAVAILABLE | DAEMON_UNAVAILABLE, DAEMON_STOPPING |
| DEADLINE_EXCEEDED | REQUEST_TIMEOUT |
| INTERNAL | INTERNAL_ERROR; detalhes sensíveis somente em diagnóstico sanitizado |

Credencial IPC em arquivo privado, conferência de UID e UDS seguem a decisão Linux.
Proposta de caminho: `$XDG_RUNTIME_DIR/gamenode/`, com socket `client.sock` e token
`client.token`; validar caminho absoluto, proprietário, permissões e comprimento.
Se o runtime dir estiver ausente/inválido, falhar com orientação clara, sem fallback
silencioso para TCP ou diretório temporário. O lock de produção deve ser estável
por usuário e funcionar mesmo entre sessões distintas; sua localização e mecanismo
devem ser definidos na implementação, sem depender somente desse runtime dir.

Bootstrap aguarda prontidão até 30 segundos. Consultas e aceitação de comandos têm
deadline de 5 segundos; não incluem o tempo completo do scan/login. A operação
aceita continua mesmo se o RPC expirar. Streaming não impõe deadline total ao scan;
uma quebra de conexão permite reconectar por até 30 segundos e consultar a mesma
referência. Esgotado esse prazo, a CLI sai com erro e informa a referência, sem
afirmar que o trabalho foi cancelado. Esses valores são limites iniciais propostos,
ajustáveis após medir o fluxo real; não substituem o TTL de login existente.

`GetStatus` informa versão de protocolo antes dos demais comandos; incompatibilidade
de major recusa operações com orientação de atualização. Em v1, acrescentar campos
compatíveis e reservar números/nomes removidos. JSON possui versão própria; não
depender implicitamente do serializador Protobuf para defini-lo.

## Saída da CLI

Preservar os comandos aceitos, acrescentando `auth login --provider <provider>`.
Texto por padrão; `--json` produz um único objeto final em stdout, sem cores ou
progresso intercalado. Progresso e diagnóstico vão para stderr. Listagem sem entradas
retorna sucesso se a biblioteca foi carregada; daemon inativo em `daemon status`
é consulta válida com `running=false`, sem iniciar o daemon.

Envelope JSON: `apiVersion`, `ok`, `data` e `error`, sempre presentes. `ok` indica
sucesso do comando: scan parcial/concluído com falha retorna `false` e conserva os
dados válidos em `data`. Erros usam `code`, `message`, `retryable` e `details`
opcional, sem segredos. Campos em lowerCamelCase, enums em snake_case, IDs uint64
como strings; ausências explícitas como `null` e listas vazias como `[]`.

Exemplo de scan solicitado com `--no-wait`, ainda sem resultado:

```json
{
  "apiVersion": "gamenode.cli/v1",
  "ok": true,
  "data": {
    "instanceId": "instance-example",
    "operationId": "scan-example",
    "kind": "scan",
    "state": "queued",
    "result": null
  },
  "error": null
}
```

Saídas já aprovadas: 0 sucesso, 1 falha operacional (inclusive scan parcial),
2 argumentos inválidos. Proposta: Ctrl+C usa o código convencional 130.
`operations status` retorna 0 para operação ativa ou concluída com sucesso, 1 para
resultado terminal malsucedido ou referência não encontrada. `--no-wait` retorna
0 após aceite, sem alegar conclusão. CLI não deve imprimir tokens ou callback.

## Critérios de aceitação propostos

1. Linux executa daemon e CLI com GUI fechada e sem dependência Tauri no daemon.
   CLI usa exclusivamente gRPC; segundo daemon não abre dados nem inicia serviços.
2. `daemon status` não inicia processo. Comando funcional inicia sob demanda e
   aguarda prontidão; falha de inicialização é reportada sem loop de reinícios.
3. Login abre navegador e recebe `app.gamenode://` pelo handler, sem GUI e sem mudar
   o servidor. Testar sucesso, sessão rejeitada, rede indisponível, callback inválido
   ou antigo e expiração. Testes automatizados usam fakes; teste manual usa a conta
   do usuário sem registrar segredos.
4. Biblioteca preserva chaves, exclusões, cache por conta e matching atuais. Não
   importar filtros da GUI nem a biblioteca remota completa. Validar sem sessão,
   restauração em andamento, cache disponível e lista vazia após carga concluída.
5. Scan distingue aceite, execução e persistência; testar provider com sucesso,
   erro, resultado vazio, parcial válido, falha de armazenamento e pedidos simultâneos.
   Esperar não bloqueia Engine; nenhum resultado perdido se torna sucesso.
6. CLI desconecta e reconecta à mesma operação. Restart invalida referência antiga;
   testar retenção e ausência de resposta sem repetir efeitos automaticamente.
7. Exercitar ausência/erro de credencial IPC, major incompatível e runtime dir
   inválido. Usar a prova de IPC como base, sem tratar o protótipo como daemon pronto.
8. Validar JSON como contrato, stdout/stderr, códigos de saída e ausência de dados
   secretos em todas as falhas. Usar testes relevantes, sem testes que apenas
   repitam a implementação.
9. Parada respeita operações não interrompíveis, conclui gravações e libera o lock
   ao terminar. Fechar a CLI mantém o daemon ativo.

O runtime e a Plugin API Steam ficam para sua etapa posterior. Os critérios deste
marco podem usar o scanner legado interno, conforme a estratégia já aceita.
