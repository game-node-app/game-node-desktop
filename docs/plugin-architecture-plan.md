# GameNode — planejamento da arquitetura Core/Daemon + Plugins

**Status:** arquitetura futura em planejamento; decisões confirmadas na conversa,
com contratos e plano de execução ainda incompletos.
**Registro inicial:** 2026-11-06. Referência do código estável: `549c635`.
**Escopo:** separar o núcleo do cliente desktop das interfaces e tornar integrações
extensíveis, preservando as funcionalidades e os dados existentes.

## Autoridade e uso deste documento

Este é o registro das decisões e pendências da arquitetura futura. Não descreve
funcionalidades já entregues no produto. A prova isolada de transporte foi autorizada
explicitamente; as demais propostas não autorizam implementação por si só. Os requisitos
do produto estável continuam em [openspec/specs](../openspec/specs/), conforme a
[política documental](../openspec/README.md).

Uma decisão confirmada abaixo expressa direção arquitetural aceita, não um contrato
de transporte, schema de persistência ou garantia de segurança já implementada.
Propostas e pendências não devem ser promovidas a decisões por inferência.
Quando uma parte for formalizada em uma mudança OpenSpec, esta seção deverá apontar
para o artefato responsável e substituir a descrição duplicada por uma referência.

Discutir preferencialmente uma decisão por vez. Não reabrir decisões confirmadas
sem evidência de uma restrição ou problema técnico concreto.

## Ponto de partida verificado

O aplicativo estável usa Rust, Tauri v2, React e TanStack. O workspace contém
`crates/core`, `crates/platform`, `crates/api` e `crates/app`, além de `ui/`.

- [core](../crates/core/src/lib.rs) contém modelos, máquinas de estado e contratos
  locais, incluindo biblioteca, descoberta, matching e acompanhamento de execução.
- [platform](../crates/platform/src/lib.rs) implementa adaptadores de sistema
  operacional e persistência; [api](../crates/api/src/lib.rs) implementa o cliente remoto.
- [Engine](../crates/app/src/engine/mod.rs) coordena serviços e estado no aplicativo;
  [commands.rs](../crates/app/src/commands.rs) expõe operações ao frontend Tauri.
- O [frontend](../ui/src/api/useSnapshot.ts) consome snapshots e eventos do host.
  Esse mecanismo atual não constitui uma Client API gRPC.

A organização em crates é uma base para investigação, não prova de que o domínio
já esteja independente de todas as necessidades da interface. O levantamento de
dependências e da propriedade do estado ainda precisa ser concluído.

## Escopo confirmado para o primeiro marco

O usuário esclareceu em 2026-11-07 que a migração deve disponibilizar as
características existentes sob Core/Daemon e interface CLI, preservando o
comportamento do software. A implementação Tauri é a referência para o levantamento;
as specs estáveis registram os requisitos, e discrepâncias devem ser conciliadas
explicitamente. Não é necessário redesenhar a detecção para iniciar a extração.

O fluxo alvo é CLI → Client API gRPC → daemon → serviços e adaptadores existentes.
A CLI não deve chamar diretamente os scanners ou importar o Engine para contornar
a API. O runtime de plugins não é pré-requisito para esse primeiro fluxo.

A conversa explorou variantes, agrupamento de instalações, opções de lançamento,
principal persistida por usuário do SO e escolha de alternativas quando indisponível.
Essas preferências foram aceitas conceitualmente, mas a orientação posterior delimita
o trabalho à preservação do comportamento atual: qualquer diferença funcional dessas
ideias fica fora do primeiro marco e exige uma mudança futura específica.
Não tratar as respostas exploratórias como autorização para acrescentar tais recursos
durante a extração.

Antes de perguntar por novas regras de produto, verificar código, specs e testes.
Classificar cada funcionalidade como preservada, extraída ou dependente de adaptação
de transporte. Levantar decisões apenas para lacunas reais introduzidas pela separação.

## Decisões confirmadas

### Escopo funcional e decisões operacionais — 2026-11-07

A primeira plataforma de implementação e validação será Linux. Windows fica para
um marco posterior. O usuário usará sua própria conta GameNode nos testes; isso
define a conta de autenticação, sem decidir automaticamente o reaproveitamento
direto dos arquivos locais ou autorizar alterações remotas por testes automatizados.

O primeiro conjunto Core/Daemon + CLI deverá disponibilizar as funções já presentes
no Tauri: login e restauração pelo navegador, listagem das instalações locais,
pesquisa/associação ao catálogo, adição dos jogos detectados à conta, alteração de
status, descoberta Steam e de diretórios monitorados, backup e restore de saves.
As restrições atuais por sistema operacional permanecem; essa lista não promete
backup ou cadastro manual no Linux onde o adaptador atual não oferece suporte.

Listar toda a biblioteca remota da conta está fora do escopo. Não confundir essa
operação com listar instalações detectadas e consultar sua associação à conta.
Depois da validação da CLI, o Tauri será o próximo cliente da mesma Client API.

Decisões aceitas pelo usuário:

- Uma instância de daemon por usuário do SO, iniciada sob demanda pela CLI.
  O daemon permanece ativo após o encerramento da CLI; não haverá configuração de
  inicialização automática com o sistema na primeira entrega.
- Login continua no navegador e preserva o contrato existente do servidor.
  A entrega do callback ao daemon deve ser resolvida localmente, sem novo fluxo REST.
- O daemon deve manter exclusão de instância por usuário antes de abrir os dados ou
  iniciar serviços. Não adaptar agora o Tauri monolítico para disputar esse lock:
  o usuário não pretende executá-lo durante o desenvolvimento do Core + CLI.
  Se a coexistência dos hosts voltar ao escopo, o lock comum e a recusa do segundo
  coordenador continuam sendo a abordagem aceita, a implementar antes desse uso.
- A CLI pode conectar-se ao daemon já ativo; não é outro escritor independente.
  O Tauri migrado posteriormente será cliente do daemon, não um segundo Core.
