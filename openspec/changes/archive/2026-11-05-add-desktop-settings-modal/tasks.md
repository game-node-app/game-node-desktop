# Tarefas

## 1. Tokens, procedência e ícones

- [x] 1.1 Criar `crates/app/ui/settings-style.slint` com o global `SettingsStyle` contendo todas as cores, tipografia, espaçamentos, tamanhos, raios e sombras de `design.md` D5 (aliases para `Theme` onde o valor coincide). Verificar que `cargo build --locked -p gamenode-app` compila.
- [x] 1.2 Adicionar a seção `settingsModal` em `crates/app/ui/theme-provenance.json`, uma entrada por token com `sourceKind`, nó ou variável Figma (seção `4987:10202`), `resolvedValue` e `reason` nas adaptações (`toggle-knob-off`, `focus-ring`, hover/pressionado, modo compacto, `danger-subtle`). Listar também como excluídos os tokens do roxo premium. Verificar que o JSON é válido (`python3 -m json.tool`) e que cada token do global tem uma entrada.
- [x] 1.3 Baixar do Figma (`get_design_context` + `download_assets`) os SVGs listados em D9 para `crates/app/ui/icons/settings/`, sem editar os arquivos. Verificar que cada arquivo não está vazio, tem `width`/`height` na raiz e não sobrou nenhuma URL temporária do Figma no repositório (`grep -r "figma.com/api" crates/` vazio).

## 2. Componentes de configurações

- [x] 2.1 Criar `crates/app/ui/settings-controls.slint` com os componentes estruturais: `SettingsGroupHeader`, `SettingsNavItem`, `SettingsUserBlock`, `Kbd`, `SettingsCloseButton`, `SettingsPanelHeader`, `SettingsSection`, `SettingsCard`, `SettingsRow` (com divisórias e quebra abaixo de 360 px). Usar só tokens de `SettingsStyle`. Verificar que compila e que não há literais de cor ou tamanho fora do global (`grep -nE "#[0-9A-Fa-f]{3,8}" crates/app/ui/settings-controls.slint` vazio).
- [x] 2.2 Implementar os controles interativos `SettingsToggle`, `SettingsSegmented`, `SettingsButton` (primary/secondary/ghost/danger), `SettingsIconButton`, `SettingsCheckbox` e `SettingsInput` (estados foco/válido/erro), com hover, pressionado, desabilitado, anel de foco e papéis acessíveis de D6. Verificar com um teste em `crates/app/tests/library_screen.rs` que Espaço/Enter alternam o toggle e ←→ trocam o segmentado.
- [x] 2.3 Implementar `Pill`, `InfoBox`, `SettingsActionBar`, `SettingsScroll` (polegar 4 px e fade 72 px) e `Toast` (timer de 4 s reiniciado por `toast-serial`). Verificar pelo teste que o toast some depois do timer (avançando o relógio de teste do Slint) e que uma nova mensagem reinicia o tempo.
- [x] 2.4 Implementar `AlertDialog` com o segundo overlay, foco inicial em Cancelar e `Esc` = Cancelar. Verificar por teste que Tab não sai do alerta e que `Esc` o fecha sem executar a ação destrutiva.

## 3. Estrutura do modal

