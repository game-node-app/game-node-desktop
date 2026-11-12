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
| D24 | Perfil local e ausência de login | Perfil local explícito e separado dos perfis autenticados. Funções locais autorizadas funcionam sem login; favoritos online exigem conta. Troca invalida contextos e não move dados automaticamente. | Uso offline previsível e isolamento. | Exige política explícita de importação/migração entre perfis; afeta comportamento do produto. | 4.4, 5.2, 8.5, 13.1, 16.1 |
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

D24, D25, D28 e a experiência de múltiplas instalações em D05–D06 merecem revisão de produto explícita. Detalhes de implementação podem ser resolvidos tecnicamente dentro das diretrizes que forem aprovadas.

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