- A GUI volta à validação na fase de conexão com o daemon. Quando o conjunto
  daemon + GUI Tauri alcançar equivalência funcional com o cliente monolítico,
  o monólito será descontinuado; não se planeja manter os dois hosts em paralelo.
- O primeiro plugin deve reproduzir a funcionalidade já implementada no Tauri,
  sem criar novas regras de descoberta. A descoberta Steam foi escolhida para esse
  recorte; os contratos e o runtime ainda precisam ser formalizados.
- Implementar uma prova de transporte/autenticação local antes de adotar o mecanismo
  como infraestrutura de produção. O resultado está registrado abaixo.

### Ciclo de vida operacional confirmado

Encerrar a CLI ou fechar seu terminal não encerra o daemon nem interrompe
monitoramento, sincronização ou backups. O daemon permanece disponível para os
próximos clientes.

A solicitação de parada inicia um encerramento organizado: impede novas operações,
interrompe tarefas recorrentes, conclui gravações em andamento e libera o lock
somente ao terminar. Se houver uma operação que não possa ser interrompida com
segurança, a parada é recusada com o motivo e o daemon permanece ativo. A primeira
entrega não oferecerá encerramento forçado pelo comando da CLI.

O tratamento de sinais do SO, a classificação das tarefas que impedem a parada e
a recuperação após falha ainda precisam ser detalhados na implementação. Essas
regras são da arquitetura futura, não do host Tauri estável.

### Conexão local confirmada para Linux

A Client API usará gRPC sobre UDS em diretório privado dentro de
`$XDG_RUNTIME_DIR`, separado dos dados persistentes. A credencial será aleatória
por execução do daemon, guardada nesse diretório com acesso exclusivo ao usuário.
As duas pontas verificarão o usuário do processo conectado, conforme o mecanismo
validado no experimento.

Após reiniciar o daemon, a CLI deverá reler a credencial e reconectar. Somente o
processo que adquirir o lock poderá remover arquivos de conexão obsoletos e iniciar
uma nova instância. `daemon status` apenas consulta; comandos funcionais podem
iniciar o daemon sob demanda.

Essa autenticação autoriza acesso local do usuário, sem distinguir aplicativos
confiáveis e sem impor restrições aos plugins. O transporte no Windows permanece
pendente de validação. A implementação ainda deverá definir os nomes dos caminhos,
o comportamento quando `$XDG_RUNTIME_DIR` estiver ausente ou inválido e os detalhes
de prontidão, timeouts e reconexão; esta decisão não aprova fallback automático.

### Retorno do login confirmado

Um pequeno handler registrado no SO receberá as URIs `app.gamenode://` sem abrir
a GUI e encaminhará o callback ao daemon pela conexão local autenticada. O daemon
validará e concluirá o fluxo existente; a CLI apresentará o resultado ao usuário.
O navegador e o contrato do servidor permanecem os mesmos.

O callback não deverá aparecer em logs, pois pode conter credenciais. A instalação
e a remoção deverão registrar e remover corretamente o handler. O nome e a forma
do executável, o tratamento de daemon indisponível e a apresentação de cancelamento
ou timeout ainda serão detalhados; não há implementação desse handler novo.

### Recorte inicial confirmado da Client API

A primeira entrega da Client API deverá permitir consultar estado e versão do
daemon e solicitar sua parada; consultar autenticação, iniciar login e receber o
resultado; listar instalações locais com as informações existentes no Tauri; e
solicitar uma varredura e acompanhar sua conclusão. As demais funções do escopo
serão expostas em incrementos posteriores.

Ao aceitar um scan, o daemon devolverá um identificador de operação. A CLI poderá
acompanhar essa operação por streaming e consultar seu resultado após reconectar
à mesma instância do daemon. O contrato distinguirá solicitação aceita, varredura
em execução e resultado persistido, incluindo eventuais falhas por provider. A
CLI só anunciará sucesso após confirmação da persistência, preservando a semântica
atual de resultados parciais e erros, sem apresentar falhas como sucesso completo.

Uma desconexão da CLI não cancela o scan. Não será exigida persistência do histórico
dessas operações entre reinicializações nesta primeira entrega; a recuperação dos
dados e trabalhos já persistidos pelo produto continua sendo preservada. Retenção
em memória e resposta para identificadores desconhecidos ainda serão detalhadas.

Os contratos Protobuf permanecerão separados dos modelos internos. Nomes de RPCs,
mensagens, erros, deadlines e regras de compatibilidade ainda precisam ser
formalizados; o recorte aceito não define um schema nem altera regras de negócio.
A [proposta técnica do primeiro marco](core-cli-contract-proposal.md) reúne esses
detalhes para revisão, incluindo mensagens, erros, JSON e critérios de aceitação.
Seu conteúdo permanece proposto até aprovação, sem modificar as specs estáveis.

### Primeiro plugin confirmado: descoberta Steam

O primeiro plugin oficial reproduzirá a descoberta Steam existente: lerá os dados
locais da Steam e informará as instalações encontradas ao Core. Associação ao
catálogo, conciliação, persistência e vínculo com a conta permanecerão no Core.

Uma falha do plugin deverá preservar os dados anteriores, conforme o comportamento
atual. A extração também deverá preservar a distinção existente entre falha e scan
concluído com resultados parciais ou vazios, sem redefinir as regras de descoberta.
O recorte não acrescenta instalação de jogos nem novas regras de identificação.

O plugin Steam será iniciado sob demanda para uma varredura. Após conectar e
informar prontidão, executará o scan; após entregar o resultado completo, poderá
encerrar. Pedidos simultâneos preservarão a coordenação atual do scanner, sem
iniciar vários processos desse plugin. Uma falha será registrada pelo Core e
preservará os dados anteriores, distinguindo-a de um scan concluído com resultado
parcial ou vazio conforme as regras existentes.

O plugin de descoberta não precisa permanecer ocioso entre varreduras. Monitorar
jogos em execução continua sendo responsabilidade separada. Esta decisão vale
para o plugin Steam; não estabelece execução curta para todas as capacidades.