- [x] 3.1 Criar `crates/app/ui/settings-state.slint` (`SettingsTopic`, `ToastKind`, `SettingsState`) e ligar `AppWindow.settings-open <=> SettingsState.open`. Incluir `SettingsState.open` em `LibraryState.modal`. Verificar por teste que, com o modal aberto, Tab nunca alcança controles da biblioteca, da barra lateral ou do menu.
- [x] 3.2 Criar `crates/app/ui/settings.slint` com overlay, modal centralizado `min(960, janela − 2·margin) × min(720, …)`, barra lateral (bloco de usuário, cabeçalho "CONFIGURAÇÕES", cinco itens, rodapé "GameNode · versão"), cabeçalho fixo do painel e área rolável. Adicionar o modo compacto abaixo de 760. Verificar com `./scripts/dev-screen.sh preset settings-account en 1100x720` e `resize 640x420` que nada é cortado e o cabeçalho continua fixo.
- [x] 3.3 Remover a página inline de settings de `crates/app/ui/app.slint` e montar o modal sobre a janela inteira, inclusive sobre a barra superior. Manter a biblioteca montada por trás. Verificar por teste que abrir e fechar o modal não altera a seleção nem a rolagem da biblioteca.
- [x] 3.4 Abrir o modal pelo botão Settings da barra lateral, pelo menu do app, por `Ctrl+,` (só sem outro diálogo aberto) e pela explicação de sincronização (tópico Conta). Fechar por botão, `Esc` e clique no overlay, devolvendo o foco a quem abriu. Verificar cada caminho por teste em `library_screen.rs`.
- [x] 3.5 Implementar a navegação: ↑↓ trocam o tópico, o tópico fica lembrado enquanto o processo vive, e com `dirty` a troca ou o fechamento abrem o `AlertDialog` (`pending-topic`/`pending-close`). Verificar por teste: trocar com edição pendente → Cancelar mantém a edição; Descartar troca de tópico e limpa `dirty`.

## 4. Painéis

- [x] 4.1 Painel **Conta**: identidade e estado da sessão, entrar (Google/Discord/X), cancelar/tentar de novo, sair, armazenamento da sessão, toggle "Sync with GameNode" e erro de playtime. Reaproveitar callbacks e strings existentes. Verificar com os presets `settings-account` e `settings-account-signed-out` e por teste que o toggle dispara `sync-changed`.
- [x] 4.2 Adicionar `failed: bool` em `ProviderItem` (`crates/app/src/library_ui.rs`), derivado da mesma fonte de `failed-stores`. Verificar com teste de unidade que uma loja com falha publica `failed = true` e as demais `false`.
- [x] 4.3 Painel **Lojas**: lista de lojas com "{n} jogos instalados", "Nenhum jogo instalado encontrado" ou "O último escaneamento falhou" (pílula warning); botão Reescanear desabilitado durante `scanning`; segmentado Loja/Interno da fonte de tempo de jogo. Verificar com os presets `settings-stores` e `settings-stores-failed` e por teste que o segmentado dispara `source-changed`.
- [x] 4.4 Seção **DRM-free** no painel Lojas: pastas monitoradas (adicionar e remover), reescanear descobertas, revisar candidatos e redefinir ignorados, usando os callbacks de `LocalGames`. Sem `LocalGames.supported`, mostrar a pílula "Indisponível neste sistema". Verificar com o preset `settings-stores` nos dois estados de suporte.
- [x] 4.5 Adicionar `LocalGames.destination-pick()` reaproveitando o seletor nativo de `saves-pick` num worker, com o resultado devolvido por `slint::invoke_from_event_loop`. Verificar no código que a callback de UI só despacha (nenhuma espera síncrona) e que o teste com um seletor falso preenche o campo e marca `dirty`.
- [x] 4.6 Painel **Backups**: InfoBox "os backups ficam neste PC", toggle de backup automático global com aplicação imediata, campo de destino com "Escolher pasta…" e `SettingsActionBar`, erro inline para destino inválido e nota de que os controles por jogo ficam nos detalhes do jogo. Sem suporte, mostrar "Indisponível neste sistema". Verificar com os presets `settings-backups`, `settings-backups-dirty` e `settings-backups-unsupported` e por teste que Salvar chama `backup-settings(automatic-salvo, novo-destino)` e Descartar restaura o valor salvo.
- [x] 4.7 Remover de `SaveProtection` (`crates/app/ui/drm-free.slint`) os controles globais (automático global e destino) e o botão "Local save protection" do antigo painel. Manter perfil de save, histórico e restauração por jogo. Verificar com o preset `protection` que só os controles por jogo aparecem e que o teste de keyboard do diálogo ainda passa.
- [x] 4.8 Painel **Avançado**: toggle de renderização por software, renderizador ativo e fallback, e pílula "Requer reinício" quando a preferência difere do renderizador em uso. Verificar com o preset `settings-advanced-restart` e por teste que o toggle dispara a callback de renderer atual.
- [x] 4.9 Painel **Sobre**: logo, versão, GPL-3.0-only, resumo e "Mostrar avisos de licença". Reestilizar o diálogo de licenças como `AlertDialog` largo (máx. 720) com a paginação atual, aberto sobre o modal. Verificar com o preset `settings-about` e por teste que `Esc` fecha primeiro as licenças e depois o modal.

