# Plugins de terceiros: pendências e sugestões para revisão conjunta

## Estado e finalidade

Registro de revisão de 2026-11-12 para `add-third-party-plugins`, preparado antes da retomada da implementação da tarefa 5.1. O usuário solicitou reunir as pendências e sugestões para resolver as questões conhecidas em conjunto, evitando decisões fragmentadas durante a implementação.

**Status: propostas para revisão, sem aprovação técnica implícita.** A autorização para documentar, commitar e publicar este registro não aprova todas as sugestões abaixo, não altera requisitos normativos nem marca tarefas como concluídas. Valores numéricos propostos precisam de validação de capacidade. Este inventário cobre lacunas identificadas nos artefatos e contratos examinados; não garante ausência de descobertas futuras.

O levantamento registrou 25/121 tarefas concluídas e 96 abertas no workspace. Esse retrato inclui alterações locais anteriores ainda não necessariamente presentes no commit deste documento. As tarefas dos demais changes são contabilizadas separadamente; conclusão de testes de contrato não substitui aceite nativo.

Decisões anteriormente confirmadas devem ser preservadas: `LibraryEntry.game_id` é o GameNode ID opcional; `installation_id` é uma identidade opaca e estável emitida pelo host. A identidade privada externa combina plugin, publisher fixado e perfil, com subescopos próprios de provider para contas e segredos. As questões abaixo detalham a realização dessas decisões, sem reabrir suas semânticas.

## Fontes e alcance

- [Proposta](../openspec/changes/add-third-party-plugins/proposal.md), [design](../openspec/changes/add-third-party-plugins/design.md) e [tarefas](../openspec/changes/add-third-party-plugins/tasks.md).
- Delta specs: [API](../openspec/changes/add-third-party-plugins/specs/desktop-plugin-api/spec.md), [permissões](../openspec/changes/add-third-party-plugins/specs/desktop-plugin-permissions/spec.md), [serviços](../openspec/changes/add-third-party-plugins/specs/desktop-plugin-services/spec.md), [providers](../openspec/changes/add-third-party-plugins/specs/desktop-providers/spec.md), [plugins externos](../openspec/changes/add-third-party-plugins/specs/desktop-external-plugins/spec.md), [desenvolvimento](../openspec/changes/add-third-party-plugins/specs/desktop-plugin-development/spec.md), [playtime](../openspec/changes/add-third-party-plugins/specs/desktop-playtime/spec.md) e [settings](../openspec/changes/add-third-party-plugins/specs/desktop-settings/spec.md).
- [Protobuf público](../crates/plugin-protocol/proto/plugin.proto), [host](../crates/app/src/plugin_host.rs), [autorização de provider](../crates/app/src/plugin_host/provider.rs) e [identidade atual da biblioteca](../crates/core/src/library/store.rs).
- [Steam](../openspec/changes/integrate-steam-plugin-into-main/tasks.md), [Package manager Linux](../openspec/changes/add-linux-package-manager-plugin/tasks.md) e [PC ports](../openspec/changes/add-pc-port-catalog/tasks.md).

Os itens combinam lacunas concretas de contrato, políticas ainda a detalhar e escolhas técnicas de implementação. Não são 33 perguntas obrigatórias ao usuário. A revisão conjunta deve separar mudanças de comportamento que exigem concordância de detalhes de engenharia que cabem no escopo aprovado.

## A. Contratos e arquitetura

Todos os itens desta seção estão **propostos**. A coluna de tarefas indica os principais pontos afetados, não uma autorização para reabrir ou concluir tarefas automaticamente.

| ID | Pendência e evidência | Sugestão | Prós | Contras / custo | Tarefas |
| --- | --- | --- | --- | --- | --- |
| D01 | Library está em `ResourceHost`, que ainda não é registrado pelo host; inclui métodos de etapas posteriores. | Manter o serviço. Implementar métodos progressivamente, retornar `UNIMPLEMENTED` nos indisponíveis e informar disponibilidade real em `DescribeApi`. | Preserva contrato e clientes; permite entrega incremental. | Introspecção, docs e comportamento precisam permanecer sincronizados. | 2.3, 5–16 |
| D02 | Recursos exigem operação emitida pelo host; a autorização existente está ligada a operações de provider. | Definir contextos próprios de recursos, emitidos pelo host e vinculados a identidade, perfil, concessões e escopo, sem simular provider. A emissão não concede autoridade adicional. | Permite consultas espontâneas e extensões independentes. | Acrescenta emissão, renovação e encerramento de contexto. | 4.2, 5.1, 8.1, 14.5 |
| D03 | Falta fechar o ciclo de assinaturas longas em relação à operação inicial. | Depois da validação inicial, vincular `Watch` à sessão, perfil e concessões. Pode sobreviver à operação inicial; encerra ao perder autorização ou sessão. Revalidar antes da entrega. | Assinaturas úteis durante toda a sessão; revogação previsível. | Gestão própria de streams e corridas de revogação. | 4.3, 5.1, 7.1, 12.4 |
| D04 | A semântica de instalação já foi aprovada; falta o mecanismo durável e sua migração. | IDs aleatórios opacos persistidos pelo host, com migração transacional das associações antigas. | Estabilidade independente de caminho, nome e match. | Migração e índices duráveis adicionais. | 5.1, 14.2 |
| D05 | `entry_key` usa provider e store ID, podendo colapsar instalações do mesmo item. | Separar item de catálogo, instalação e evidência de descoberta; admitir várias instalações por item. | Representa discos, versões e cópias sem duplicar o jogo lógico. | Reconciliação e seleção de instalação mais complexas. | 5.1, 14.2, 14.5 |
| D06 | `LibraryGet` aceita jogo e instalação, mas um jogo pode ter várias instalações. | Instalação é o alvo inequívoco. Se ambos vierem, exigir correspondência. Só aceitar jogo sozinho quando houver uma instalação autorizada; caso contrário, erro seguro de ausência/ambiguidade. | Evita seleção silenciosa do alvo errado. | Clientes precisam tratar ambiguidade; erros não podem revelar instalações proibidas. | 5.1, 7.2 |
| D07 | Favoritos pertencem ao GameNode ID; autorização e apresentação podem pertencer à instalação. | Verificar vínculo atual e manter favorito no jogo original ao rematching; nunca transferir automaticamente. | Preserva intenção e evita alteração indireta. | Corrigir match pode exigir ação adicional do usuário. | 5.2, 5.3 |
| D08 | Interação de leitura global, categorias públicas, registros selecionados e dados privados precisa de tabela completa. | `library.read.all` libera projeção básica da biblioteca; publicações e campos sensíveis usam autorizações próprias. Escopo de leitura não implica acesso a arquivos. | Limites claros e sem expansão implícita. | Mais combinações para documentar e testar. | 5.1, 12.3, 12.4, 19.3 |
| D09 | Spec exige conteúdo por plataforma, runtime informante e frescor; `LibraryEntry` atual não contém essa projeção. | Acrescentar projeção tipada opcional e filtros, incluídos apenas quando solicitados e autorizados. | Descoberta entre emuladores sem expor bytes ou caminhos. | Amplia mensagens e exige expiração/deduplicação. | 2.2, 5.1, 14.4 |
| D10 | Descoberta sem provider já é exigida; emissão do contexto e reconciliação precisam ser fechadas. | Host abre operação vinculada às raízes aprovadas; extensão envia observações e resultado completo ou falho. Reconciliar por produtor e raiz. | Preserva fatos em falhas e isola produtores. | Distinguir ausência confirmada, raiz indisponível e varredura parcial. | 8.1, 14.5, 19.5 |
| D11 | Design exige plano imutável e aprovação exata; pedido atual de custom launch não representa todo o ciclo explicitamente. | Receita versionada, plano resolvido no host, aprovação e execução pela identidade/revisão do plano; revalidar recursos antes de executar. | Vincula o revisado ao executado. | Modelos distintos de receita, plano e aprovação. | 2.2, 7.8–7.11, 10.7 |
| D12 | `user_intent_id` existe; emissão, validade e uso único precisam de contrato completo. | Token opaco de uso único ligado a plugin, perfil, ação e alvo, com validade curta. Sem token, confirmação do host com limite de frequência. | Suporta ações da UI e impede reutilização de cliques antigos. | UX de expiração, cancelamento e solicitações repetidas. | 7.2, 7.9, 10.7 |
| D13 | Spec exige proveniência de pacote, publisher, versão e linhagem; diversas respostas têm só parte desses campos. | Envelope tipado comum preenchido pelo host, com origem, horários e referências limitadas de derivação. | Auditoria consistente e fontes distinguíveis. | Mais armazenamento e tratamento de origens removidas. | 2.2, 12.2, 12.4 |
| D14 | Publicação exige intenção do produtor e concessão do usuário; representação deve ser uniforme entre APIs. | Estado explícito de publicação separado de registro privado e concessões de leitura/mutação; política idêntica em Data e domínio. | Evita publicação implícita e bypass por alias. | Mais transições em atualização, retirada e exclusão. | 12.2–12.4, 19.3–19.4 |
| D15 | Nem todas as publicações/respostas representam durabilidade efetiva e expiração. | Modo explícito nas novas escritas; responder modo efetivo e expiração. Dados de sessão pertencem à sessão autenticada do produtor; persistência exige concessão própria. | Evita perda inesperada após sucesso aparente. | Mais campos e tratamento de recusa nos clientes. | 2.2, 6.1, 12.2, 16.1 |
| D16 | `PersistenceMutationReply` existe, mas RPCs de limpeza retornam `MutationReply`, sem bytes recuperados. | Resposta específica com bytes recuperados, resultado parcial, proteção e eventual job. Se contrato já foi distribuído, evoluir de forma compatível ou adicionar método. | Explica efeitos reais e limpeza parcial. | Verificar distribuição anterior antes de mudar assinatura. | 2.2, 16.2–16.5 |
| D17 | Há semânticas padrão em `Struct` e extensões não uniformes entre recursos. | Tipar campos usados pelo host e métricas padrão; extensões versionadas apenas para anotações, nos recursos designados. | Validação e interoperabilidade melhores. | Mais modelagem inicial e manutenção do protobuf. | 2.2, 2.6–2.7, 12.1, 12.6, 15 |
| D18 | Métricas precisam de unidades, normalização, ausência, cobertura e agregação uniformes. | Definir timestamps e janelas; ausência explícita; distinguir medição de estimativa; agregações informam cobertura e lacunas. | Evita ausência virar zero e comparações incompatíveis. | Produtores e consumidores precisam tratar metadados adicionais. | 12.1, 15.3 |
| D19 | Inspeções extensas devem usar jobs; contratos misturam respostas imediatas e `job_id`. | Metadados estritamente limitados podem ser imediatos; traversal, hashing e comparação extensa sempre geram jobs, com resultados paginados. | Latência limitada e cancelamento consistente. | Mais chamadas para pequenas varreduras. | 7.1, 13.3–13.4 |