Na primeira entrega, falha de inicialização, crash ou perda de conexão do plugin
Steam encerrará a operação com erro explícito. Resultados incompletos dessa
execução não serão aplicados como scan concluído; os dados anteriores serão
preservados. Isso não altera os resultados parciais que o scanner atual considera
válidos quando conclui normalmente.

Não haverá reinício automático imediato após falha. Uma nova solicitação de scan
poderá iniciar outra instância normalmente. O diagnóstico identificará plugin,
etapa e motivo, sem expor credenciais. Timeouts e detecção de processo conectado
mas sem resposta ainda precisam ser definidos.

Esse fluxo validará processo independente, comunicação gRPC e entrega de
descobertas. O runtime e os contratos serão implementados após validar a separação
Core/Daemon + CLI; a escolha da integração não antecipa essa etapa.

### Handshake de plugins confirmado

O daemon iniciará o processo com uma credencial temporária exclusiva daquela
execução. O plugin se conectará à Plugin API e apresentará essa credencial. No
handshake, ambos verificarão compatibilidade de protocolo e o plugin informará
suas capacidades; o daemon conferirá essas informações com o manifesto antes de
aceitar a instância como pronta. Ao encerrar ou falhar, a credencial será invalidada.

O mecanismo vincula a conexão à instância iniciada mediante posse da credencial;
não entrega credenciais REST ao plugin, não comprova procedência oficial e não
oferece sandbox. O canal de entrega da credencial e a política concreta de versões
ainda precisam ser definidos. Não pressupor proteção contra outros processos do
mesmo usuário com acesso aos recursos da instância.

### Comandos e interação confirmados da CLI inicial

Os executáveis serão `gamenode-cli` para a CLI e `gamenode-daemon` para o serviço.
O nome do executável gráfico será tratado na integração da GUI. Os comandos
iniciais aprovados são:

```bash
gamenode-cli daemon status
gamenode-cli daemon start
gamenode-cli daemon stop
gamenode-cli auth status
gamenode-cli auth login
gamenode-cli games list
gamenode-cli games scan
gamenode-cli operations status <id>
```

`games scan` acompanha a operação até terminar; `--no-wait` apenas solicita e
retorna o identificador. `operations status <id>` consulta o estado ou resultado
posteriormente, respeitando a retenção da instância do daemon.

A saída será texto legível por padrão, com `--json` para automação. Resultados vão
para `stdout`; progresso e diagnósticos para `stderr`. Códigos de saída: `0` para
sucesso, `1` para falha operacional e `2` para argumentos inválidos. Scan parcial
informa as falhas e retorna `1`, preservando os resultados válidos. `Ctrl+C`
encerra a espera da CLI, sem cancelar o scan no daemon. O schema JSON e o detalhe
das mensagens ainda serão formalizados.

A tabela abaixo reúne o comportamento dos comandos iniciais e propostas para
incrementos posteriores. Os comandos adicionais não foram aprovados por esta
confirmação do conjunto inicial e ainda não existem como executáveis funcionais.

| Comando inicial ou proposto para incremento posterior | Comportamento |
| --- | --- |
| `gamenode-cli daemon status` | Informar daemon ativo, versão e endpoint, sem expor segredos. |
| `gamenode-cli daemon start` | Iniciar sob demanda respeitando o lock; operações funcionais também podem solicitar essa inicialização. |
| `gamenode-cli daemon stop` | Solicitar encerramento organizado; recusar com motivo se houver operação que não possa ser interrompida com segurança. |
| `gamenode-cli auth login` | Abrir o navegador e aguardar o callback do fluxo existente. |
| `gamenode-cli auth status` | Informar sessão verificada, em verificação, indisponível ou ausente. |
| `gamenode-cli auth logout` | Usar o sign-out existente, preservando os dados não secretos conforme as regras atuais. |
| `gamenode-cli games list` | Listar instalações locais, com chave, provider, título e match; não importar a biblioteca remota. |
| `gamenode-cli games scan` | Acompanhar scan até terminar; `--no-wait` devolve o identificador sem aguardar. Distinguir aceitação, resultado e erro de persistência. |
| `gamenode-cli operations status <id>` | Consultar estado ou resultado da operação na instância do daemon. |
| `gamenode-cli games search <texto>` | Pesquisar o catálogo para associação de entradas locais. |
| `gamenode-cli games match <entry-key> <game-id>` | Registrar a escolha de catálogo por conta, preservando o mecanismo atual. |
| `gamenode-cli games add <entry-key> --status playing` | Adicionar o jogo detectado à conta; usar somente status aceitos pelo contrato atual. |
| `gamenode-cli games status <entry-key> --status finished` | Editar o status da associação existente, preservando coleções e plataformas como hoje. |
| `gamenode-cli discovery ...` | Expor adicionar executável, diretórios monitorados, candidatos e aprovação; detalhar os subcomandos após mapear o fluxo atual. |
| `gamenode-cli saves ...` | Expor configuração, backup, histórico, preview e confirmação de restore do mecanismo atual. |

A primeira fatia verificável inclui estado/versão e parada do daemon, estado/login
de autenticação, listagem local e scan acompanhado, conforme o recorte confirmado
da Client API, além do início explícito e da consulta de operações aprovados acima.
As demais operações completam o escopo em incrementos e seus comandos ainda são
propostas.
A CLI precisa reportar limitações do SO e falhas reais, sem confundi-las com ausência
de jogos. Restore continua exigindo preview e confirmação; não reduzir a proteção
existente para acomodar uma interface de terminal.

### Prova de transporte e autenticação local

Implementada em [tools/ipc-probe](../tools/ipc-probe/README.md), em workspace separado.
Valida Protobuf/gRPC sobre UDS com UID nas duas pontas e credencial aleatória por
instância em arquivo privado, sem usar banco ou token REST do produto.

