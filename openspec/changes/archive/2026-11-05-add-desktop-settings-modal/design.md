# Design

## Contexto

Motivação e escopo: ver `proposal.md`. Requisitos: `specs/desktop-settings/spec.md` e `specs/desktop-shell/spec.md`.

Estado atual que molda a abordagem:

- **Página inline.** `crates/app/ui/app.slint` desenha "Settings / About" no lugar da área principal quando `settings-open` é verdadeiro. São quatro `Panel` genéricos (renderização, playtime e sync, sessão, sobre) e o diálogo de licenças, que é um overlay próprio.
- **Proteção de saves.** `crates/app/ui/drm-free.slint` › `SaveProtection` é um overlay com `#0008` que mistura controles globais (`automatic`, `destination` via `LocalGames.backup-settings(bool, string)`), controles por jogo, pastas monitoradas e revisão de descobertas.
- **Tokens existentes.** `crates/app/ui/theme.slint` (`Theme`) já tem parte dos valores do Figma novo: `#111111`, `#1A1A1A`, `#222222`, `#2A2A2A`, accent, status e subtle. A procedência fica em `crates/app/ui/theme-provenance.json`.
- **Inércia.** A biblioteca já trata modais por `LibraryState.modal` (`match-open || add-open`): controles recebem `inert` e não pegam foco nem clique. O Slint não tem focus trap nativo, então o padrão do projeto é deixar inerte o que está atrás.
- **Lojas.** `LibraryState.providers` (`ProviderItem { id, name, count, selected }`), `LibraryState.scanning`, `LibraryState.failed-stores` (texto) e `LibraryState.rescan()` já existem.
- **Plataforma.** Slint 1.18.1, estilo `fluent-dark`, Roboto Variable embutida. O `Text` do Slint tem `letter-spacing`, mas **não** tem `line-height` nem `text-transform`.
- **Restrições do repositório.** Nunca bloquear a thread de UI. MVP Windows primeiro. Sem testes de performance no plano. Revisão visual por `./scripts/dev-screen.sh` (`ui-review`).

## Objetivos / Não objetivos

**Objetivos:**

- Um único global de estilo para o modal, com **todos** os tokens extraídos da seção Figma `4987:10202` e procedência por token.
- Uma pequena biblioteca de componentes de configurações, reutilizável por qualquer painel futuro, sem valores soltos nos painéis.
- Migrar 100% do conteúdo atual de Settings/About e os controles globais de backup, sem mudar engine, banco ou API.

**Não objetivos:**

- Itens da referência web: Premium, Editar perfil, Notificações, Privacidade, Aparência e idioma, MCP, Downloads, "Reportar um bug", avatar com upload, gradiente premium.
- Novas preferências persistidas. Exceção: o seletor de pasta de destino, que só preenche o campo existente.
- Tema claro, troca de tema, troca manual de idioma.
- Reestilizar a biblioteca, o diálogo de match ou o editor de jogo DRM-free. O token set do modal não substitui `Theme` fora dele.
- Componentes do Figma sem uso no desktop agora: Select, rádio, cartão de opção, textarea. Os tokens ficam documentados abaixo, mas os componentes **não** são implementados até um painel precisar deles.

## Decisões

### D1. Modal dentro da janela, não janela separada

O modal é um overlay no `AppWindow`, com a mesma estrutura dos diálogos atuais: retângulo de overlay, `TouchArea` que consome cliques e `FocusScope` para `Esc`.

- **Alternativa: janela de SO separada.** Rejeitada. Complica foco e tray no Windows, posicionamento no Wayland e o ciclo de vida de instância única. Também foge do padrão da referência (modal sobre a app).
- **Alternativa: manter como view da área principal.** Rejeitada. É o problema descrito na proposta.

### D2. Estado global `SettingsState`

Novo arquivo `crates/app/ui/settings-state.slint`:

```slint
export enum SettingsTopic { account, stores, backups, advanced, about }
export enum ToastKind { success, warning, danger }
export global SettingsState {
    in-out property <bool> open;
    in-out property <SettingsTopic> topic: SettingsTopic.account;
    in-out property <bool> dirty;            // há edição de texto pendente
    in-out property <bool> alert-open;       // confirmação de descarte
    private property <SettingsTopic> pending-topic; // destino da troca adiada
    in-out property <bool> pending-close;
    in-out property <string> toast-text;
    in-out property <ToastKind> toast-kind;
    in-out property <int> toast-serial;      // incrementado por mensagem; reinicia o timer
    callback show-toast(ToastKind, string);
}
```

- `AppWindow.settings-open` passa a ser `<=> SettingsState.open`. Assim `main.rs`, `library_ui.rs` e `ui-review` mantêm o nome. **BREAKING:** o significado muda para "modal aberto".
- `LibraryState.modal` passa a incluir `SettingsState.open`, para que barra lateral, menu, biblioteca e `ResizeHandle` fiquem `inert` sem duplicar lógica.
- O tópico não é persistido, o que atende ao requisito "lembrado na execução". Ele fica no global, que vive enquanto o processo vive.
- O toast some com um `Timer` Slint de 4 s, reiniciado a cada `toast-serial`.