D01 corrige a avaliação da pausa anterior: registrar um serviço com métodos explicitamente indisponíveis pode ser escolha técnica normal durante desenvolvimento. Não exige necessariamente separar Library em outro serviço. Nenhum método indisponível deve ser anunciado como implementado ou responder sucesso fictício; a entrega final continua exigindo todo o escopo aprovado.

## B. Políticas transversais propostas

| ID | Questão | Sugestão | Prós | Contras / custo | Tarefas |
| --- | --- | --- | --- | --- | --- |
| D20 | Paginação e eventos | Ordenação determinística com desempate por ID; cursor ligado a consulta, perfil, concessões e revisão. Expiração inicial de 5 minutos. Histórico limitado para retomada; exigir ressincronização quando expirar. | Páginas consistentes e sem troca de escopo. | Mudanças frequentes podem invalidar paginação. | 5.1, 7.1, 12.4 |
| D21 | Idempotência | Chave por identidade, perfil e ação lógica, com hash do pedido. Repetição igual retorna resultado; diferente gera conflito. Persistir para efeitos duráveis por pelo menos 24 horas após conclusão e enquanto o job estiver ativo. | Retries e reinícios não duplicam efeitos dentro da janela. | Armazenamento adicional; não garante deduplicação eterna. | 6–7, 12, 16 |
| D22 | Limites | Tabela central com limites por quantidade e bytes, separando memória, persistência e concorrência. Publicar pela introspecção. | Evita limites divergentes e consumo ilimitado. | Ajustes podem ser necessários após medições. | 2.3, 5–7, 12, 15–16 |
| D23 | Cancelamento e reinício | Persistir jobs com efeitos duráveis; cancelar em fronteiras seguras e informar efeitos realizados. Retomar automaticamente só operações comprovadamente retomáveis. | Evita repetir lançamento, restauração ou instalação. | Algumas falhas exigem recuperação ou nova ação do usuário. | 7.1, 9.3, 16.5 |
| D24 | Perfil local e ausência de login | **Superada em 2026-11-14:** não há perfil local; o perfil é a conta GameNode logada (uso do app exige login). Sem sessão, nenhum contexto de perfil nem processo de terceiros; início offline não verificado usa a conta em cache; logout, sessão rejeitada e conta diferente são trocas de perfil com fencing; pacote/confiança por usuário do SO, ativação/concessões/configuração/dados por conta. Ver design §1, `desktop-plugin-permissions` (Account-bound plugin profile) e tarefa 4.6. A sugestão original conflitava com `desktop-favorites` (local-first sem login). | Uso offline previsível e isolamento. | Exige política explícita de importação/migração entre perfis; afeta comportamento do produto. | 4.4, 5.2, 8.5, 13.1, 16.1 |
| D25 | Troca do publisher | Novo fingerprint é nova identidade de segurança. Nova confiança não transfere concessões. Migração explícita de dados compatíveis preserva origem; segredos não migram automaticamente. | Evita herança silenciosa de acesso. | Recuperação legítima após perda de chave mais trabalhosa. | 3.5, 9.4, 16.4 |
| D26 | Atualizações | Índice versionado e assinado sobre bytes definidos, sequência e expiração. Readiness inicial de 15 segundos ajustável pelo host; limitar falhas consecutivas antes de manter inativo. | Recuperação previsível e sem loop de reinício. | Limite exato de falhas e comportamento de plugins lentos devem ser fechados; dados precisam preservar rollback. | 9.1–9.4 |
| D27 | Identidade de conteúdo | Separar conteúdo lógico, revisão dos bytes e localização. Hash é evidência; cópias podem compartilhar conteúdo mantendo instalações distintas. Não fundir variantes por nome. | Preserva variantes, saves e histórico em mudanças de caminho. | Hashing caro ou confirmação nos casos ambíguos. | 13.1, 14.2, 14.5 |
| D28 | Runtime e playtime | Propor observações a cada 5 segundos e frescor máximo inicial de 15 segundos, negociados. Duração pelo relógio monotônico do host; conflito sem evidência suficiente vira desconhecido, sem contabilização. | Limita contagem indevida e evita arbitragem arbitrária. | Pode subcontar em falhas; exige definir timestamps aceitos e prioridades de evidência. | 14.3, 19.1–19.2 |
| D29 | Arquivos e volumes | Handles ligados a perfil, concessão, raiz e identidade do volume; revalidar antes do efeito. Criação usa handle do diretório pai e nome relativo validado. | Trata arquivos novos e troca de discos com escopo definido. | Implementação por SO; identidade limitada em alguns volumes. | 6.2, 13.2, 13.5, 14.1 |
| D30 | Hash de árvore | Formato versionado, ordenação definida, caminhos relativos, tipos e hashes. Não seguir links por padrão. Instabilidade/incompletude resulta em indeterminado. | Comparação reproduzível sem falso inalterado. | I/O elevado; sem snapshot do filesystem não há instantâneo atômico garantido. | 13.4 |
| D31 | Metadados e artwork | Lista inicial pequena de campos descritivos, como nome de exibição. Overrides separados dos fatos detectados; reset explícito. PNG/JPEG/WebP estáticos com limites de bytes e pixels. | Evita conflitos e permite restaurar apresentação original. | Menor flexibilidade inicial; lista e limites exatos ainda devem constar do contrato. | 5.2, 5.4 |
| D32 | HTTP | Métodos documentados, HTTPS e origens aprovadas; bloquear headers de transporte perigosos; sem cookies do Core; revalidar redirects e endereço efetivamente conectado. | Integração auditável sem emprestar credenciais do host. | Serviços com cookies/redirects complexos exigem configuração; fechar lista exata de métodos/headers. | 6.3, 6.5 |
| D33 | Compatibilidade e aceite | Matriz por método, SO e versão; distinguir contrato, integração e aceite nativo. Anunciar suporte conforme implementação e evidência exigida. | Não confunde cross-build com execução real. | Depende de ambientes nativos e entradas adequadas. | 11.7, 18.1–18.10 |