Quatro testes e uma execução entre processos separados passaram em Linux x86-64;
Clippy também passou. Credenciais ausentes/incorretas foram recusadas, assim como
um segundo owner; restart invalida a credencial anterior. O mecanismo é viável
nessa combinação Rust/Tonic. O mecanismo foi escolhido para a Client API no Linux,
conforme a decisão acima; não foi validado em Windows, outros SDKs, streaming ou carga. Os limites e
comandos de reprodução estão no README do experimento.

A exclusão do protótipo usa caminhos próprios. Ela não implementa ainda o lock
de produção do daemon. O IPC desktop existente pode ser aproveitado nessa extração.
Um lock compartilhado com o Tauri monolítico só será necessário se a coexistência
dos hosts voltar ao escopo; encaminhar ativação, sozinho, não excluiria dois hosts.

### 1. Daemon independente como núcleo

O núcleo será executado em um daemon independente de Tauri e de frameworks gráficos.
Ele coordenará o estado canônico local, a comunicação REST com o backend social,
a sincronização, o gerenciamento dos plugins e a mediação das capacidades.
Coordenará também monitoramento, normalização e reconciliação das descobertas.
As fronteiras exatas entre esses serviços ainda serão refinadas.

O backend remoto permanece separado e acessado por REST. Não foi decidida sua
migração para gRPC.

### 2. Interfaces independentes e preservação da GUI

GUI, CLI e eventual TUI serão clientes do daemon. A GUI Tauri será preservada e
migrará progressivamente para a Client API. Não existe decisão de reescrita imediata
da interface. Outras GUIs poderão usar a mesma API, sem registro como plugin funcional.

### 3. APIs distintas, com gRPC e Protocol Buffers

Client API e Plugin API serão contratos separados, ambos baseados em gRPC e
Protocol Buffers. Poderão compartilhar infraestrutura e tipos de transporte.
O domínio interno não deverá depender dos tipos gerados por Protobuf.

Client API atende interfaces e ferramentas. Plugin API atende o ciclo de vida,
as observações e as capacidades das extensões. Os serviços e mensagens ainda não
foram definidos.

### 4. Um processo independente por plugin

Cada plugin executará em seu próprio processo, sem carregamento como biblioteca
dinâmica no daemon. Isso permite independência de linguagem e runtime, gestão
individual do ciclo de vida e contenção de falhas de processo.

Processos separados não constituem sandbox. O modelo não garante restrição de
acesso a arquivos, rede ou execução de comandos pelo plugin.

### 5. Preferência de transporte local

Preferir Unix Domain Sockets onde houver suporte adequado. O SDK deverá abstrair
o transporte sempre que possível. A disponibilidade de AF_UNIX no Windows não
comprova compatibilidade com todos os runtimes gRPC pretendidos.

O transporte Windows permanece pendente: UDS, Named Pipes ou TCP loopback como
alternativa dependem de validação. Não há fallback definitivamente escolhido.

### 6. Capacidades e mediação pelo Core

Um plugin poderá oferecer várias capacidades e consumir capacidades de outros
providers por mediação do Core. Descoberta, identificação, metadados, biblioteca
adquirida e instalação são exemplos funcionais; seus namespaces não estão aprovados.

Integrações específicas de Steam, EA, lojas e gerenciadores de pacotes pertencem
aos plugins. O Core não será um catálogo universal de comandos dessas ferramentas.
Poderá fornecer mecanismos genéricos de abertura de URIs ou início de processos.
Capacidade funcional, como instalar um jogo, não se confunde com o mecanismo usado.

A existência de uma capacidade não garante que toda loja disponibilize API ou URI
pública e estável para realizá-la. Isso deverá ser validado por integração.

### 7. Observações versus estado canônico e operações sociais

Plugins fornecem observações e capacidades; não alteram arbitrariamente o estado
canônico da biblioteca. O Core processa e reconcilia as descobertas.

Detectar um jogo em execução é uma observação local. Interpretá-la e publicar
presença ou dados na plataforma social é responsabilidade de negócio do daemon.
Plugins não receberão automaticamente as credenciais REST do usuário.

Jogo de catálogo e instalação local são conceitos distintos. Um mesmo jogo pode
ter várias instalações, lojas, identificadores e fontes de evidência. A representação
dessas relações e as regras de conciliação ainda não foram aprovadas.

### 8. Confiança, permissões e elevação

Plugins declararão permissões em manifesto, com autorizações explícitas na aplicação.
Plugins capazes de executar comandos locais deverão ser destacados em vermelho.
Execução de comandos e execução administrativa serão permissões distintas.

Plugins executarão sem privilégios administrativos por padrão. Elevação deverá ser
necessária e deliberadamente autorizada; uma operação privilegiada não justificará
elevar implicitamente todo o plugin. Broker ou serviço privilegiado é uma proposta,
não uma solução escolhida.

O usuário assume a decisão de confiar nos plugins comunitários instalados. O projeto
mantém e oferece suporte aos oficiais, mas não audita cada operação dos comunitários.
O daemon continua responsável por proteger suas credenciais e funcionalidades sociais.

**Limitação comprovada do modelo:** um processo nativo com os privilégios do usuário
pode iniciar processos e acessar recursos diretamente, sem passar pelo SDK. Manifesto,
autorização e restrição efetiva do sistema operacional são camadas distintas.
Uma permissão negada no manifesto não garante bloqueio local sem isolamento efetivo.

### 9. Oficiais e comunitários

Ambos usarão contratos de extensão compatíveis; não há decisão de SDKs separados.
Os oficiais poderão acompanhar o instalador e ser selecionados por sistema operacional.
Sua procedência deverá ser verificável, não apenas autodeclarada no manifesto.
Assinaturas e distribuição ainda precisam ser definidas.

Os plugins oficiais previstos não instalarão jogos; poderão iniciá-los por URI.
Comunitários poderão oferecer instalação e outros mecanismos locais. Esta restrição
sobre instalação de jogos não redefine as funcionalidades estáveis de ferramentas
por jogo e backups; a destinação desses componentes na migração ainda será examinada.