### D3. Tópicos e mapeamento do conteúdo atual

| Tópico | Ícone (Figma) | Conteúdo | Origem atual |
|---|---|---|---|
| Conta | `Settings / Ícone / user` `4987:10207` | Sessão (identidade, entrar via Google/Discord/X, cancelar, tentar de novo, sair); armazenamento da sessão; Sync with GameNode e erro de playtime | Tela de login, menu (Sign out), painéis "Session & security" e "Playtime & sync" |
| Lojas | `Settings / Ícone / plug` `4987:10215` | Lojas detectadas (`providers`, `failed-stores`, `scanning`, Reescanear); fonte de tempo de jogo; seção DRM-free (pastas monitoradas, reescanear, revisão de descobertas, redefinir ignorados) | Menu "Rescan", painel "Playtime & sync", metade de `SaveProtection` |
| Backups | `Settings / Ícone / shield` `4987:10222` | Aviso local; backup automático global; destino (campo + Escolher pasta + salvar); nota sobre controles por jogo | Metade global de `SaveProtection` |
| Avançado | `icon/desktop` `4988:11919` | Renderização por software; renderizador ativo/fallback; pílula "Requer reinício" | Painel "Appearance & performance" |
| Sobre | `icon/book` `4988:13003` | Logo, versão, licença; resumo de licenças; abrir avisos | Último painel e overlay de licenças |

- O botão "Local save protection" do painel atual some. `SaveProtection` continua abrindo pelos detalhes do jogo, só com os controles por jogo, como exige o requisito "DRM-free details expose protection and maintenance" de `add-drm-free-games-and-local-backups`.
- Sem `LocalGames.supported`, Backups e a seção DRM-free mostram a pílula neutra "Indisponível neste sistema" no lugar dos controles. O tópico Backups continua listado, porque a navegação é estável.
- Abrir a partir da explicação de sync (`dismissed(true)`) define `topic = account`.

### D4. Modelo de aplicação

| Tipo de controle | Aplicação | Rodapé | Exemplos |
|---|---|---|---|
| Toggle, segmentado, rádio | Imediata, persistida em worker | Não | Sync, fonte de tempo de jogo, backup automático, renderização por software |
| Campo que **é** uma preferência | Pendente até Salvar | Sim (`dirty`) | Destino do backup |
| Campo que **dispara uma ação** | Ação no botão ao lado | Não (não marca `dirty`) | "Pasta a monitorar" + Monitorar pasta |

- **Backup automático.** O toggle chama `LocalGames.backup-settings(novo, destino-salvo)`. O destino salva com `backup-settings(automatic-salvo, novo-destino)`. Assim a callback existente basta.
- **Escolher pasta.** Precisa de uma nova callback `LocalGames.destination-pick()`, que reutiliza o seletor nativo já usado por `saves-pick` fora da thread de UI. Ela só preenche o campo e marca `dirty`.
- **Falha de persistência.** O Rust republica o valor salvo e seta o erro do painel. O controle reflete o estado vindo do engine, nunca um estado local otimista que fica para trás.
- **Toast.** Disparado pelo Rust (`SettingsState.show-toast`) ao concluir:
  - salvar destino: "Destino de backup salvo"
  - reescanear: "Biblioteca reescaneada"
  - falha: variante danger com a mensagem localizada

### D5. Tokens: global `SettingsStyle`, separado de `Theme`

Novo `crates/app/ui/settings-style.slint` com `export global SettingsStyle`.