Pontos iniciais para D22: 8 assinaturas por plugin, 256 eventos por fila com teto adicional de bytes, 2 jobs pesados simultâneos por plugin e limite global de concorrência. O teto de bytes, o limite global e os demais valores ainda precisam ser dimensionados. Saturação deve produzir erro recuperável ou ressincronização, nunca perda silenciosa de eventos necessários à consistência. Preservar os limites já aprovados: páginas até 200, valor privado de 1 MiB, quota privada de 16 MiB, quota gerenciada padrão de 64 MiB incluindo a privada, HTTP até 8 MiB e 30 segundos.

D25, D28 e a experiência de múltiplas instalações em D05–D06 merecem revisão de produto explícita. Detalhes de implementação podem ser resolvidos tecnicamente dentro das diretrizes que forem aprovadas.

## C. Organização, correções e critérios de fechamento

| Pendência | Sugestão | Prós | Contras |
| --- | --- | --- | --- |
| Contratos/SDK já marcados concluídos | Auditar 2.2, 2.4 e 2.6 contra cenários; reabrir apenas partes incompletas ou criar tarefas corretivas rastreáveis. | Preserva trabalho válido e torna progresso confiável. | Contagem de conclusão pode diminuir. |
| Dependências fora da ordem numérica | Definir dependências explícitas: contratos → identidade/contextos/jobs → persistência/autorização compartilhada → adaptadores → UI/exemplos → aceite. | Reduz retrabalho em Library, launch e storage. | Reorganização prévia do checklist. |
| Change extenso | Manter escopo aprovado e executar em marcos internos verificáveis. Separar changes só com contratos e integração rastreados. | Revisões menores sem perder requisitos. | Não reduz o volume total. |
| Processo de decisão | Revisar um pacote único com decisão, alternativas, tarefas afetadas e critérios de aceite. | Reduz interrupções e decisões contraditórias. | Etapa inicial de planejamento mais cuidadosa. |

Antes de considerar o planejamento fechado:

1. Registrar para cada D01–D33 a resolução: aprovada, substituída ou já coberta, com referência; detalhar os valores/listas ainda propostos.
2. Reconciliar proposta, design, delta specs, tarefas e contratos públicos; preservar semânticas previamente aprovadas e compatibilidade oficial.
3. Conferir campos de request/response, erros, disponibilidade, autorizações e efeitos para cada RPC e evento, não apenas existência de nomes de rotas.
4. Fixar dependências e critérios de aceite; identificar tarefas a adicionar/reabrir, sem marcar implementação concluída por atualização documental.
5. Registrar ambientes e entradas necessárias ao aceite nativo. Sincronização de main specs e implementação seguem suas autorizações próprias.

## D. Desenvolvimento restante no change

Retrato do checklist do workspace na data do levantamento; não representa aprovação das propostas deste registro.

| Bloco | Tarefas abertas |
| --- | ---: |
| 5. Library, metadados, favoritos, matching e artwork | 5 |
| 6. Storage, arquivos, rede e notificações | 5 |
| 7. Jobs, launch, instalação, backups, ferramentas e ports | 11 |
| 8. Providers externos, credenciais e configuração | 7 |
| 9. Atualizações e recuperação | 4 |
| 10. Interface de gerenciamento | 8 |
| 11. SDK e exemplos Rust/Go | 9 |
| 12. Compartilhamento e consentimento | 7 |
| 13. Saves, inspeção e volumes | 7 |
| 14. Bibliotecas, runtimes e conteúdo | 5 |
| 15. Diagnósticos, hardware, presença e desempenho | 4 |
| 16. Persistência, quotas e limpeza | 5 |
| 17. Administração e interoperabilidade | 4 |
| 18. Aceite integrado e nativo | 10 |
| 19. Playtime, publicação e descoberta | 5 |
| **Total** | **96** |

## E. Outros changes ativos

Há 19 tarefas abertas adicionais no retrato consultado: Steam 11, package manager Linux 4 e PC ports 4. São escopos relacionados com aceite próprio; este registro não os conclui nem incorpora automaticamente suas implementações ao change de plugins externos.

| Pendência | Sugestão | Prós | Contras / dependência |
| --- | --- | --- | --- |
| Steam: pacote e aceite Linux | Concluir candidato empacotado e cenários reais de login, descoberta, falhas e encerramento; registrar confirmação do usuário exigida pelo plano. | Base real para regressões. | Ambiente e conta de teste utilizáveis. |
| Steam: aceite Windows | Após confirmação Linux, executar pacote identificado e roteiro para credenciais, pipes, tracking e ports. | Evidência atribuída ao candidato correto. | VM/máquina e interação nativa. |
| Package manager Linux | Concluir backends PackageKit/pacman e fluxo `ProviderPlugin`; distinguir sucesso vazio de falha; verificar exclusão no pacote Windows. | Estabiliza integração que compartilha infraestrutura. | Trabalho adicional de backend e empacotamento. |
| Extrator PS4 PKG | Avaliação isolada: licença compatível, execução local sem shell, versão fixável, cancelamento, suporte às entradas e resultado aceito pelo port. Selecionar apenas após evidência. | Evita depender de ferramenta inadequada. | Pesquisa e dumps adequados do usuário/desenvolvedor; nenhuma ferramenta foi escolhida neste registro. |
| Runner do extrator | Implementar após seleção, com argumentos estruturados, supervisão, saída limitada e validação do destino. | Contrato baseado em comportamento real. | Depende da avaliação anterior. |
| Aceite integrado de ports | Reutilizar roteiro entre ports e Steam, mantendo identidade do candidato e evidência por execução. | Evita duplicação e preserva rastreabilidade. | Mudanças relevantes exigem reexecutar cenários afetados. |

## Limites da revisão

Não foram executados novos testes de implementação para produzir este registro. A auditoria identifica pontos a reconciliar; não certifica que todo código existente satisfaz as specs. A autorização para publicar este documento mantém as sugestões como propostas e deixa pendentes sua resolução conjunta e a atualização normativa correspondente.

## D. Decisões adotadas na implementação da tarefa 5.1 (2026-11-14)

Confirmadas pelo usuário durante a tarefa 5.1 e implementadas apenas para `Library.List/Get/Watch`. Valem para as demais tarefas só onde dizem respeito a elas.