### 10. CLI antes de uma TUI completa

A CLI será o primeiro cliente para validar a separação. Deverá consumir exclusivamente
a Client API gRPC para executar operações, sem importar a implementação interna do daemon.
Uma TUI poderá vir depois. Nomes de executáveis e bibliotecas ainda não estão definidos;
referências anteriores a `gamecli` designam o GameNode.

### 11. Migração incremental e preservação do produto

Preservar funcionalidades e dados do cliente estável. Validar primeiro um fluxo
funcional independente da GUI, antes de construir um sistema completo de plugins.
Cada transferência de responsabilidade deverá evitar dois escritores concorrentes
entre o host Tauri e o daemon.

Redução de memória é um objetivo a medir. Separar processos pode melhorar o ciclo
de vida, mas também adiciona overhead; não há redução de consumo garantida.

## Propostas discutidas, ainda não aprovadas como contratos

- Entidades conceituais `Game`, `Installation`, `ExternalIdentity` e
  `DiscoveryEvidence`. A discussão desse modelo foi adiada; não existe schema aceito.
- Workspace com domínio, aplicação, protocolo, SDK, daemon, CLI e desktop separados.
  Nomes e diretórios são ilustrativos, não uma refatoração autorizada.
- Manifesto com identidade, versão, entrypoint, plataformas, capacidades e permissões.
  YAML foi exemplo; formato e schema não foram escolhidos.
- Serviços gRPC para lifecycle, scan em stream, biblioteca e operações de instalação.
  Exemplos anteriores não fixam métodos nem mensagens.
- Sincronização de clientes por snapshot mais cursor e stream incremental.
- Estados de plugin instalado, iniciando, pronto, falhou, parando e parado e política
  de ativação para outras capacidades. A execução sob demanda da descoberta Steam
  e o handshake com credencial temporária por instância já estão confirmados acima.
- Ampliação da CLI para funções posteriores ao conjunto inicial confirmado,
  incluindo execução e plugins. `clap` e Ratatui foram sugestões, sem escolha
  obrigatória.

## Questões pendentes de decisão

As prioridades abaixo são uma recomendação de sequência, não decisões adicionais.

| Ordem | Tema | Questões a resolver | Resultado necessário |
| --- | --- | --- | --- |
| 1 | Domínio e conciliação existentes | Como o código atual representa identidade, instalações, evidências, conflitos e indisponibilidade? O que precisa atravessar a Client API sem mudar essa semântica? | Mapeamento dos modelos e cenários existentes, com apenas as lacunas de extração. |
| 2 | Propriedade e persistência | Quem possui cada estado? O que é cache, observação, estado durável ou estado da interface? Quais dados são da conta ou do usuário do SO? | Mapa de propriedade e escrita, incluindo recuperação e migração dos dados atuais. |
| 3 | Fronteira inicial de extração | Que dependências unem Engine, Tauri, frontend, REST e adaptadores? Como preservar tracking, matching, ferramentas e backups? | Inventário do código e primeiro fluxo vertical com critérios de aceitação. |
| 4 | Instância e inicialização | Instância por usuário, início sob demanda, permanência após a CLI e parada organizada estão aceitos; sem autostart na primeira entrega. Como mapear tarefas que impedem parada, sinais e falha de inicialização? Lock do daemon é necessário; adaptação do monólito fica fora desta etapa. | Detalhamento operacional, exclusão de instância do daemon e descoberta do endpoint. |
| 5 | IPC e identidade | Rust/Tonic + UDS foi validado no Linux. Como adotar e endurecer o mecanismo, suportar Windows/outros SDKs e definir autenticação dos plugins, revogação e versões? | Contrato de conexão de produção e matriz de compatibilidade. |
| 6 | Permissões e isolamento | Haverá plugins nativos confiáveis, restritos ou ambos? Quais restrições são realmente aplicadas em cada SO? Como elevar uma operação específica? | Modelo de autorização com garantias e limitações explícitas, incluindo acesso à API social. |
| 7 | Lifecycle de plugins | Como iniciar, verificar prontidão, detectar falha, reiniciar, encerrar e atualizar? O que acontece com tarefas em execução? | Máquina de estados, timeouts e política de recuperação. |
| 8 | Resolução de providers | Como escolher entre providers da mesma capacidade? Haverá preferência do usuário, prioridades, fallback e dependências opcionais? | Regras determinísticas de seleção e compatibilidade. |
| 9 | Operações assíncronas | Como representar progresso, cancelamento, retry, idempotência e operações externas não canceláveis? O que sobrevive ao restart? | Modelo mínimo de operação e recuperação sem duplicar efeitos. |
| 10 | Eventos e múltiplos clientes | Como garantir ordenação, snapshot consistente, cursor, reconexão, retenção e backpressure? Como coordenar alterações simultâneas? | Contrato de eventos e ressincronização. |
| 11 | Credenciais | Quem armazena os tokens e como migram do host? Como clientes e plugins acessam serviços autorizados sem receber o segredo? | Política de armazenamento e mediação autenticada. |
| 12 | Distribuição e compatibilidade | Como empacotar, verificar procedência, instalar, atualizar e remover? Como negociar versões independentes e novas permissões? | Política de pacote, origem oficial e atualização multiplataforma. |
| 13 | Observabilidade e recursos | Como obter logs por processo, correlação, diagnósticos e erros sem expor segredos? Como medir memória e latência? | Diagnóstico pela CLI e métricas comparáveis antes/depois. |

A definição do domínio não deve substituir silenciosamente o comportamento estável
de entradas por provider. Qualquer mudança nessa semântica precisa de decisão e
delta OpenSpec próprios. O mesmo vale para o escopo por conta dos dados existentes.

## Levantamento inicial: biblioteca e varredura

Inspeção realizada em 2026-11-07. Esta seção registra o código encontrado e propõe
a fronteira de extração; não define ainda um contrato Protobuf nem altera regras.

### Caminho atual

