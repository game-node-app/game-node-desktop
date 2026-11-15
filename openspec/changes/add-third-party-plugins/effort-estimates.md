# Estimativa de esforço — add-third-party-plugins

Estimativa original para as **73 tarefas que estavam pendentes no momento da divisão**, agora distribuídas entre um MVP e três follow-ups. Após concluir 8.1, restam 21 tarefas no MVP e 51 nos follow-ups. A tabela conserva as faixas originais por tarefa; [`delivery-split.md`](delivery-split.md) define a ordem, o escopo e a propriedade atual.

## Reestimativa do MVP — 2026-11-15

Antes desta execução, as 22 tarefas abertas do MVP somavam **58,5–121,5 horas** na estimativa original por tarefa. A tarefa 8.1 aproveitou o suporte a sessões de extensão já existente no protocolo/SDK e foi concluída com a ponte de registro do host, sem recriar o fluxo de launch. A estimativa de 1–3 horas restantes para 8.1 foi consumida nesta execução.

Com 8.1 concluída, o esforço restante estimado para as 21 tarefas do MVP é **55,5–115,5 horas de foco** (aproximadamente **7–15 dias de 8 horas**). A decomposição é:

| Frente do MVP | Tarefas | Esforço restante |
|---|---|---:|
| Composição e ciclo de vida | 8.2, 8.3, 8.7 | 7,5–15,5 h |
| Recuperação de atualização | 9.3 | 3–6 h |
| Interface de gestão e compatibilidade | 10.1, 10.2, 10.4, 10.5, 10.8 | 10–22 h |
| SDK, exemplos e conformidade | 11.1, 11.2, 11.3, 11.5, 11.6, 11.7, 11.9 | 17–36 h |
| Verificação focada e aceitação | 18.1, 18.2, 18.3, 18.4, 18.10 | 18–36 h |

As tarefas 18.2 e 18.3 representam **10–20 horas de aceitação nativa** no Linux e Windows, já incluídas no total. A estimativa conta o tempo de execução e registro dos cenários; disponibilidade de máquinas, contas, builds e correção de falhas pode aumentar o prazo de calendário. O intervalo também é de horas de foco, não de execução contínua. A revisão de escopo reduziu 8.1; as outras faixas continuam como antes porque a conversa não identificou mudanças que justifiquem reduzi-las.

## Como interpretar

- As horas são **horas de foco de implementação**, incluindo leitura do escopo, código/documentação e verificação focada. Não são prazo de calendário.
- Os registros históricos desta thread mostram aproximadamente: 7.3 ≈ 52 min; 7.4 ≈ 17 min; 7.7 ≈ 38 min; 9.1 ≈ 45 min; 9.2 ≈ 42 min. A 7.8 passou de 75 min e ainda não terminou. Esses tempos incluem investigação, validação e pausas entre interações, então servem como calibração ampla, não como velocidade líquida.
- Os registros de thread disponíveis **não incluem tokens consumidos por tarefa**. Para evitar números inventados, “carga” é um proxy de contexto e coordenação: baixa/média para escopo local, média/alta para múltiplos crates, contratos de segurança ou corrida de estado.
- As estimativas pressupõem que as bases anteriores e decisões do OpenSpec estão disponíveis. Tarefas 18.2/18.3 dependem de acesso aos ambientes Linux/Windows; espera por máquinas, contas e hardware pode aumentar muito o tempo de calendário.
- A soma é aproximadamente **224,5–459 horas de foco** (cerca de 29–58 dias de 8 h). A largura da faixa reflete integração e retrabalho; paralelismo reduz calendário, não esforço total.

## Modelo recomendado

- **Luna**: documentação, exemplos determinísticos, fixtures ou mudanças localizadas com critérios claros.
- **Sol**: recomendado para integração entre crates/UI/protocolo, autorização, persistência, concorrência e testes de regressão. A documentação oficial posiciona Sol para tarefas complexas equilibrando qualidade, tempo e custo; Luna para tarefas delimitadas e frequentes.
- **Astra**: não é necessário para a maioria da implementação. Consideraria uma revisão de arquitetura antes de codificar **7.11, 10.7, 10.10, 12.4, 14.2, 14.3, 16.5 e 19.3**, pois combinam vários componentes, decisões de segurança e efeitos difíceis de reverter. Depois, Sol pode implementar com o escopo fechado.