| ID | Decisão | Alcance |
| --- | --- | --- |
| D02 | Contexto de recurso emitido pelo host em `OpenSessionReply.resource_context` (API 1.6+), vinculado a sessão, tipo, jogo-alvo e perfil; não concede autoridade. Perfil ausente é `UNAVAILABLE`; contexto de perfil anterior é obsoleto. | Library em 5.1; extensões e renovação ficam em 8.1 e 14.5. |
| D03 | `Watch` vive enquanto sessão, perfil e concessões valem; cada entrega revalida e a mudança de concessão encerra o stream. Máximo de 8 watches por sessão. | Library em 5.1; demais Watch em 7.1 e 12.4. |
| D04 | `installation_id` aleatório (128 bits), persistido pelo host (`library_installations`), atribuído uma vez por entrada e nunca alterado. Entrada sem id durável não é projetada. | Uma instalação por entrada `<provider>:<store id>`. |
| D05 | Divisão de um item de catálogo em várias instalações continua em 14.2 e herdará esses ids. | Adiada para 14.2. |
| D06 | A instalação é o alvo; `game_id` sozinho só vale com exatamente uma instalação admitida; ambiguidade é `FAILED_PRECONDITION`; informar os dois exige correspondência. | Library.Get. |
| D08 | `library.read.all` libera apenas a projeção básica (provider, nome, estado, ids, revisão), sem caminhos, store id nem campos de varredura. | Library. |
| D09 | A projeção tipada de disponibilidade ROM/runtime não entra em 5.1; selecionar categoria ou registro comum em Library é recusado com `UNIMPLEMENTED`. | Adiada para 14.4 e 18.7. |
| D20 | Ordem por `installation_id`; cursor assinado (HMAC) ligado a consulta, sessão, perfil, revisão de concessões e revisão da biblioteca, com validade de 5 minutos; qualquer mudança da biblioteca invalida o cursor. Watch sem backlog por padrão, 256 eventos retidos, posição mais antiga exige nova listagem. | Library; demais listagens reaproveitam a política em suas tarefas. |

Efeito no protocolo: campo aditivo `resource_context = 7` em `OpenSessionReply`. Banco de dados: migração 10 (`library_installations`).

## E. Decisões adotadas na implementação da tarefa 5.2 (2026-11-14)

Confirmadas pelo usuário durante a tarefa 5.2 e implementadas para `Metadata.Update`, `Favorites.List` e `Favorites.Set`.

| ID | Decisão | Alcance |
| --- | --- | --- |
| D31 (parcial) | Só `display_name` é gravável (aparado, até 256 bytes, sem caracteres de controle; valor vazio restaura o nome detectado). Fica separado dos fatos detectados, é o título da linha da biblioteca (antes do nome do jogo associado e do nome detectado) e aparece em `LibraryEntry.display_name` e `metadata_revision`. Lista maior de campos, limites de artwork e redimensionamento continuam em aberto para 5.4. | 5.2; artwork em 5.4. |
| D07 | O favorito pertence ao jogo GameNode e nunca é movido por rematching. `FavoritesSet` exige entrada do próprio provider atualmente associada ao jogo (senão `NOT_FOUND`) e usa o mesmo caminho local-first da estrela do usuário, então a sincronização com o GameNode segue a configuração dele. A concessão `favorites.write` é o consentimento; sem confirmação por chamada. | 5.2; 5.3 cuida do rematching. |
| Revisão de favoritos | Favoritos não têm revisão: `expected_revision` deve ser 0 e as respostas trazem revisão 0; a escrita é de estado desejado, última gravação vence. **Desvio explícito** de revisões por recurso, porque o estado local não tem revisão e um booleano idempotente não tem o que conflitar. Revisitar se favoritos ganharem revisão persistida. | Favorites. |
| D21 | Chave de idempotência obrigatória (1 a 128 bytes). Tabela em memória por plugin e perfil (24 h, 1024 chaves, hash do pedido): mesma chave e mesmo pedido devolve o resultado original; mesma chave com outro pedido é `FAILED_PRECONDITION`; só resultados confirmados são lembrados. Perdida ao reiniciar; a repetição então é protegida pela revisão (metadados) ou pela idempotência natural (favoritos). Persistência em SQLite continua em aberto para efeitos duráveis maiores. | 5.2; demais mutações reaproveitam a política. |
| D12 | Inalterada: o token de intenção do usuário continua em aberto para ações de lançamento e instalação. | 7.2, 7.9, 10.7. |

Efeito no banco de dados: migração 11 (`library_display_overrides`, revisão por entrada que só cresce; limpar mantém a linha). Efeito no protocolo: campos aditivos `display_name = 8` e `metadata_revision = 9` em `LibraryEntry`.

## F. Decisões adotadas na implementação da tarefa 5.3 (2026-11-14)

Confirmadas pelo usuário durante a tarefa 5.3 (alvo, proteção da escolha do usuário e revisão de conflito) e implementadas para `Matching.Search` e `Matching.Assign`. Os demais itens abaixo são detalhes de engenharia adotados dentro do escopo aprovado, para revisão.

| ID | Decisão | Alcance |
| --- | --- | --- |
| D07 (conclusão) | O rematching por plugin nunca move o favorito: ele fica no jogo em que foi marcado. Corrigir um match por plugin não transfere favoritos. | 5.3. |
| Alvo da atribuição | `MatchingAssign` nomeia a entrada por `installation_id` (campo aditivo `installation_id = 5`); `provider_id` e `provider_game_id` ficam reservados e devem vir vazios. Uma única identidade de entrada em toda a API 1.6; entrada alheia ou desconhecida é `NOT_FOUND`, mesmo com `library.read.all`. | 5.3; 5.4 reaproveita. |
| Escolha do usuário protegida | A escolha gravada passa a registrar a origem (`user` ou `plugin`; migração 12, padrão `user`). Um plugin define ou troca só as próprias escolhas; trocar a escolha do usuário (inclusive "sem jogo") é `FAILED_PRECONDITION`. O usuário escolher de novo, mesmo o jogo que o plugin escolheu, torna a escolha dele. "Esquecer minha escolha" remove qualquer uma; o plugin não consegue limpar um match. **Desvio** do texto de design "escrita explícita do usuário": a concessão `matching.write` é o consentimento para o provider ajustar as próprias entradas, mas nunca sobrepõe o usuário. | 5.3. |
| Revisão de conflito | `expected_revision` é a revisão da entrada (`LibraryEntry.revision`), que o plugin já lê em List/Get. Divergência devolve `REVISION_CONFLICT` com a revisão atual e não grava; a revisão pode divergir por outro campo da entrada (o plugin relê e repete). Mesmo jogo já escolhido é `ALREADY_APPLIED` com a revisão atual, antes da checagem de revisão. A revisão da entrada é em memória (reinicia com o processo), como a projeção. | 5.3. |
| Busca limitada (detalhe de engenharia) | Consulta de 1 a 128 bytes; uma página de até 50 (padrão 20), sem cursor; `platform` e `confidence` reservados e vazios (o catálogo não tem plataforma nem pontuação); `release_year` aditivo (campo 5 em `MatchCandidate`); 20 buscas por minuto por plugin e perfil (`RESOURCE_EXHAUSTED`); sem sessão verificada ou online, ou falha do catálogo, `UNAVAILABLE`, nunca lista vazia. Os valores numéricos e a justificativa de cada um estão em "Limites de matching" abaixo e precisam de validação de capacidade. | 5.3. |
| Jogo atribuível (detalhe de engenharia) | Só um jogo que o host já conhece (registros de jogos da biblioteca) ou um dos últimos 256 devolvidos por buscas de plugin pode ser atribuído; outro é `NOT_FOUND`. O resultado de uma busca de plugin não entra na biblioteca nem no diálogo do usuário. | 5.3. |
| D12 | Inalterada. | 7.2, 7.9, 10.7. |

### Limites de matching e como foram decididos

Escolhidos pelo assistente por raciocínio sobre limites que o host já tem; **não houve medição nem pergunta ao usuário**. Detalhes em `docs/plugin-api-permissions.md#matching-limits`.