| Etapa | Implementação | Responsabilidade |
| --- | --- | --- |
| Composição | [main.rs](../crates/app/src/main.rs) | Adquire instância antes de abrir SQLite; cria runtime, caminhos, stores, credenciais, cliente REST, providers e Engine; também configura janela, tray e eventos Tauri. |
| Entrada da GUI | [commands.rs](../crates/app/src/commands.rs) | `get_snapshot` consulta o Engine; `rescan_library` chama `Engine::rescan()` e retorna sem esperar o scan. |
| Coordenação | [engine/mod.rs](../crates/app/src/engine/mod.rs) | Traduz comandos em inputs, executa efeitos e publica snapshots; possui fila FIFO para operações de storage. |
| Regras de biblioteca | [machine.rs](../crates/core/src/library/machine.rs) | Controla conta, cache, scan ativo, pedido subsequente e descarte de respostas antigas. Mantém também seleção e layout da interface. |
| Descoberta | [registry.rs](../crates/core/src/provider/registry.rs) e [Steam](../crates/core/src/provider/steam/mod.rs) | Resolvem roots e executam scanners nativos. O Engine executa o scan com `spawn_blocking`; para `manual`, lê registros aprovados do store e verifica disponibilidade do executável. |
| Persistência | [store.rs](../crates/core/src/library/store.rs) e [library_db.rs](../crates/platform/src/library_db.rs) | Contrato e implementação SQLite de entradas, resultados de scan, metadados, escolhas e exclusões. |
| Apresentação | [view.rs](../crates/core/src/library/view.rs) e [query.rs](../crates/core/src/library/query.rs) | Constroem linhas, detalhes e ações; a view inclui pesquisa, seleção e filtros da GUI. |
| Entrega ao frontend | `SnapshotPublisher` em `commands.rs` | Coalesce snapshots e os entrega ao React por `engine:snapshot`; é adaptador de interface, não regra de descoberta. |

O ciclo principal é `ScanRequested → RunScan → ProviderScanned → PersistScan`.
Depois de `ProviderScanned`, o Engine também sincroniza tracking, lê contadores e
avalia envio de totais conforme as condições existentes. Extrair apenas a chamada
ao scanner ignoraria esses efeitos e não reproduziria o fluxo do aplicativo.

### Comportamentos existentes a preservar

- A biblioteca fica inativa sem conta. A restauração pode abrir o cache da última
  conta enquanto verifica a sessão; uma rejeição fecha essa biblioteca. A CLI deve
  distinguir indisponibilidade por autenticação de uma biblioteca realmente vazia.
- Carregar o cache leva ao primeiro scan. Pedidos repetidos durante um ciclo
  geram no máximo um ciclo adicional; resultados obsoletos são ignorados.
  Um ciclo pode executar os scanners de vários providers em workers separados.
- As chaves continuam `<provider>:<id>`. Sucesso substitui o conjunto instalado do
  provider; erro de scan preserva o conjunto anterior. Exclusões e escolhas persistem.
- O scan Steam pode concluir com registros parciais ou vazios ao pular locais
  inacessíveis. Registros manuais indisponíveis continuam sujeitos às regras DRM-free.
- Chamadas de rede continuam dependentes da sessão verificada. A CLI não deve
  inventar uma conta ou liberar matching para facilitar a demonstração.
- O timer atual do host chama `Tick` e `maintenance()` a cada segundo. A manutenção
  coordena, entre outras tarefas, o scan periódico de 30 minutos. Esse agendamento
  precisa acompanhar o serviço ao sair do host gráfico.
- Escritas de persistência são enfileiradas. Terminar um scan em memória não comprova
  que todas as gravações já foram concluídas com sucesso.

### Fronteira mínima recomendada

Extrair a coordenação de aplicação hoje hospedada em `crates/app/src/engine` para
uma crate independente de Tauri, preservando inicialmente suas máquinas, ports e
efeitos. O nome da crate permanece uma escolha de implementação. `gamenode-core`
continua contendo as regras; não precisa absorver o host de processos ou o transporte.

Os módulos de produção do Engine inspecionados não usam Tauri diretamente. Recebem
um `tokio::runtime::Handle`, adaptadores em `Ports` e um callback de publicação.
Entretanto, a crate `gamenode-app` depende de `tauri` e usa `tauri-build`:
simplesmente importá-la no daemon manteria essa dependência de build.

A extração deve levar os módulos auxiliares do Engine e preservar seus testes;
recortar somente `rescan()` exigiria reimplementar a coordenação de conta,
persistência, matching e tracking. A dependência existente do Engine em adaptadores
de `gamenode-platform` também precisa ser mantida explícita; ele não é domínio puro.

O bootstrap reutilizável deve separar runtime, stores, providers e serviços de
janela, geometria, tray, protocolo de assets e emissão de eventos. A geração dos
manifests embutidos hoje depende de [build.rs](../crates/app/build.rs) e
[providers.rs](../crates/app/src/providers.rs); deve ficar disponível ao daemon
sem executar o build Tauri nem manter uma segunda lista manual de providers.

### Primeiro fluxo vertical proposto

1. A extração é inicialmente consumida pelo próprio host Tauri, preservando o
   funcionamento e os testes antes de transferir a propriedade para outro processo.
2. O daemon compõe a mesma lógica e adaptadores, controla o timer e expõe uma
   Client API gRPC mínima de estado da biblioteca e solicitação de scan.
3. A CLI lista o estado e solicita scan exclusivamente por essa API. Uma projeção
   de consulta própria deve usar as regras atuais de entradas, matches e exclusões,
   sem alterar a seleção ou herdar a pesquisa e o filtro salvos pela GUI.
4. O primeiro teste integrado usa uma conta restaurável e dados isolados; valida
   também a ausência de sessão. Uma autenticação completa pela CLI é uma etapa
   posterior, não justificativa para contornar a máquina de autenticação.

Não haverá acesso direto da CLI ao SQLite, scanners ou implementação do Engine.
Não introduzir nesse marco plugins, novas identidades ou preferências de lançamento.