- Valores idênticos aos do `Theme` são **aliases** (`out property <color> bg-raised: Theme.color-raised;`). Isso evita duas fontes para o mesmo valor.
- Valores novos são literais.
- `theme-provenance.json` ganha a seção `settingsModal`: para cada token, `sourceKind` (variable/node/adaptation), `sourceName`, `nodeId` ou `variableId`, `resolvedValue` e, em adaptações, `reason`.
- **Alternativa: trocar `Theme.color-text-default` (#D9D9D9) por #FFFFFF e `text-dimmed` (#808080) por #7A7A7A globalmente.** Rejeitada nesta change. Mudaria a biblioteca inteira sem revisão. Fica como possível unificação futura.
- **Alternativa: estilo `fluent-dark` dos `std-widgets`** (`Switch`, `LineEdit`, `CheckBox`). Rejeitada no modal. Não segue a referência e não aceita os tokens. `LineEdit` só pode ser usado embrulhado, se for preciso (ver D6).

#### Cores

| Token `SettingsStyle` | Valor | Origem | Uso |
|---|---|---|---|
| `bg-base` | `#0B0B0B` | variável `color/bg/base` | Corpo do modal, inputs, segmentado, info box, barra de ações do alerta, fade |
| `bg-sidebar` | `#111111` (= `Theme.color-chrome`) | variável `color/bg/sidebar` | Navegação lateral |
| `bg-surface` | `#1A1A1A` (= `Theme.color-secondary`) | variável `color/bg/surface` | Cartões, alerta, cartão de opção |
| `bg-raised` | `#222222` (= `Theme.color-raised`) | variável `color/bg/raised` | Item selecionado, botão secundário, kbd, pílula neutra, tile, botão fechar, toast, segmento ativo, select |
| `border-subtle` | `#2A2A2A` (= `Theme.color-border-subtle`) | variável `color/border/subtle` | Bordas de modal, cartões, divisórias, inputs, botões |
| `text-primary` | `#FFFFFF` | variável `color/text/primary` | Títulos, rótulos, texto de botão, valores |
| `text-secondary` | `#ADADAD` | variável `color/text/secondary` | Descrições, itens de navegação, pílula neutra, kbd, ghost |
| `text-muted` | `#7A7A7A` | variável `color/text/muted` | Cabeçalho de grupo, meta, contadores, rodapé "GameNode · versão", "—" |
| `accent` | `#F15025` (= `Theme.color-primary`) | variável `color/accent/primary` | Botão primário, ícone do item selecionado, borda de opção, knob ligado, foco |
| `accent-subtle` | `#F150251F` (= `Theme.color-accent-subtle`) | variável `color/accent/subtle` | Pílula accent |
| `accent-hover` | `accent.brighter(10%)` | adaptação (mesmo padrão de `Action`) | Hover do primário |
| `accent-pressed` | `#872C13` (= `Theme.color-tertiary`) | adaptação (`Theme`) | Pressionado do primário |
| `success` | `#3DD68C` (= `Theme.color-success`) | variável `color/status/success` | Borda de input válido, ícone do toast, indicador conectado |
| `success-subtle` | `#3DD68C1F` | variável `color/status/success-subtle` | Pílula success |
| `warning` | `#F5A524` | variável `color/status/warning` | "Alterações não salvas", pílula "Requer reinício", "Falhou" |
| `warning-subtle` | `#F5A5241F` | variável `color/status/warning-subtle` | Fundo da pílula warning |
| `danger` | `#E5484D` (= `Theme.color-danger`) | variável `color/status/danger` | Botão danger, texto de erro de campo, borda de input inválido |
| `danger-subtle` | `#E5484D1F` (= `Theme.color-danger-subtle`) | adaptação (`Theme`; o Figma não usa no modal) | Tile do alerta destrutivo |
| `danger-hover` | `danger.brighter(10%)` | adaptação | Hover do danger |
| `overlay` | `#0000009E` (preto 62%) | nó `4988:10445` "Overlay" | Fundo atrás do modal e do alerta (o alerta empilha um segundo overlay) |
| `scrollbar-thumb` | `#FFFFFF38` (branco 22%) | nó `4988:10711` | Polegar da rolagem interna |
| `fade-start` / `fade-end` | `#0B0B0B00` → `#0B0B0B` | nó `4988:10712` | Esmaecimento "continua abaixo" |
| `toggle-track-on` | `#1A1A1A` | componente `4501:3276` (SVG) | Trilho ligado |
| `toggle-track-off` | `#2E2E2E` | componente `4501:3273` (SVG) | Trilho desligado |
| `toggle-border` | `#2E2E2E` | `4501:3276` / `4501:3273` | Borda 1 px do trilho |
| `toggle-knob-on` | `#F15025` | `4501:3276` | Knob ligado |
| `toggle-knob-off` | `#7A7A7A` | **adaptação** do `#1D1D1D` de `4501:3273` | Knob desligado. O original tem contraste ~1,2:1 contra o trilho e não atinge 3:1 (WCAG 1.4.11) |
| `focus-ring` | `#F15025` | adaptação (o Figma não define foco) | Anel de foco de todos os controles |
| `shadow-modal` | `#0000008C`, y 32, blur 80 | modal `4988:10446` (`0 32 80 rgba(0,0,0,.55)`) | Modal |
| `shadow-alert` | `#00000099`, y 24, blur 64 | alerta `4988:13705` (`0 24 64 rgba(0,0,0,.6)`) | Alerta e avisos de licença |
| `shadow-toast` | `#00000080`, y 12, blur 32 | toast `4988:14320` (`0 12 32 rgba(0,0,0,.5)`) | Toast |

**Não adotados** (registrados como excluídos): roxo premium `rgba(151,71,255,.12)→0`, borda `rgba(151,71,255,.45)`, texto `#E4D5FF`.

#### Tipografia (Roboto Variable embutida)

O Slint não tem `line-height`. A altura de linha do Figma vira espaçamento entre blocos, e linhas de texto quebrado usam a altura natural da Roboto. Diferença aceita na revisão visual: até 3 px por linha. Também não há `text-transform`: textos em maiúsculas são traduzidos já em maiúsculas, como o "YOUR SPACE" atual.

| Token | Tamanho / peso | Extra | Origem | Uso |
|---|---|---|---|---|
| `type-title` | 20 / 500 | — | `text/xl/medium` | Título do painel, título do alerta |
| `type-section` | 16 / 500 | — | `text/md/medium` | Título de seção |
| `type-label` | 14 / 600 | — | `text/sm/semibold` | Rótulo de linha, item selecionado, nome no bloco de usuário, rótulo de opção |
| `type-body` | 14 / 400 | — | `text/sm/regular` | Item de navegação, texto de input, corpo do alerta |
| `type-caption` | 13 / 400 | lh Figma 18 → `space-caption-gap` | nós de descrição (`13/18`) | Descrições, ajuda, "Alterações não salvas" |
| `type-button` | 13 / 500 | — | botões | Texto de botões, segmento ativo, toast-ação |
| `type-control` | 13 / 400 | — | Select `4988:11732`, segmento inativo `4988:11829` | Valor de select, segmento inativo |
| `type-meta` | 12 / 400 | — | `text/xs/regular` | E-mail/estado no bloco de usuário, contador, "Sempre", rodapé |
| `type-pill` | 12 / 500 | — | pílulas | Pílulas |
| `type-kbd` | 11 / 500 | — | kbd "Esc" | Atalhos |
| `type-group` | 11 / 600 | `letter-spacing: 0.6px`, maiúsculas | cabeçalho "CONFIGURAÇÕES" | Cabeçalho de grupo da navegação |

#### Espaçamento, tamanhos e raios

Nomes semânticos no global (`out property <length>`):

| Token | Valor | Origem |
|---|---|---|
| `modal-max-width` × `modal-max-height` | 960 × 720 | modal `4988:10446` |
| `modal-margin` | 24 (16 se a janela < 760 de largura) | **adaptação** (janela mínima 640×420) |
| `sidebar-width` | 216 (176 se o modal < 760) | Figma 216; compacto é adaptação |
| `sidebar-padding-y` / `-x` / `gap` | 16 / 10 / 14 | sidebar |
| `user-avatar` | 32 | `Avatar/UserAvatar` `4988:10449` |
| `user-block-padding-y` / `-x` / `gap` | 4 / 6 / 10 | bloco de usuário |
| `nav-gap` | 2 | lista de navegação |
| `nav-group-padding-bottom` / `-left` | 6 / 10 | cabeçalho de grupo |
| `nav-item-height` | 32 | `Settings / Item` |
| `nav-item-padding-y` / `-x` / `gap` | 8 / 10 / 10 | idem |
| `icon-sm` / `icon-md` / `icon-lg` | 14 / 16 / 18 | ícones `chevron/lock/restart/trash` 14, navegação/close/check 16, tile 18 |
| `footer-padding-top` / `-left` | 6 / 10 | "GameNode · versão" |
| `header-padding-top` / `-left` / `-right` / `-bottom` | 18 / 32 / 16 / 16 | cabeçalho fixo do painel |
| `header-gap` / `title-gap` / `close-cluster-gap` | 24 / 4 / 8 | idem |
| `close-button` | 32 | botão fechar |
| `scroll-padding-top` / `-x` / `-bottom` | 28 / 32 / 40 (x = 20 no modo compacto) | área rolável; compacto é adaptação |
| `section-gap` / `section-header-gap` | 32 / 16 | seções |
| `row-padding-y` / `-x` / `gap` / `text-gap` | 16 / 20 / 16 / 4 | linha de cartão |
| `table-row-padding-y` / `table-header-padding-y` / `table-col` | 14 / 12 / 76 | variante tabela (reservada) |
| `tile` / `tile-lg` | 36 / 40 | tile de linha / tile do alerta |
| `scrollbar-width` / `fade-height` | 4 / 72 | `4988:10711` / `4988:10712` |
| `toggle-width` × `toggle-height` / `toggle-knob` / `toggle-inset` | 33 × 16 (+1 px de borda) / 14 / 2 | `4501:3276` |
| `radio` / `radio-on-border` / `radio-off-border` | 18 / 5 / 1.5 | rádio (reservado) |
| `button-padding-y` / `-x` / `icon-gap` | 7 / 12 / 6 | botões |
| `input-padding-y` / `-x` | 10 / 12 | input |
| `select-padding` | 7 / 12 esq. / 10 dir., gap 8 | Select `4988:11732` (reservado) |
| `segmented-padding` / `gap` / `item-padding-y` / `-x` | 3 / 2 / 5 / 12 | Segmented `4988:11826` |
| `pill-padding-y` / `-x` | 3 / 8 | pílulas |
| `kbd-padding-y` / `-x` | 2 / 6 | kbd |
| `infobox-padding-y` / `-x` / `gap` | 12 / 14 / 6 | info box |
| `actions-gap` | 8 | rodapé de ações, barra do alerta |
| `alert-width` | 440 | `4988:13705` |
| `alert-padding` | 24 topo / 24 lados / 20 base, gap 14; header gap 12 | idem |
| `alert-actions-padding-y` / `-x` | 16 / 24 | idem |
| `toast-padding` | 10 vertical / 14 esq. / 10 dir., gap 12 | `4988:14320` |
| `focus-ring-width` / `focus-ring-offset` | 2 / 2 | adaptação |

| Raio | Valor | Onde |
|---|---|---|
| `radius-scrollbar` | 2 | polegar |
| `radius-kbd` | 4 | kbd |
| `radius-nav` | 6 | item de navegação, segmento, preview de opção |
| `radius-control` | 8 | botão, input, select, segmentado, botão fechar, info box |
| `radius-tile` | 10 | tile, cartão de opção, toast |
| `radius-card` | 12 | cartão de seção |
| `radius-modal` | 16 | modal, alerta (o fade usa 15 no canto inferior direito, por estar dentro da borda) |
| `radius-pill` | 999 → `height / 2` | pílulas, avatar, rádio, toggle |

### D6. Componentes (`crates/app/ui/settings-controls.slint`)

Todos usam apenas `SettingsStyle`. Estados que o Figma não define são adaptações:

- **Hover:** um degrau acima do fundo. Transparente → `bg-surface`, `bg-surface` → `bg-raised`, `bg-raised` → `bg-raised.brighter(8%)`.
- **Pressionado:** `darker(8%)`.
- **Desabilitado:** opacidade 0.45 e `accessible-enabled: false`.
- **Foco:** anel `focus-ring` de 2 px, afastado 2 px, com raio do controle + 2.
- **Animações:** `animate background { duration: 120ms; }`, igual a `Action`.

| Componente | Anatomia e tokens | Estados | Acessibilidade |
|---|---|---|---|
| `SettingsNavItem` | Altura 32; padding 8×10; gap 10; raio 6; ícone 16; `type-body` `text-secondary`; pílula opcional | Padrão: sem fundo. Hover: `bg-surface`. Selecionado: `bg-raised` + `type-label` `text-primary` + ícone colorizado `accent` | `list-item`, `accessible-item-selected`; setas ↑↓ dentro da lista |
| `SettingsGroupHeader` | `type-group` `text-muted`, padding esq. 10 e base 6 | — | Texto |
| `SettingsUserBlock` | Avatar 32 com inicial do nome em `type-label` sobre `accent-subtle`/`accent`. O desktop não tem foto, então a inicial é adaptação. Nome `type-label`; estado `type-meta` `text-muted` com ponto `success` 6 px quando conectado | Sem sessão: ícone `user` em `bg-raised` + "Sem sessão" | Rótulo combinado "nome, conectado" |
| `SettingsPanelHeader` | Título `type-title` + descrição `type-caption` `text-secondary` (gap 4); cluster: `Kbd "Esc"` + `SettingsCloseButton`; borda inferior `border-subtle` | Fixo | Título como nome da região |
| `SettingsCloseButton` | 32×32, `bg-raised`, raio 8, ícone `close` 16 `text-secondary` | Hover `text-primary` | `button`, rótulo "Fechar configurações" |
| `Kbd` | `bg-raised`, borda 1 `border-subtle`, raio 4, padding 2×6, `type-kbd` `text-secondary` | — | Oculto da árvore acessível (decorativo) |
| `SettingsSection` | Título `type-section` + descrição `type-caption`, gap 4; 16 até o cartão | — | `groupbox` com o título |
| `SettingsCard` | `bg-surface`, borda 1 `border-subtle`, raio 12, filhos separados por divisória 1 px | — | — |
| `SettingsRow` | Padding 16×20, gap 16; tile opcional 36 (`bg-raised`, raio 10, ícone 18); coluna rótulo `type-label` + descrição `type-caption` `text-secondary`, gap 4; slot de controle à direita | Clicar no rótulo aciona o controle da linha | Rótulo e descrição viram nome e descrição do controle |
| `SettingsToggle` | 33×16 + borda 1 px; raio `height/2`; trilho `toggle-track-on/off`, borda `toggle-border`; knob 14 a 2 px da borda (`accent` ligado à direita / `toggle-knob-off` desligado à esquerda); `animate x 120ms` | Desabilitado 0.45; foco: anel | `switch` com `accessible-checked`; Espaço/Enter alternam; área clicável = linha inteira |
| `SettingsSegmented` | Contêiner `bg-base`, borda 1 `border-subtle`, raio 8, padding 3, gap 2. Item: padding 5×12, raio 6. Ativo: `bg-raised` + `type-button` `text-primary`. Inativo: `type-control` `text-secondary` | Hover inativo: `text-primary` | `tab-list` com `tab`s selecionáveis; ←→ movem e aplicam |
| `SettingsInput` | `bg-base`, borda 1 `border-subtle`, raio 8, padding 10×12, `type-body`; linha de ajuda `type-caption` `text-secondary` e contador `type-meta` `text-muted` à direita | Foco: borda 1.5 `accent`. Válido: borda 1.5 `success` + ícone `check` 16. Erro: borda 1.5 `danger` + texto de ajuda `danger` | `text-input`; erro em `accessible-description` |
| `SettingsButton` (`variant`: primary, secondary, ghost, danger) | Padding 7×12, raio 8, `type-button`, ícone opcional 14 com gap 6. Primary: `accent`/branco. Secondary: `bg-raised` + borda `border-subtle`/branco. Ghost: sem fundo/`text-secondary`. Danger: `danger`/branco | Hover/pressionado conforme acima; ocupado: texto mantido + opacidade 0.45 | `button` |
| `SettingsIconButton` | 32×32, raio 8, ghost, ícone 16 | Hover `bg-raised` | `button` com rótulo obrigatório |
| `Pill` (`variant`: neutral, accent, success, warning) | Padding 3×8, raio `height/2`, `type-pill`. Neutral: `bg-raised`/`text-secondary`; accent: `accent-subtle`/`accent`; success: `success-subtle`/`success`; warning: `warning-subtle`/`warning` | — | Texto |
| `InfoBox` | `bg-base`, borda 1 `border-subtle`, raio 8, padding 12×14, gap 6, ícone 16 opcional | — | Texto |
| `SettingsActionBar` | Alinhado à direita, gap 8: "Alterações não salvas" `type-caption` `warning` + Ghost "Descartar" + Primary "Salvar alterações" | Só com `dirty` | Ordem de foco: Descartar → Salvar |
| `SettingsCheckbox` | **Adaptação** (o Figma não tem): 18×18, raio 4. Desmarcado: borda 1.5 `text-muted`. Marcado: fundo `accent` + `check` 14 branco | Foco: anel | `checkbox` |
| `SettingsScroll` | `Flickable` + polegar 4 px `scrollbar-thumb` raio 2, a 4 px da borda direita, e fade 72 px na base quando `viewport-y > -(viewport-height - visible-height)` | Polegar arrastável; roda do mouse | Rola com PageUp/PageDown/Home/End quando focado |
| `AlertDialog` | Segundo overlay `overlay`; 440 de largura (máx. janela − 32); `bg-surface`, borda 1, raio 16, `shadow-alert`. Corpo: padding 24/24/20, gap 14; header gap 12 com tile 40 (raio 10, `danger-subtle` + `alert` 16 `danger`), título `type-title`, `Kbd Esc`; texto `type-body` `text-secondary`. Barra: `bg-base`, padding 16×24, gap 8, à direita: Secondary "Cancelar" + Danger | Foco inicial em Cancelar; `Esc` = Cancelar | Papel de diálogo pelo título; foco preso |
| `Toast` | `bg-raised`, borda 1 `border-subtle`, raio 10, padding 10 / 14 esq. / 10 dir., gap 12, `shadow-toast`; ícone `check` 16 `success` (ou `alert` `warning`/`danger`); texto `type-label`; Ghost "Fechar". Posição: centralizado, 24 px acima da base do painel | Some em 4 s; não recebe foco automático | Anunciado por leitor de tela (live region). Se o Slint 1.18 não expuser `live`, o toast vira `accessible-role: text` e o foco permanece; documentar a limitação em `theme-provenance.json` |

**Componentes reservados** (só tokens, sem implementação nesta change): Select (`4988:11732`: `bg-raised`, borda, raio 8, padding 7/12/10, gap 8, `type-control`, `chevronDown` 14), Rádio (18; desligado: borda 1.5 `text-muted`; ligado: fundo branco + borda 5 `accent`), Cartão de opção (`bg-surface`, borda 1 → 1.5 `accent` quando selecionado, raio 10, padding 10/10/12, gap 10, preview 72 raio 6), Textarea (altura 88), variante tabela.

### D7. Layout e responsividade

```
┌ overlay (#0000009E) ──────────────────────────────────────────────────┐
│   ┌ modal: min(960, janela−2·margin) × min(720, janela−2·margin) ──┐  │
│   │┌ sidebar 216 ─┐┌ header fixo (título, descrição, Esc, ✕) ──────┐│  │
│   ││ usuário      ││───────────────────────────────────────────────││  │
│   ││ ──────────── ││ scroll: seções → cartões → linhas             ││  │
│   ││ CONFIGURAÇÕES││                                               ││  │
│   ││ ▸ Conta      ││                                     ▌ thumb   ││  │
│   ││   Lojas      ││                                               ││  │
│   ││   Backups    ││ [Alterações não salvas · Descartar · Salvar]  ││  │
│   ││   Avançado   ││▒▒▒▒▒▒▒▒▒▒▒▒▒▒ fade 72 ▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒││  │
│   ││   Sobre      ││            [toast]                            ││  │
│   ││ GameNode·v   │└───────────────────────────────────────────────┘│  │
│   │└──────────────┘                                                 │  │
│   └─────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────┘
```

- **Posição.** O modal fica centralizado na janela inteira, inclusive sobre a barra superior. O overlay também cobre a barra superior.
- **Rodapé de ações.** O `SettingsActionBar` aparece no fim do conteúdo da seção editável, alinhado à direita, como no Figma. Ele não é fixo.
- **Modo compacto** (largura do modal < 760, adaptação): `sidebar-width` 176, `scroll-padding-x` 20 e `modal-margin` 16. Os títulos dos tópicos cabem em 176 com `overflow: elide`.
- **Linhas estreitas.** A coluna de texto da linha tem `horizontal-stretch: 1` e `wrap: word-wrap`. O controle à direita tem largura preferida. Abaixo de 360 px de painel, o controle desce para baixo do texto (`if` de layout no `SettingsRow`).
- **Licenças.** O diálogo de licenças é reestilizado como `AlertDialog` largo: máx. 720 de largura, altura máx. janela − 64, mesma paginação. Ele abre sobre o modal com o segundo overlay.

### D8. Teclado e foco

- **Atalho `Ctrl+,`.** Fica no `shell-focus` do `AppWindow`. Abre o modal se nenhum outro diálogo estiver aberto (`!LibraryState.modal && !sync-explanation`).
- **`Esc`, em cascata:**
  1. Alerta → Cancelar.
  2. Licenças → fechar.
  3. Modal → fechar se `!dirty`; se `dirty`, abre o alerta com `pending-close`.
- **Foco ao abrir:** item selecionado da navegação.
- **Tab.** Percorre navegação → cabeçalho (fechar) → conteúdo, com wrap. O foco não escapa porque tudo atrás está `inert` (D2).
- **Foco ao fechar.** O `AppWindow` guarda quem abriu (`opener`: sidebar, menu ou atalho) e devolve o foco a ele. Sem opener, devolve ao `shell-focus`.
- **Setas.** ↑↓ na navegação trocam o tópico. Com `dirty`, abrem o alerta com `pending-topic`. ←→ no segmentado trocam a escolha.

### D9. Ícones

- **Download.** SVGs 16 px baixados do Figma na implementação com `get_design_context` e `download_assets`. As URLs temporárias expiram em 7 dias, então não ficam no código. Eles vão para `crates/app/ui/icons/settings/` e entram no código por `@image-url`, colorizados por token.
- **Inventário com nó:**
  - `user` `4987:10207`
  - `plug` `4987:10215`
  - `shield` `4987:10222`
  - `desktop` `4988:11919` (18, exibido em 16)
  - `book` `4988:13003` (18, exibido em 16)
  - `close` `4988:10525`
  - `check` `4988:10580`
  - `alert` `4988:11165`
  - `lock` `4988:11911` (14)
  - `restart` `4988:11987` (14)
  - `trash` `4988:11998` (14)
  - `plus` `4988:10701` (18)
  - `google` `4988:11872`
  - `discord` `4988:11883`
  - `x` `4988:11897`
  - `chevronDown` `4988:11734` (reservado)
- **Regras.** Os SVGs não são editados. Tamanhos diferentes são só escala do `Image` com `image-fit: contain`.
- **Ícone de pasta.** Não existe no Figma. "Escolher pasta…" é um botão só com texto.

### D10. Ligações Rust

Sem mudanças de engine, banco ou API.

- **`library_ui.rs`:**
  - Adiciona `failed: bool` em `ProviderItem`, derivado da mesma fonte que já monta `failed-stores`.
  - Publica toasts ao fim de `rescan` e de `backup-settings`, a partir do evento de conclusão que já atualiza `LibraryState.scanning` e `LocalGames.busy`.
- **`main.rs`:** `settings-open` continua sendo setado como hoje. `sync-dismissed(open-settings)` também seta `SettingsState.topic = account`.
- **`LocalGames.destination-pick()`:** usa o mesmo caminho de seletor nativo de `saves-pick`, despachado para worker. O resultado volta por `slint::invoke_from_event_loop` e preenche o campo.
- Toda gravação continua pelos `storage.submit`/workers atuais. Callbacks de UI só enfileiram.

### D11. i18n

- **Contexto.** Strings novas usam o contexto `@tr("Settings" => "...")`, como o padrão `"DRM-free" => ...` existente.
- **Reaproveitamento.** Strings existentes que não mudam de texto são reaproveitadas com o mesmo contexto e id, por exemplo "Sync with GameNode", "Software rendering" e a ajuda da fonte de tempo de jogo, para manter as traduções.
- **Títulos e descrições dos painéis** (EN / pt-BR):

| Tópico | EN | pt-BR |
|---|---|---|
| Conta | Account — "Your GameNode connection and what this PC syncs." | Conta — "Sua conexão com o GameNode e o que este PC sincroniza." |
| Lojas | Stores — "Stores found on this PC and how playtime is read." | Lojas — "Lojas encontradas neste PC e como o tempo de jogo é lido." |
| Backups | Backups — "Local protection for your DRM-free game saves." | Backups — "Proteção local dos saves dos seus jogos DRM-free." |
| Avançado | Advanced — "Rendering and troubleshooting options." | Avançado — "Renderização e opções para solucionar problemas." |
| Sobre | About — "Version, license and third-party notices." | Sobre — "Versão, licença e avisos de terceiros." |

- **Outras strings novas:**
  - "SETTINGS"/"CONFIGURAÇÕES"
  - "Unsaved changes"/"Alterações não salvas"
  - "Discard"/"Descartar"
  - "Save changes"/"Salvar alterações"
  - "Discard changes?"/"Descartar alterações?"
  - "Requires restart"/"Requer reinício"
  - "Unavailable on this system"/"Indisponível neste sistema"
  - "{n} installed games"/"{n} jogos instalados"
  - "No installed games found"/"Nenhum jogo instalado encontrado"
  - "Last scan failed"/"O último escaneamento falhou"
  - "Choose folder…"/"Escolher pasta…"
  - "Close settings"/"Fechar configurações"
  - textos de toast

### D12. Revisão visual (`ui-review`)

- **Presets.** `ui-review.rs` e `dev-screen.sh preset` ganham:
  - `settings-account`, `settings-account-signed-out`
  - `settings-stores`, `settings-stores-failed`
  - `settings-backups`, `settings-backups-dirty`, `settings-backups-unsupported`
  - `settings-advanced-restart`, `settings-about`
  - `settings-alert`, `settings-toast`, `settings-compact` (640×420)
- **Preset antigo.** O preset `settings` atual vira alias de `settings-account`.
- **Locale.** Os presets valem em `en` e `pt_BR` (`dev-screen.sh locale`).

## Riscos / Trade-offs

- [O Slint não tem `line-height`, então descrições de várias linhas ficam ~3 px mais compactas que no Figma] → Aceito como adaptação registrada. A distância entre blocos segue os tokens. A revisão compara pelo screenshot.
- [Sem focus trap nativo, um controle novo sem `inert` pode receber foco atrás do modal] → `LibraryState.modal` inclui o modal, e toda `Action`/`SidebarItem`/`ResizeHandle` já respeita `inert`. Há um teste de UI que faz Tab pelo modal e verifica que o foco não sai.
- [`text-muted` #7A7A7A sobre `bg-surface` #1A1A1A dá ~4,1:1, abaixo de 4,5:1 para 12 px] → `text-muted` fica restrito a informação não essencial (cabeçalho de grupo, rodapé, "—"). Contagens e estados de loja usam `text-secondary` (~7,8:1).
- [O knob desligado do Figma (#1D1D1D) é quase invisível] → Adaptação `toggle-knob-off = #7A7A7A`, registrada na procedência. O estado também é exposto ao leitor de tela.
- [**BREAKING** em `settings-open`, que agora abre um modal: automações do `ui-review` e testes que esperam a página inline quebram] → Presets atualizados na mesma change. O alias `settings` é mantido.
- [Dependência de `add-drm-free-games-and-local-backups`] → Os blocos dependentes ficam atrás de `LocalGames.supported` e não alteram aquela capability. Se esta change for aplicada antes do archive dela, os requisitos referenciados continuam no delta dela.
- [Dois globais de estilo (`Theme` e `SettingsStyle`) com valores próximos de texto (#D9D9D9 vs #FFFFFF)] → Isolados ao modal por decisão (D5). A unificação fica para uma change futura de tema.
- [Live region do toast pode não existir no Slint 1.18] → Fallback descrito em D6. O toast nunca é a única forma de feedback, porque o estado do controle também muda.

## Plano de migração

1. Entregar numa única versão. Não há migração de dados: preferências e chaves do banco não mudam.
2. Remover a página inline e o botão "Local save protection" do painel atual no mesmo commit que monta o modal, para nunca haver dois lugares com o mesmo controle.
3. `SaveProtection` perde os controles globais no mesmo commit que o painel Backups ganha esses controles.
4. Rollback: reverter os commits da change. Como nada persistido muda, versões anteriores leem o mesmo banco.

## Questões em aberto

Nenhuma que mude o escopo. Decisões que podem ser revistas na revisão visual sem alterar os requisitos:

- Ícones dos tópicos Avançado (`desktop`) e Sobre (`book`). Se o design system do GameNode ganhar ícones próprios (cpu, info), trocar mantendo a procedência.
- Unificação futura de `Theme` com os tons de texto do modal (#FFFFFF/#ADADAD/#7A7A7A), numa change de tema separada.