| Limite | Valor | Origem |
| --- | --- | --- |
| Consulta de busca | 1 a 128 bytes | Metade dos 256 caracteres do diálogo do usuário (`MAX_QUERY_LEN`) e igual ao limite de chaves de idempotência e de `provider_id`. |
| Resultados | padrão 20, máximo 50, uma página | A API do GameNode é consultada com `SEARCH_LIMIT = 20`, então 20 é o máximo prático hoje; 50 é folga. Abaixo do teto de 200 das listas de Library. Sem paginação no catálogo. |
| Buscas | 20 por 60 s, por plugin e perfil | Uma a cada 3 s em média: suficiente para casar uma tela de entradas e baixo o bastante para não virar rastreador em massa com a sessão do usuário. Valor sem medição. |
| Jogos atribuíveis oferecidos | 256, o mais antigo sai primeiro | Limita a memória por resultados de busca; mesmo número dos eventos retidos pelo watch de Library. |
| `installation_id` em Assign | até 128 bytes | Só rejeita abuso antes da consulta. |
| `release_year` | campo aditivo, 0 se desconhecido | Título sozinho não separa remakes; o catálogo já traz o ano. |

O limite de buscas por minuto é o mais sensível e o primeiro a revisar quando houver plugins reais.

Efeito no banco de dados: migração 12 (`chosen_matches.source`). Efeito no protocolo: campos aditivos `installation_id = 5` em `MatchingAssignRequest` e `release_year = 5` em `MatchCandidate`; contrato `endpoint_field_policy.json` atualizado.

## G. Decisões adotadas na implementação da tarefa 5.4 (2026-11-14)

Confirmadas pelo usuário durante a tarefa 5.4 (efeito do Set, biblioteca de imagem e leitura) e implementadas para `Artwork.Read` e `Artwork.Set`. Os limites numéricos abaixo são detalhe de engenharia adotado dentro do escopo aprovado, para revisão.

| ID | Decisão | Alcance |
| --- | --- | --- |
| D31 (conclusão) | `ArtworkSet` grava um **override da entrada** do próprio provider, nunca a capa do jogo GameNode (compartilhada por todas as entradas do jogo e substituída pelo próximo download). O override fica separado dos fatos detectados, vira a capa daquela entrada na biblioteca e é removido com `encoded_image` vazio (reset explícito, restaura a capa do jogo). Revisão por entrada que só cresce (`LibraryEntry.artwork_revision`, campo aditivo 10), no mesmo molde de `metadata_revision`. | 5.4. |
| Decodificação | Dependência nova: crate `image` (somente decodificadores PNG/JPEG/WebP e codificadores JPEG/PNG; Rust puro) em `gamenode-platform`. O host nunca guarda os bytes do plugin: decodifica, aplica a orientação EXIF, reduz e codifica de novo, então metadados e dados extras não chegam ao disco. Tudo em worker bloqueante. | 5.4. |
| Leitura | `ArtworkRead` devolve a capa que a entrada mostra: o override, senão a capa do jogo associado já baixada. `artwork_kind` só aceita `cover` (vazio vale `cover`). Nada é baixado pela leitura: sem capa disponível é `NOT_FOUND`. `max_bytes` é o orçamento do chamador; imagem maior que ele é `RESOURCE_EXHAUSTED`, sem truncar. | 5.4. |
| Alvo | `installation_id` do chamador, como em 5.2 e 5.3; entrada alheia ou desconhecida é `NOT_FOUND`, mesmo com `library.read.all`, e nenhuma imagem é decodificada para ela. `game_id`, se informado, deve ser o jogo atual da entrada. | 5.4. |
| Referências de recurso aprovadas | **Não implementado nesta tarefa.** O texto da 5.4 e o design falam em "bytes limitados ou handles de recurso aprovados", mas os handles de arquivo só passam a existir na 6.2 e `ArtworkSetRequest` não tem campo para eles. Hoje só bytes embutidos são aceitos, sem caminho, URL ou esquema. Aceitar handles de artwork é a tarefa 6.6, depois da 6.2 (campo aditivo); a 5.4 continua aberta até lá, por decisão do usuário. | 6.2, 6.6. |
| D21 | Reaproveitada: chave de idempotência obrigatória; uma chamada repetida é respondida antes de decodificar a imagem. | 5.4. |

### Limites de artwork e como foram decididos

Escolhidos pelo assistente por raciocínio sobre limites que o host já tem; **não houve medição nem pergunta ao usuário** (D31 já fixava os formatos e que bytes e pixels têm limite). Detalhes em `docs/plugin-api-permissions.md#artwork-limits`.

| Limite | Valor | Origem |
| --- | --- | --- |
| Imagem codificada | até 4 MiB | As mensagens de plugin têm teto de 8 MiB; deixa folga para o pedido. Os downloads de capa do host têm teto de 5 MiB. |
| Pixels da origem | até 4096 x 4096 (16.777.216), nenhum lado acima de 8192 | Capa aparece com algumas centenas de pixels. As dimensões vêm do cabeçalho e são checadas antes de decodificar, então arquivo pequeno que expande num bitmap enorme é recusado sem alocar. |
| Memória de decodificação | limite de alocação de 128 MiB; duas decodificações por vez | RGBA16 de 16 megapixels ocupa 128 MiB; duas bastam e limitam a memória por imagens de plugin a ~256 MiB. |
| Tamanho gravado | cabe em 600 x 900, proporção mantida, sem ampliar | Cerca do dobro da largura da grade, para telas de alta densidade; proporção 2:3 como as capas dos jogos. Cada entrada guarda no máximo um arquivo. |
| Formato de saída | JPEG (qualidade 88); PNG se algum pixel for transparente | Capas são JPEG no resto do host; JPEG perderia a transparência. |
| Formatos aceitos | PNG, JPEG e WebP estáticos | De D31. A assinatura decide e o `media_type` declarado deve concordar; APNG e WebP animado são recusados. |
| Orçamento de leitura | `max_bytes` 0 vale 5 MiB, com teto de 5 MiB | Maior capa que o host guarda (teto do download). |

Sem limite de taxa por plugin em `ArtworkSet`: as escritas são serializadas, no máximo duas imagens são decodificadas ao mesmo tempo e uma repetição não decodifica nada, o que limita CPU e memória mas não a frequência de troca. Revisar junto dos demais limites de taxa quando houver plugins reais.

Efeito no banco de dados: migração 13 (`library_artwork_overrides`, arquivo por entrada em `covers/` como `plugin-<entrada>-<conteúdo>.jpg|png`). Efeito no protocolo: campo aditivo `artwork_revision = 10` em `LibraryEntry`; contrato `endpoint_field_policy.json` atualizado.

## H. Decisões adotadas na implementação da tarefa 5.5 (2026-11-14)

Detalhes de engenharia dentro do escopo da 5.5, para revisão; nenhuma mudança de protocolo ou de banco.

| Decisão | Alcance |
| --- | --- |
| Os auxiliares do SDK Rust (`HostClient::resources()` → `Resources`) enviam exatamente o pedido que a chamada bruta enviaria e devolvem a resposta do host sem alteração. Erros são o `Status` do próprio host, não o `Error` legado do SDK, porque este esconde códigos que o plugin precisa distinguir (`NOT_FOUND`, `FAILED_PRECONDITION` da escolha do usuário, `RESOURCE_EXHAUSTED`). | 5.5. |
| A chave de idempotência é do plugin: `idempotency_key()` gera uma por mudança pretendida, a mesma chave vale para reenviar a mesma mudança, e conflito de revisão é resposta (`Outcome::RevisionConflict`), não erro. | 5.5. |
| Equivalência SDK × gRPC bruto é verificada nos testes de serviço do host de teste: leituras comparadas diretamente, escritas reenviadas cruas sob a chave do SDK (o host só devolve o resultado original para um pedido idêntico) e recusas comparadas por código e mensagem. | 5.5. |
| As receitas são código compilado (`crates/plugin-sdk/examples/library_recipes.rs`) e executado nesses mesmos testes, não trechos soltos na documentação. | 5.5. |
| O SDK é só Rust; clientes Go usam o cliente gerado com as mesmas regras (o exemplo Go não tem SDK). | 5.5. |