### Dependências a resolver antes da implementação do fluxo

- **Propriedade da instância e do banco:** o IPC atual é de ativação/callback do
  desktop, não gRPC. Não iniciar daemon e host como escritores independentes do
  mesmo diretório real. Usar perfil isolado no primeiro ensaio e definir a passagem
  de propriedade antes de permitir coexistência sobre dados do usuário.
- **Autenticação e callback:** a validação da sessão pertence ao serviço extraído;
  janela, browser e encaminhamento de `app.gamenode://` exigem composição adequada.
  Não modificar o contrato de callback no primeiro marco.
- **Transporte e acesso local:** validar a combinação gRPC/IPC e a autenticação
  local antes de expor comandos que usem a sessão. O IPC existente não deve ser
  anunciado como Client API pronta.
- **Semântica de scan:** decidir como a resposta identifica pedido aceito,
  coalescência, término e erro. Não reutilizar a flag `scanning` como prova de
  persistência nem introduzir um framework geral de operações antes da necessidade.
- **Recursos auxiliares:** a composição atual inicializa backups e ferramentas.
  Verificar localização do helper, disponibilidade por SO e recuperação de jobs
  antes de afirmar que mover o Engine é suficiente para rodar sem a GUI.

### Verificação proposta para a primeira extração

Reaproveitar os testes existentes de [máquina de biblioteca](../crates/core/src/library/machine/tests.rs)
e [Engine](../crates/app/src/engine/tests/library.rs), especialmente cache antes da
verificação, rejeição de sessão, troca de conta, scan sem bloquear, coalescência,
persistência e exclusões. Preservar também os testes dos módulos auxiliares movidos.

Acrescentar testes de integração somente para a fronteira nova: CLI falando gRPC
com daemon, equivalência da listagem, isolamento dos filtros da GUI, estado sem
conta e resultados de scan. Verificar que a árvore de dependências do daemon não
inclui Tauri e que nenhuma GUI é inicializada durante o fluxo.

Este levantamento foi feito por leitura de código e testes; não representa uma
execução desses testes, um build do daemon ou um contrato já aprovado.

## Sequência de migração proposta

Esta sequência organiza a investigação e a implementação futura; não é cronograma
aprovado nem uma lista de tarefas prontas para execução.

1. Mapear dependências e propriedade do estado; extrair apenas a lógica necessária
   para um fluxo executável sem inicializar Tauri.
2. Criar um daemon mínimo com Client API gRPC, reutilizando os componentes extraídos.
3. Validar uma CLI listando jogos pelo daemon, sem GUI. Um adaptador legado de scanner
   pode atender esse marco, sem exigir Plugin Runtime pronto.
4. Após validar a CLI, adaptar o Tauri como próximo cliente da Client API, definindo
   um único responsável por cada escrita durante a transição.
5. Construir runtime mínimo de plugins: manifesto, processo, conexão autenticada,
   negociação de versão, capacidades, autorizações, encerramento e falha.
6. Migrar um scanner para o primeiro plugin oficial e validar descoberta,
   reconciliação, persistência e recuperação.
7. Expandir SDKs, distribuição, controle visual de permissões, providers e clientes.

O candidato a primeiro marco é listar jogos instalados pela CLI via daemon.
O levantamento inicial recomenda extrair o Engine com seus módulos auxiliares;
o recorte de implementação e os contratos ainda precisam ser formalizados.

## Critérios propostos para avaliar a migração

- Executar o primeiro fluxo com a GUI fechada e sem dependência de Tauri no daemon.
- Verificar reconciliação, persistência e reinicialização por cenários reproduzíveis.
- Preservar biblioteca, escolhas, sessões, backups e ferramentas existentes.
- Validar isolamento de falha de plugin e distinguir isso de isolamento de segurança.
- Medir memória total e por processo em repouso, scan e GUI fechada, comparando com
  o aplicativo estável; registrar também latência e quantidade de plugins ativos.
- Demonstrar que clientes usam a API pública e que cada estado possui um responsável
  definido, sem dependências circulares ou contratos de transporte no domínio.

O fluxo existente de listagem e varredura já recebeu o levantamento inicial acima.
O próximo passo é fechar as decisões que bloqueiam a primeira extração e formalizar
seus contratos e tarefas. A primeira entrega deve comprovar equivalência de
comportamento com o host atual, sem formalizar um novo modelo de descoberta nem
implementar o runtime de plugins antecipadamente.

## Pendências após a prova de IPC — 2026-11-07

### Decisões que bloqueiam o primeiro fluxo Core/Daemon + CLI

1. **Lifecycle operacional — decisão principal concluída:** o daemon permanece após
   a CLI terminar, com parada organizada, recusa de parada insegura e sem autostart
   na primeira entrega. Restam o mapeamento das tarefas que impedem parada, sinais
   do SO e tratamento de falha de inicialização/recuperação.
2. **Conexão de produção — mecanismo Linux decidido:** UDS em diretório privado
   dentro de `$XDG_RUNTIME_DIR`, credencial por execução e verificação de usuário
   nas duas pontas; somente o owner do lock limpa conexões obsoletas. Restam nomes
   dos caminhos, runtime dir ausente/inválido, prontidão, timeouts e reconexão.
   Windows, outros SDKs e endurecimento continuam pendentes.
3. **Exclusão de instância:** implementar o lock de produção do daemon antes de
   acessar dados ou iniciar serviços. Salvaguardas no Tauri monolítico ficam fora
   desta etapa, pois ele não será executado durante o desenvolvimento do Core + CLI.
   A abordagem de lock comum está aceita caso a coexistência volte ao escopo.
   O encaminhamento do callback de login continua necessário independentemente disso.
4. **Callback do login — abordagem decidida:** handler de `app.gamenode://` sem GUI,
   encaminhamento autenticado ao daemon, validação no daemon e resultado na CLI;
   sem registrar o callback em logs. Restam forma do executável, registro/remoção
   pelo instalador, daemon indisponível e apresentação de cancelamento/timeout.