São recomendações para comparar, não garantia de desempenho. Use o modelo mais leve que passe os mesmos testes e revise os mesmos critérios. Referência: [guia oficial de seleção de modelos](https://developers.openai.com/api/docs/guides/model-selection).

## Estimativa por tarefa

| Tarefa | Escopo (resumo) | Foco estimado | Carga de contexto | Modelo sugerido |
|---|---|---:|---|---|
| 7.8 | Implement game.custom_launch recipe propose/request/remove with alternate executable/URI, argument vectors, working directory | 1,5–4 h | Média/alta | Sol |
| 7.9 | Implement per-game/profile plugin-owned overlays for official, third-party and manual games, with independent normal/custom g | 2–4 h | Média/alta | Sol |
| 7.10 | Fence approvals to recipe/base-target/resource revisions, sanitize child environment and associate wrapper/child process attr | 3–6 h | Média/alta | Sol |
| 7.11 | Build immutable typed LaunchPlan resolution and versioned canonical approval fingerprints, binding source/package/component i | 5–10 h | Alta | Sol (Astra para revisão de arquitetura se houver conflito) |
| 8.1 | Compose external provider backends and provider-free launch extensions beside the official registry, reserving official ids/n | 3–6 h | Média/alta | Sol |
| 8.2 | Add external secret/account/login service composition using the 4.4 namespace resolver, with dedicated logical secret slots,  | 3–6 h | Média/alta | Sol |
| 8.3 | Implement start/enable/disable/remove and bounded process-tree shutdown with durable per-account enabled state and session fe | 4–8 h | Média/alta | Sol |
| 8.4 | Add ui.configure and correlated Open/Focus/Close lifecycle results with one window handle per plugin/profile | 4–8 h | Média/alta | Sol |
| 8.5 | Handle window closure/fencing on profile change (including sign-out, session rejection and a different verified account), rev | 2–4 h | Média/alta | Sol |
| 8.6 | Connect optional declarative ui.settings rendering with its existing closed schema and explicit authorization, preferring ext | 2–5 h | Média/alta | Sol |
| 8.7 | Document lifecycle, process-supervision limits and plugin-owned UI/storage responsibilities | 0,5–1,5 h | Baixa/média | Luna |
| 8.8 | Connect the approved-origin source of 6.3 (`NetworkAccess::set_scopes`) to the persisted per-account permission store for thi | 2–5 h | Média/alta | Sol |
| 9.3 | Implement staged activation, readiness timeout and last-known-good rollback using current grants | 3–6 h | Média/alta | Sol |
| 9.4 | Implement explicit downgrade and publisher-key re-trust review, with versioned plugin-private data policy | 2–4 h | Média/alta | Sol |
| 10.1 | Add asynchronous Tauri commands and React Query inventory/permission/update/configuration cache updates | 3–6 h | Média/alta | Sol |
| 10.2 | Extend Stores PluginInventory with import, activation, disable, permission scopes and removal using shared controls/modals | 3–6 h | Média/alta | Sol |
| 10.3 | Add Configure routing and lifecycle status, with external-window preference and optional form fallback | 2–4 h | Média/alta | Sol |
| 10.4 | Add publisher fingerprint, requested/granted/effective access, native-code consent and offline license viewers | 1–3 h | Baixa/média | Luna |
| 10.5 | Add update diff, failure and rollback status plus complete en/pt-BR labels and mocks | 1–3 h | Baixa/média | Luna |
| 10.6 | Add per-game launch recipe selection and explicit return to detected default using the shared host launch-plan review for run | 3–6 h | Média/alta | Sol |
| 10.7 | Implement the shared Launch plan / Plano de lançamento review with source provenance, ordered known components, complete lite | 5–10 h | Alta | Sol (Astra para revisão de arquitetura se houver conflito) |
| 10.8 | Show target requirements, detected effective environment, compatibility verdict/reasons and separate author-tested coverage i | 2–4 h | Baixa/média | Luna |
| 10.9 | Connect the folder-approval port of 6.2/6.7 to the real app: implement `RootApprover` for the engine with asynchronous Tauri  | 5–10 h | Média/alta | Sol |
| 10.10 | Implement the host-issued user intents of design section 20 for `GamesRequestLaunch` and `GamesRequestInstall`: an in-memory  | 6–12 h | Alta | Sol (Astra para revisão de arquitetura se houver conflito) |
| 11.1 | Prepare versioned public SDK/protocol source archives with usable external dependency metadata and pinned tooling | 3–6 h | Baixa/média | Luna |
| 11.2 | Create examples/plugins/rust-sdk as a runnable separate-identity MIT plugin using the SDK, including provider and launch-exte | 2–5 h | Média/alta | Sol |
| 11.3 | Create examples/plugins/go-grpc with pinned protobuf generation, provider and launch-extension fixture profiles and no GameNo | 3–6 h | Média/alta | Sol |
| 11.4 | Implement a real plugin-owned configuration window in both examples with focus/close and one persistent non-secret preference | 2–4 h | Média/alta | Sol |
| 11.5 | Add runnable fake account/secret/login/action/settings recipes covering all existing RPC/event families and a new Core read/c | 4–8 h | Média/alta | Sol |
| 11.6 | Supply per-example manifests, notices, package/sign commands and standalone build/install/run guides | 1–3 h | Baixa/média | Luna |
| 11.7 | Add conformance checks for generated-code freshness, public dependency boundaries, supported-version fallback and both exampl | 2–4 h | Baixa/média | Luna |
| 11.8 | Add equivalent safe Go/Rust launch-consent fixtures for emulators/runtimes, known mod layers and environment-only customizati | 3–6 h | Média/alta | Sol |
| 11.9 | Publish target compatibility examples and deterministic evaluator fixtures for Windows builds/products, Debian release constr | 2–4 h | Baixa/média | Luna |
| 12.1 | Define typed stable domain/report bases, versioned namespaced Struct schemas, opaque byte envelopes and get/list/watch access | 4–8 h | Média/alta | Sol |
| 12.2 | Implement bounded publication/session retention with host-assigned source/developer/origin/publisher/version/package provenan | 2–5 h | Média/alta | Sol |
| 12.3 | Implement default-deny common-data category read grants selected during activation and separate sensitive/custom source-to-re | 4–8 h | Média/alta | Sol |
| 12.4 | Implement domain queries and Data projections/subscriptions over consistent authorization with revocation/cursor fencing, bou | 6–12 h | Alta | Sol (Astra para revisão de arquitetura se houver conflito) |
| 12.5 | Document each dataset/method/permission/projection and safe non-shareable exclusions | 1–2 h | Baixa/média | Luna |
| 12.6 | Implement structural Struct validation, size/depth/key budgets and namespace/field projection | 3–6 h | Média/alta | Sol |
| 12.7 | Implement revision-checked typed field masks and owner-authorized namespace patches | 3–6 h | Média/alta | Sol |
| 13.2 | Add shared-emulator-resource declarations and backing-resource coordination for backup/restore | 3–6 h | Média/alta | Sol |
| 14.2 | Add approved runtime/interpreter resources and per-content game/installation bindings to structured launch recipes | 6–12 h | Alta | Sol (Astra para revisão de arquitetura se houver conflito) |
| 14.3 | Define and implement the common typed runtime-session event/result family over Exchange under default-deny per-game/runtime g | 6–12 h | Alta | Sol (Astra para revisão de arquitetura se houver conflito) |
| 14.4 | Exercise A registering an owned runtime with a custom compatibility extension and B querying it through Runtimes.List/Get/Wat | 2–4 h | Média/alta | Sol |
| 14.5 | Add negotiated scoped runtime-content discovery through Games.RegisterContent for runtime plugins/extensions under library.di | 4–8 h | Média/alta | Sol |
| 15.1 | Implement StorageDiagnostics benchmark/SMART report ingestion and projected retrieval with volume binding, tool/version, unit | 3–6 h | Média/alta | Sol |
| 15.2 | Implement Computer hardware and GamePresence publish/clear/query/watch with versioned fields, safe identities, game/session a | 4–8 h | Média/alta | Sol |
| 15.3 | Implement bounded GamePerformance batches/end-series and retained query/aggregation with FPS/CPU/RAM semantics, computer/game | 5–10 h | Média/alta | Sol |
| 15.4 | Document collection-versus-transport responsibilities and performance schema examples | 0,5–1,5 h | Baixa/média | Luna |
| 16.1 | Add independent default-deny storage.persist, owner-attributed durable stores keyed by the 4.4 namespace resolver and explici | 4–8 h | Média/alta | Sol |
| 16.2 | Implement logical-only Persistence usage/list/delete/clear/retention-policy operations with authenticated owner resolution | 3–6 h | Média/alta | Sol |
| 16.3 | Implement per-plugin/category logical/physical accounting, quotas and bounded performance-history retention with user-protect | 4–8 h | Média/alta | Sol |
| 16.4 | Implement uninstall keep/delete choice and inert archived-data reclaim/reuse with preserved source/version provenance | 3–6 h | Média/alta | Sol |
| 16.5 | Implement own-namespace automatic cleanup policy within user limits and reviewed host-wide cleanup for unused/large datasets, | 4–8 h | Alta | Sol (Astra para revisão de arquitetura se houver conflito) |
| 17.1 | Add activation-time common-data category read controls and separate sensitive-data source/recipient field/history/delivery/re | 3–6 h | Média/alta | Sol |
| 17.2 | Add host-only physical-path storage accounting, uninstall retain/delete and cleanup preview/policy screens | 4–8 h | Média/alta | Sol |
| 17.3 | Extend both Go/Rust examples with fixture producer-consumer exchanges, performance history/cleanup, save/location inspection  | 4–8 h | Média/alta | Sol |
| 17.4 | Add matching Go/Rust hybrid-record, opaque-byte and custom-dataset examples with schema-version/unknown-namespace handling | 3–6 h | Média/alta | Sol |
| 18.1 | Run focused Rust protocol/SDK/host tests, UI component tests and Go example checks, then required workspace checks | 3–6 h | Média/alta | Sol |
| 18.2 | On native Linux, exercise signed package import, both example transports/windows, grant revocation, scoped file/keyring acces | 4–8 h | Baixa/média | Luna |
| 18.3 | On native Windows, exercise both example named-pipe sessions/windows, ACL/junction behavior, credential isolation, supervised | 6–12 h | Baixa/média | Luna |
| 18.4 | Confirm official Steam sign-in/discovery/catalog/counter/actions and Linux package discovery remain unchanged with empty, inv | 2–4 h | Baixa/média | Luna |
| 18.5 | Exercise detected and custom launches across official, third-party and manual game fixtures, including a Linux Proton-style r | 2–4 h | Baixa/média | Luna |
| 18.6 | Exercise the combined two-plugin data flow: selected per-game read/write, private-only query defaults, explicit performance-h | 3–6 h | Baixa/média | Luna |
| 18.7 | Verify the entire endpoint matrix against generated contract fixtures and both clients, including per-query common-data selec | 3–6 h | Média/alta | Sol |
| 18.8 | Exercise common launch-plan consent end to end for runtime, mod and parameter-only overlays on official/third-party game fixt | 2–4 h | Baixa/média | Luna |
| 18.9 | Record native Windows/Linux environment-detection acceptance separately from pure evaluator fixtures, including execution use | 3–6 h | Baixa/média | Luna |
| 18.10 | Verify identity/profile isolation end to end across account identity, secret slots, session and durable storage, caller-owned | 3–6 h | Média/alta | Sol |
| 19.1 | Add manifest playtime/pause capability declarations and developer-selected disabled/application-open accounting | 3–6 h | Média/alta | Sol |
| 19.2 | Add installation/settings disclosure and a persistent per-plugin accounting disable control, including the specified pt-BR wa | 2–4 h | Baixa/média | Luna |
| 19.3 | Implement producer-requested, user-authorized public-pool publication distinct from private storage and independent read/crea | 5–10 h | Alta | Sol (Astra para revisão de arquitetura se houver conflito) |
| 19.4 | Add installation/settings controls for public-pool publication, operation and category/record scopes | 3–6 h | Média/alta | Sol |
| 19.5 | Verify provider and host-issued scoped runtime-content discovery contexts end to end, preserving legacy provider validation w | 2–5 h | Média/alta | Sol |