## I. Decisões adotadas na implementação da tarefa 6.1 (2026-11-14)

Confirmadas pelo usuário durante a tarefa 6.1 (onde guardar o durável, vida do dado de sessão e TTL) e implementadas para `Storage.Get`, `Storage.Put` e `Storage.Delete`. Os limites numéricos que não vêm do design são detalhe de engenharia, para revisão.

| ID | Decisão | Alcance |
| --- | --- | --- |
| Backend durável | Um arquivo SQLite por plugin e perfil de conta em `plugin-data/<fingerprint>/<plugin>/<perfil>/storage.sqlite3`, como no layout do design. Quota e revisão são checadas na mesma transação, então escrita acima da quota não muda nada, e apagar o namespace é apagar a pasta. Nomes de pasta são a identidade com caracteres inseguros em `%XX`, ou um resumo se ficarem longos demais. Leitura nunca cria arquivo. | 6.1. |
| `SESSION_ONLY` | Memória do host, por namespace; descartado quando a última sessão aberta do plugin termina ou o perfil muda. Todas as sessões abertas do mesmo plugin compartilham o espaço, então uma janela de configuração enxerga o que o provider gravou. Só exige `storage.write`. | 6.1. |
| Durabilidade explícita | `UNSPECIFIED` é `INVALID_ARGUMENT`. `PERSISTENT` sem `storage.persist` é `PERMISSION_DENIED` e não grava nada, nem em memória: nunca vira sucesso volátil. Ler e apagar valores já gravados não exigem `storage.persist`, então revogá-lo só impede novas escritas duráveis, sem esconder nem prender dados existentes. | 6.1; design §16. |
| Uma camada por chave | A chave vive em uma camada por vez; gravar com a outra durabilidade move o valor e remove a cópia anterior. Os dois espaços compartilham o conjunto de chaves e `StorageGetReply.durability` informa onde está. | 6.1. |
| Revisões | Contador único por namespace (mantido em memória e gravado no arquivo durável como maior revisão usada): chave apagada e recriada nunca repete revisão, nem ao mover entre camadas ou reiniciar. `expected_revision` deve ser a revisão atual (0 para chave inexistente). Apagar chave ausente é `ALREADY_APPLIED`, revisão 0, qualquer que seja o `expected_revision`. | 6.1. |
| TTL e resposta | `ttl_seconds` 0 não expira; máximo de 30 dias. Campos aditivos: `durability` e `expires_at` (segundos Unix, 0 sem expiração) em `StorageGetReply` e `StorageMutationReply`. Valor vencido é chave inexistente e libera espaço. | 6.1; design 16 ("durability, expiry and owner"). |
| Idempotência e commit | Mesmas regras de 5.2 a 5.4 (chave obrigatória, nova checagem de permissão e revisão de grants antes da primeira mudança). A fila de serialização é por namespace, não a global das mutações de biblioteca, para um disco lento de um plugin não travar os outros. | 6.1. |
| Secret x Storage | Storage não é cifrado nem sensível; credenciais seguem pelo serviço `Secret`. Documentado. | 6.1. |

### Limites de storage e como foram decididos

Valor máximo e quota vêm do design (1 MiB, 16 MiB). Os demais foram escolhidos pelo assistente, **sem medição nem pergunta ao usuário** (o TTL máximo de 30 dias foi a opção recomendada confirmada). Detalhes em `docs/plugin-api-permissions.md#storage-limits`.

| Limite | Valor | Origem |
| --- | --- | --- |
| Valor | até 1 MiB | Design. |
| Quota | 16 MiB de bytes lógicos por camada e namespace | Design. O total gerenciado de 64 MiB do design cobre também dados de outros serviços, ainda não implementados. |
| Chave | 1 a 256 bytes, sem caracteres de controle | Assistente: chaves são nomes; o limite mantém o índice pequeno. |
| Sobrecarga por entrada | 64 bytes cobrados além de chave e valor | Assistente: valor vazio não pode ser grátis, ou um plugin guardaria milhões de chaves sem pagar; com isso cabem no máximo ~260 mil entradas. |
| TTL | até 30 dias | Opção confirmada pelo usuário. |
| Camadas | quota separada por camada | Assistente: a camada de sessão é memória do host, então 16 MiB por plugin é o máximo que ela ocupa. |

A quota conta bytes lógicos, não o tamanho do arquivo (páginas e índices do SQLite pesam mais). Não há limite de taxa por plugin para escritas. Faltam, em tarefas próprias, a interface de uso e limpeza, a escolha de manter ou apagar na desinstalação (16.4, 17.2) e os métodos `Persistence*` (16.2). A 16.1 (stores duráveis por dono com respostas explícitas de durabilidade e expiração) ficou parcialmente coberta por esta tarefa: restam o consentimento de retenção do receptor e o armazenamento de dados compartilhados.

Efeito no protocolo: campos aditivos `durability` e `expires_at` nas respostas do Storage; contrato `endpoint_field_policy.json` atualizado. Sem migração do banco principal.

## J. Decisões adotadas na implementação da tarefa 6.2 (2026-11-14)

Confirmadas pelo usuário durante a tarefa 6.2 (como a aprovação acontece, quanto ela dura, como criar arquivos e como tratar pastas compartilhadas) e implementadas para `Files.RequestAccess`, `List`, `Read`, `Write` e `Delete`. Os limites numéricos não vêm do design, para revisão.

| ID | Decisão | Alcance |
| --- | --- | --- |
| Aprovação do usuário | `FilesRequestAccess` chama uma porta do host, `RootApprover` (`choose_root` e `confirm`, assíncrona, com 5 minutos para responder), que recebe plugin, instalação, tipo, operações e a pasta sugerida e devolve a pasta escolhida; depois o usuário confirma o escopo final (pasta, operações e todos os jogos afetados). O seletor nativo só existe no Windows (no Linux devolve `Unsupported`) e a UI de configurações é a seção 10, então **a porta está pronta, mas nenhum aprovador está ligado no app**: uma nova aprovação responde `FAILED_PRECONDITION` até a seção 10 chamar `FileAccess::set_approver`. Aprovações já gravadas funcionam. | 6.2, 10.x. |
| Persistência | A aprovação (plugin + instalação + tipo + pasta + volume/nó + operações + jogos afetados) é gravada por perfil de conta, no mesmo arquivo SQLite do storage do plugin (tabela `file_roots`, esquema 2), sem contar na quota. Vale até ser revogada, até o grant `files.*` sair ou até o conjunto de jogos da pasta mudar. Cada sessão recebe handles novos para a mesma aprovação, sem novo pedido; pedir mais operações pergunta de novo. | 6.2. |
| Handles | Tokens opacos, selados (ChaCha20-Poly1305 com chave aleatória do processo), ligados à sessão e ao propósito: carregam aprovação e caminho relativo, e não podem ser lidos, forjados, editados, reaproveitados por outra sessão ou usados como cursor. Malformado, alheio, editado ou revogado é sempre `NOT_FOUND`. | 6.2. |
| Criar arquivo | Campo aditivo `FilesWriteRequest.name = 6`: com `name`, `file_handle` é um handle de diretório e o arquivo é criado ou sobrescrito dentro dele; nomes novos têm de ser portáveis (sem separadores, `.`/`..`, `< > : " \| ? *`, ponto/espaço no fim ou nome de dispositivo). `resource_id` da resposta é o handle do arquivo. Delete remove só arquivos regulares, nunca diretórios. Arquivos não têm revisão (`expected_revision` deve ser 0). | 6.2. |
| Pasta compartilhada | Jogos afetados = entradas da biblioteca, de qualquer provider, cuja pasta de instalação é a raiz, está dentro dela ou a contém (links resolvidos). O aprovador recebe a lista completa; a aprovação grava o conjunto e deixa de valer se um jogo entrar ou sair da pasta, até nova aprovação. Um pai aprovado só para leitura não permite escrita. | 6.2; design §17. |
| Pastas reservadas | A pasta de dados do aplicativo é reservada: não pode ser aprovada nem estar dentro de uma raiz aprovada que a exclua do alcance; quando um pai aprovado a contém, ela não é listada e nada abaixo dela é alcançável. | 6.2. |
| Confinamento | Linux: `openat` com `O_NOFOLLOW` a partir de um descritor de diretório, sem janela entre checar e usar. Windows: cada componente é checado como ponto de reanálise e o caminho final do handle aberto deve ser exatamente a raiz mais os componentes. Links, junções, outros volumes e arquivos especiais não existem para o plugin. | 6.2. |
| Aprovação só para entradas próprias | Só entradas da biblioteca do próprio provider podem ser alvo (igual a 5.2 a 5.4). Saves e prefixos sem entrada de biblioteca ficam para os serviços de perfis de save e locais. | 6.2, 13.x. |
| Serialização | Toda a chamada de arquivo roda sob o lock do namespace do plugin (o mesmo do Storage), então revogação não passa entre a checagem e a mudança; em troca, o Storage do mesmo plugin espera uma chamada de arquivo em andamento. Mutações do plugin são serializadas. | 6.2. |