5. **Client API mínima — recorte decidido:** estado/versão e parada do daemon,
   estado/login de autenticação, instalações locais e scan. Scan terá identificador,
   streaming e consulta após reconexão à mesma instância; sucesso depende de
   persistência confirmada. Não se exige histórico entre reinicializações. Restam
   RPCs e mensagens, estados de autenticação, erros, deadlines, compatibilidade,
   retenção em memória e resposta a identificadores desconhecidos.
6. **Comandos e interação — conjunto inicial decidido:** `gamenode-cli` e
   `gamenode-daemon`, comandos iniciais, espera do scan/`--no-wait`, consulta de
   operações, texto/`--json`, separação stdout/stderr e códigos 0/1/2 estão aceitos.
   Restam schema JSON, mensagens e confirmações das futuras ações sensíveis;
   comandos posteriores e nome do executável gráfico serão detalhados depois.

### Trabalho já orientado, ainda não executado

- Extrair o Engine e seu bootstrap sem Tauri, levando os testes existentes e a
  geração dos manifests; fazer primeiro o host atual consumir essa extração.
- Implementar a exclusão de instância do daemon e preservar dados, credenciais e
  recuperação de trabalhos. O lock da prova não é a implementação de produção.
  Não adaptar a exclusão do Tauri monolítico nesta etapa; não executar os dois hosts
  sobre o mesmo perfil. Descontinuar o monólito após equivalência do daemon + GUI.
- Criar daemon e CLI reais e testar equivalência da biblioteca, login, scan e efeitos
  associados, usando perfil isolado. Não há tais executáveis funcionais ainda.
- Detalhar a exposição incremental de associação ao catálogo, adição à conta,
  alteração de status, descoberta monitorada e backup/restore, respeitando os SOs
  suportados. Não acrescentar listagem integral da biblioteca remota.
- Após validar a CLI, fazer o Tauri consumir a mesma Client API. Medir regressões,
  memória e responsividade antes de ampliar o runtime.

### Decisões posteriores para plugins

- A descoberta Steam está confirmada como primeiro plugin. Falta formalizar seu
  contrato de observações e conclusão/falha, preservando o scanner existente.
- Sua execução será sob demanda, com prontidão antes do scan e possibilidade de
  encerramento após entregar o resultado completo. Preservar a coordenação atual
  de pedidos simultâneos, sem multiplicar processos desse plugin.
- A política inicial de falha está confirmada: erro explícito, preservação dos
  dados anteriores e nenhum reinício automático imediato; nova solicitação pode
  iniciar outra instância. Restam timeouts e detecção de ausência de resposta.
- O handshake com credencial por execução, conferência de capacidades com manifesto,
  compatibilidade de protocolo e invalidação ao encerrar/falhar está confirmado.
  Restam manifesto mínimo, entrega da credencial, política concreta de versões e
  detalhamento dos estados e limites operacionais.
- Definir permissões da API social, comunicação mediada e limites realmente impostos
  aos processos. Não considerar o token da prova um modelo de autorização por plugin.
- Definir resolução entre providers, tarefas duráveis, eventos e ressincronização
  conforme os fluxos concretos exigirem; não construir toda a abstração antecipadamente.
- Definir distribuição, procedência oficial, atualização e eventual sandbox.

### Pendências consolidadas após a revisão das decisões

Para organizar a discussão, restam oito frentes de planejamento, não oito perguntas
atômicas. A divisão abaixo é uma recomendação de sequência; não são novas decisões.
Os detalhes técnicos podem ser propostos nos artefatos de implementação, sem exigir
uma rodada de confirmação para cada campo, caminho ou timeout.

Antes da primeira implementação, fechar três frentes:

1. Plataformas da primeira entrega — decidido: Linux inicialmente, Windows depois.
   A proposta técnica recomenda erro explícito sem runtime dir válido, pendente
   de aprovação junto aos demais contratos.
2. Dados e credenciais — parcialmente decidido: testes com a conta real do usuário.
   Perfil separado e cópia dos dados existentes são propostas, não decisões;
   reaproveitamento/migração local e mapa de propriedade ainda precisam ser fechados.
3. Contratos e compatibilidade — proposta disponível: revisar em conjunto a
   [proposta técnica](core-cli-contract-proposal.md), que reúne RPCs, estados, erros,
   JSON, limites operacionais e critérios de aceitação. A maior parte é trabalho
   de especificação técnica, não uma nova escolha de comportamento do produto.

Antes das etapas posteriores, detalhar cinco frentes:

4. Manifesto e runtime de plugins: schema mínimo, entrega da credencial, versões,
   estados e timeouts, a partir do handshake e das políticas Steam já aceitos.
5. Autorizações, isolamento e elevação: o que será controlado pela API e o que será
   efetivamente restringido pelo SO, sem atribuir garantias indevidas ao manifesto.
6. Resolução entre providers quando houver mais de um para a mesma capacidade.
7. Eventos e operações ampliados: sincronização da GUI, múltiplos clientes,
   recuperação, retenção e cancelamento conforme os fluxos posteriores exigirem.
8. Distribuição e operação: procedência oficial, atualização, empacotamento,
   diagnóstico e critérios de medição de recursos.

Essas frentes consolidam a tabela geral e os detalhes restantes acima; não os
substituem nem reabrem decisões já confirmadas. Não é necessário concluir as cinco
frentes posteriores para iniciar a extração Core/Daemon + CLI.

### Evidência adicional do cliente estável

Durante o teste manual da GUI foi corrigida a interpretação do resumo de adição à
conta: o React passou a consumir os outcomes serializados pelo Rust e a calcular os
contadores a partir dos itens. A suíte de frontend passou com 178 testes, usando
`NODE_OPTIONS=--no-experimental-webstorage` para compatibilidade do Node 26 com
o ambiente de testes. O usuário confirmou adicionar três jogos e receber sucesso.
Essa correção preserva a operação existente; não é parte da extração do daemon.