## 5. Feedback, persistência e thread de UI

- [x] 5.1 Publicar toasts a partir do Rust ao concluir: reescanear ("Biblioteca reescaneada"), salvar destino ("Destino de backup salvo") e falhas (variante danger com a mensagem localizada). Verificar por teste que cada evento de conclusão incrementa `toast-serial` com o texto e a variante esperados.
- [ ] 5.2 Em falha de persistência de preferência imediata, republicar o valor salvo e mostrar o erro do painel. Verificar por teste com um store que falha: o toggle volta ao estado anterior e o erro aparece.
- [x] 5.3 Revisar todas as callbacks novas e alteradas do modal para garantir que só enfileiram trabalho (`storage.submit`, workers, seletor nativo) e atualizam a UI via `invoke_from_event_loop`. Verificar por leitura de código e pelo teste de 5.2, que não pode depender de espera síncrona.

## 6. Traduções

- [x] 6.1 Adicionar as strings novas com contexto `"Settings"` em `i18n/en/LC_MESSAGES/gamenode-app.po` e `i18n/pt_BR/LC_MESSAGES/gamenode-app.po` (títulos e descrições dos painéis, "CONFIGURAÇÕES", rodapé de ações, alerta, pílulas, estados de loja, "Escolher pasta…", "Fechar configurações", toasts), reaproveitando ids existentes quando o texto não muda. Verificar com `cargo test --locked -p gamenode-app --test i18n_catalogs`.
- [x] 6.2 Conferir em pt_BR que nenhum rótulo da barra lateral ou botão é cortado no modo compacto. Verificar com `./scripts/dev-screen.sh preset settings-compact pt_BR 640x420`.

## 7. Revisão visual e documentação

- [x] 7.1 Adicionar os presets de D12 em `crates/app/examples/ui-review.rs` e `scripts/dev-screen.sh` (com `settings` como alias de `settings-account`). Verificar que cada preset gera `/tmp/gamenode-screen.png` em `en` e `pt_BR`.
- [ ] 7.2 Atualizar a lista de presets em `AGENTS.md` e `docs/development.md`. Verificar que todos os comandos documentados rodam como escritos.
- [ ] 7.3 Atualizar os testes que esperavam a página inline (`crates/app/tests/library_screen.rs`, alcance por teclado de settings; `library_pt_br.rs`) para o modal. Verificar com `cargo test --locked -p gamenode-app --test library_screen --test library_pt_br --test i18n_catalogs`.

## 8. Integração

- [ ] 8.1 Comparar os presets `settings-*` com a seção Figma `4987:10202` (tokens, espaçamentos, estados) e registrar desvios aceitos como adaptações em `theme-provenance.json`. Verificar que todo valor visual do modal pode ser rastreado a um token com procedência.
- [ ] 8.2 Rodar a suíte completa `cargo test --locked -p gamenode-app` e `cargo xwin test --workspace --target x86_64-pc-windows-msvc --locked`. Verificar que ambas passam.
- [ ] 8.3 Validar no Windows (`scripts/install-windows-wsl.sh`): abrir por `Ctrl+,`, menu e barra lateral; Escolher pasta com o seletor nativo; trocar renderização e ver "Requer reinício". Verificar manualmente e registrar o resultado no PR.

## Workflow follow-up

- Arquivar a change depois da revisão do PR, e depois do archive de `add-drm-free-games-and-local-backups`, da qual ela depende.