### Limites de arquivos e como foram decididos

Escolhidos pelo assistente por raciocínio, **sem medição e sem pergunta ao usuário** (o design não fixa números). Detalhes em `docs/plugin-api-permissions.md#file-limits`.

| Limite | Valor | Origem |
| --- | --- | --- |
| Bytes por leitura ou escrita | até 4 MiB (padrão de leitura 1 MiB) | Mensagens de plugin têm teto de 8 MiB; arquivos maiores vão em partes. |
| Tamanho de arquivo que o plugin pode fazer crescer | 512 MiB | Saves e memory cards são pequenos; impede encher um disco numa cadeia de chamadas. Não limita o total escrito. |
| Entradas lidas de um diretório | 20.000 | Cada página ordena o diretório inteiro, então o trabalho por página tem teto; diretório maior é recusado, não cortado. |
| Página de listagem | padrão 100, máximo 200 | Igual aos outros serviços paginados. |
| Profundidade e nome | 64 componentes, 255 bytes | Limites comuns de sistemas de arquivos. |
| Caminho num handle | até 2.048 bytes | Entradas mais profundas não são listadas. |
| Tempo para responder a aprovação | 5 minutos | Longo o bastante para achar uma pasta. |
| Cache de pastas canônicas da biblioteca | 60 segundos | A checagem de compartilhamento roda a cada chamada. |

Sem limite de taxa por plugin. **Não verificado em execução:** o backend Windows foi só verificado quanto a tipos para o alvo Windows (as dependências em C não compilam neste ambiente); precisa de aceitação nativa. A checagem de compartilhamento só conhece as pastas de instalação da biblioteca: uma pasta de save fora delas pode ser usada por jogos que o host não vê.

Efeito no banco: esquema 2 do arquivo SQLite por plugin (`file_roots`), sem mudança no banco principal. Efeito no protocolo: campo aditivo `FilesWriteRequest.name = 6`; contrato `endpoint_field_policy.json` atualizado.

## K. Decisões adotadas na implementação da tarefa 6.3 (2026-11-14)

O design já fixava o essencial de `Network.Fetch`: origens HTTPS exatas, resposta de até 8 MiB, prazo de até 30 s, revalidação de redirecionamentos e DNS, negação de loopback, redes privadas e link-local, e nenhuma credencial do GameNode. Não foi feita pergunta ao usuário nesta tarefa; as decisões abaixo são detalhe de engenharia dentro desse escopo, para revisão.

| Decisão | Alcance |
| --- | --- |
| **Origens aprovadas vêm de uma porta (`OriginScopes`)**, consultada de novo a cada fetch e a cada redirecionamento, no mesmo molde do aprovador de pastas da 6.2. Nada é aprovado por padrão. **Nenhuma implementação está ligada ao app**: o armazenamento de permissões ainda não é consultado por sessões de terceiros, então todo fetch responde `PERMISSION_DENIED` até a ativação (seção 8/10) chamar `NetworkAccess::set_scopes`. | 6.3, 8.x, 10.x. |
| Origem é esquema, host e porta exatos (porta padrão omitida, host em minúsculas, punycode); sem curingas, caminhos ou prefixos. Hosts com caracteres que não pertencem a um nome (como `*`) não formam origem. | 6.3. |
| O host resolve o nome com o próprio resolvedor do cliente e **recusa a conexão se qualquer resposta for não pública** (loopback, privada, link-local, CGNAT, local única/site-local IPv6, multicast, não especificada, documentação, benchmarking, reservada, Teredo, e formas IPv4-mapped, NAT64 e 6to4 que embutem um IPv4 proibido). Conecta só nos endereços que passaram, então o nome não pode passar e depois apontar para outro lugar. URLs com endereço literal (inclusive `2130706433` e `0x7f.1`) passam pelo mesmo teste. Vale também para origens aprovadas. | 6.3. |
| Redirecionamentos são seguidos pelo host, no máximo 5, cada alvo checado contra as origens aprovadas antes de qualquer envio; o plugin nunca sabe para onde apontava e o corpo do redirecionamento nunca é lido. `Authorization`, `Cookie` e `Proxy-Authorization` do plugin não atravessam origens. | 6.3. |
| Sem pool de conexões, sem cookies e **sem proxy do sistema** (um proxy resolveria os nomes e contornaria a checagem de endereços). Redes que só saem por proxy não são suportadas. | 6.3. |
| O orçamento conta bytes do fio: o host não descomprime. Um único prazo cobre redirecionamentos e corpo. | 6.3. |

### Limites de rede e como foram decididos

Resposta de 8 MiB, prazo de 30 s e a exigência de origens exatas vêm do design; o resto foi escolhido pelo assistente por raciocínio, **sem medição**. Detalhes em `docs/plugin-api-permissions.md#network-limits`.

| Limite | Valor | Origem |
| --- | --- | --- |
| Resposta | teto de 8 MiB, padrão 1 MiB | Teto do design; o padrão evita chamadas descuidadas grandes. |
| Prazo | teto e padrão de 30 s | Design. |
| Redirecionamentos | 5 | Assistente: navegadores aceitam 20, fluxos de API raramente passam de dois, e cada salto é outra checagem e conexão. |
| Corpo do pedido | 1 MiB | Assistente: pedidos são chamadas de API pequenas. |
| URL, cabeçalhos | 2.048 bytes; 32 cabeçalhos de até 4.096 bytes | Assistente: limites comuns de servidores. |
| Cabeçalhos devolvidos | 64, 32 KiB | Assistente. |
| Fetches simultâneos | 4 por plugin, 16 no total, sem fila | Assistente: limita sockets e memória; excesso é `RESOURCE_EXHAUSTED`. |

Sem limite de taxa nem cota total de bytes por plugin. **Não verificado em execução:** um fetch por TLS real a um servidor público (o ambiente de teste só alcança servidores locais, com a checagem de endereço relaxada, mais o cliente estrito real contra os destinos que ele deve recusar).

Efeito no protocolo: nenhum campo novo; `endpoint_field_policy.json` atualizado. Sem mudança de banco.

## L. Decisões adotadas na implementação da tarefa 6.4 (2026-11-14)

Confirmadas pelo usuário durante a tarefa 6.4 (como funciona o fallback de idioma e até onde a tarefa vai); o resto é detalhe de engenharia dentro do escopo do design (texto simples, limitado, atribuído ao plugin, com limite de taxa, sem script nem links).

| Decisão | Alcance |
| --- | --- |
| **Traduções opcionais.** Campo aditivo `NotificationPublishRequest.translations` (até 8, cada uma com `locale` próprio e não vazio; um locale aparece uma vez, inclusive o do texto principal). O fallback é feito pela UI, que conhece o idioma do usuário: locale exato, depois o mesmo idioma (a etiqueta sem região primeiro), depois o texto principal. Maiúsculas e minúsculas não importam. | 6.4. |
| **Toast atribuído, de ponta a ponta.** O host entrega a um `NotificationSink`; o app o liga a um evento do Tauri (`plugin:notification`); a UI mostra um toast de 8 s com "Do plugin `<id>`" e o texto como texto puro. Sem centro de notificações, sem histórico e sem silenciar por plugin. É evento, não estado: sem janela escutando, o aviso se perde. | 6.4. |
| Atribuição é o id do plugin autenticado; não há campo no pedido para nome, ícone ou remetente. O rótulo é o id (reverso-domínio), não o nome de exibição, que o `Registration` ainda não carrega. | 6.4. |
| Texto: o host recusa caracteres de controle (exceto quebras de linha no corpo) e caracteres que disfarçam texto (sobreposições e isolados bidirecionais, espaço de largura zero, junção de palavra, BOM, separadores de linha e parágrafo, anotação intercalar e caracteres de tag). Junções de emoji e de escritas que precisam delas são permitidas. A UI remove tudo isso de novo antes de exibir (defesa em profundidade). O host nunca interpreta o texto; marcação, Markdown, `javascript:` e URLs aparecem como os caracteres que são, sem link. | 6.4. |
| Ações: o design cita "identificadores de ação do host permitidos", mas o protocolo não tem campo para isso e não há ação de host definida; **não há ações nesta versão**. | 6.4. |
| Limite de taxa: janela deslizante de 60 s, 5 por plugin e 20 no total; excesso é `RESOURCE_EXHAUSTED` com a espera na mensagem e em `retry-after-ms`, não conta nada e não é lembrado (a mesma chave pode ser reenviada depois). Idempotência igual às outras mutações. | 6.4. |

### Limites de notificação e como foram decididos

Escolhidos pelo assistente por raciocínio, **sem medição**; o design só fixa "texto simples limitado, com limite de taxa". Detalhes em `docs/plugin-api-permissions.md#notification-limits`.

| Limite | Valor | Origem |
| --- | --- | --- |
| Título | 120 bytes, uma linha | Assistente: um toast é uma linha. |
| Corpo | 512 bytes, 8 linhas | Assistente: uma ou duas frases. |
| Traduções | 8 | Assistente. |
| Taxa | 5 por plugin e 20 no total por 60 s | Assistente: relata progresso e resultado sem enterrar a janela; o teto comum limita uma multidão de plugins. |
| Duração do toast | 8 s (os do Core duram 4 s) | Assistente: tem corpo para ler. |

**Não verificado visualmente:** o toast foi coberto por testes de componente (texto hostil vira texto, sem `a`, `script` ou `img`), mas não olhado num navegador nem no app nativo. Efeito no protocolo: campos aditivos `NotificationTranslation` e `translations = 6`; `endpoint_field_policy.json` atualizado. Sem mudança de banco.

## M. Decisões adotadas na implementação da tarefa 6.6 (2026-11-14)

Sem perguntas ao usuário: o design e a tarefa já fixam o essencial (campo aditivo ao lado dos bytes inline, handles da 6.2, mesmos limites). O resto é detalhe de engenharia.

| Decisão | Alcance |
| --- | --- |
| Campo aditivo `ArtworkSetRequest.file_handle = 7`, exclusivo com `encoded_image` (os dois juntos são `INVALID_ARGUMENT`). `media_type` é obrigatório com o handle, como com bytes inline, e precisa combinar com os bytes. | 6.6. |
| Precisa de `files.read` além de `artwork.write`; sem ele nada é lido. `DescribeApi` lista uma permissão por método e continua listando só `artwork.write`; a exigência de `files.read` para o handle está no protocolo e na documentação. | 6.6. |
| O handle tem de ser de um arquivo regular de uma aprovação vigente **da mesma instalação** alvo. O handle da aprovação de outra entrada do próprio plugin é `PERMISSION_DENIED`: uma pasta que o usuário aprovou para outro jogo não serve de fonte de capa para este. Diretório: `FAILED_PRECONDITION`. Caminho, URL, handle forjado, editado, de outra sessão ou de aprovação revogada: `NOT_FOUND`. | 6.6. |
| O host lê o arquivo (até 4 MiB + 1 byte, só para distinguir "maior que o limite") no worker do namespace do plugin e depois decodifica no worker de imagens, com os mesmos limites de pixels e formatos. A mensagem de arquivo grande é a mesma dos bytes inline. | 6.6. |
| Idempotência: o hash do pedido usa o handle e o tipo declarado (os bytes só são conhecidos depois da leitura, e a repetição é respondida antes). Um arquivo que mudou depois não muda a resposta à mesma chave; outro arquivo precisa de outra chave. `files.read` é verificado de novo no commit, antes de `artwork.write`, para a recusa nomear a permissão perdida. | 6.6. |
| Não há limite de taxa próprio, como nas capas inline. O SDK Rust ganha `Resources::set_cover_from_file`. | 6.6. |

## N. Decisões adotadas na implementação da tarefa 6.7 (2026-11-14)

Confirmadas pelo usuário durante a tarefa 6.7 (as sete primeiras linhas); o resto é detalhe de engenharia.

| Decisão | Alcance |
| --- | --- |
| **Sugestão de outro sistema é descartada em silêncio.** Placeholder do outro SO (`{appdata}`/`{localappdata}` no Linux, `{xdg_*}` no Windows), caminho de unidade ou compartilhamento do Windows no Linux, caminho Unix no Windows, ou pasta do sistema que não existe (`{documents}` sem XDG) não é erro: o plugin lista todas as plataformas numa só lista. Placeholder desconhecido, caminho relativo, `..`, texto vazio, longo ou com controle, mais de 8 por tipo e tipo inválido recusam o pedido (`INVALID_ARGUMENT`), antes de perguntar ao usuário. | 6.7. |
| **Caminho absoluto literal é aceito**, como candidato não verificado igual aos demais. `/` e `\` separam nomes. | 6.7. |
| **Mais de 8 candidatos por tipo recusam o pedido**, para a prioridade não ser alterada em silêncio. | 6.7. |
| **Descrição é texto puro** (não Markdown): uma linha, até 120 bytes, mesmas regras de caracteres das notificações (6.4). Está no diálogo de consentimento, a pior superfície para disfarce, e formatação enfraqueceria o rótulo "declarado pelo plugin, não verificado". Markdown poderia vir depois como campo aditivo próprio, com revisão de segurança. | 6.7. |
| **A origem é decidida pelo host**, comparando o caminho canônico escolhido com o dos candidatos que existem; o aprovador não alega "declarado". Valores: `declared` e `user` (a pasta que o host sugere para uma instalação conta como `user`). Coluna nova `origin` no schema v3 do armazenamento (aprovações antigas viram `user`). | 6.7. |
| **Aprovação vigente que já cobre o pedido ignora a lista declarada**, sem pergunta nova; a lista malformada continua sendo recusada mesmo assim. Ampliar de leitura para escrita pergunta de novo, com as sugestões de novo. | 6.7. |
| **Só `save` e `prefix` aceitam sugestão**; `installation` com sugestão é `INVALID_ARGUMENT`, porque o host sabe onde a instalação está. A raiz de descoberta de jogos (por exemplo a biblioteca padrão de uma loja) é outro conceito: não é aprovação de pasta de uma entrada, e foi registrada na tarefa 14.1. | 6.7, 14.1. |
| Pasta reservada do host (dados privados do GameNode) nunca é oferecida e é descartada sem erro. Duplicatas (mesma pasta escrita de outro jeito) colapsam, mantendo a primeira. Declarar para um tipo que o pedido não pede é ignorado. | 6.7. |
| A pré-seleção é a sugestão própria do host (instalação), senão a primeira declarada que existe; a verificação é só de existência, nada é listado nem lido, e nada volta ao plugin. O diálogo real é a 10.9. | 6.7, 10.9. |

