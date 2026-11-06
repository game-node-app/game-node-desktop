# Proposta

## Por quê

Hoje "Settings / About" é uma página longa no lugar da biblioteca. Ela mistura renderização, sincronização, sessão e licenças em painéis genéricos, e a proteção de saves de jogos DRM-free fica num diálogo separado que mistura controles globais e por jogo. Cada nova área (lojas, backups) piora esse acúmulo. A equipe web está desenhando uma nova tela de configurações: um modal com submenus e estilo próprio (Figma `GameNode` › seção "Menus · Premium v2 (Sam)", nó `4987:10202`). Esta change adapta essa **ideia** ao desktop, com uma configuração dedicada por tópico e um conjunto de tokens fechado para implementar a tela.

> Os itens e o layout do Figma (Premium, Editar perfil, Notificações, MCP, Privacidade, Aparência etc.) **não** são pedidos de implementação. Aproveitamos deles a estrutura (modal + navegação lateral + painéis com seções e cartões), a linguagem visual e os componentes. O conteúdo de cada painel vem do que o desktop já faz.

## O que muda

- **Settings vira um modal sobre a janela principal.** "Settings / About" deixa de substituir a área de conteúdo. Ele abre um modal com overlay, navegação lateral e um painel por tópico. A biblioteca continua montada por trás e fica inerte enquanto o modal está aberto. O modal abre pelo botão "Settings" da barra lateral, pelo menu do app, pela explicação de sincronização e pelo atalho `Ctrl+,`.
- **Tópicos do desktop**, cada um com seu painel dedicado:
  - **Conta**: identidade conectada, sincronizar com o GameNode, onde a sessão está guardada, entrar e sair.
  - **Lojas**: lojas detectadas pelos provedores embutidos (com contagem de jogos), fonte de tempo de jogo (Loja/Interno) e reescanear. Quando houver suporte a DRM-free, também pastas monitoradas e descobertas ignoradas.
  - **Backups**: proteção local de saves. Inclui backup automático global e pasta de destino. Os controles por jogo (perfil de save, histórico, restauração) continuam nos detalhes do jogo.
  - **Avançado**: renderização (renderização por software, renderizador ativo e fallback automático) e o aviso de que a mudança exige reinício.
  - **Sobre**: versão, licença GPL-3.0-only e avisos de licença.
- **Conteúdo existente reorganizado, sem perda.** Todos os controles e textos da página atual e os controles globais de backup migram para o painel correspondente. Nenhuma preferência nova é persistida, exceto a reorganização dos controles de backup já previstos.
- **Padrões de interação desktop**:
  - Preferências binárias e escolhas aplicam e persistem na hora (comportamento atual).
  - Campos de texto (pasta de destino de backup) usam um rodapé "Alterações não salvas · Descartar · Salvar alterações".
  - Fechar com edições pendentes pede confirmação num diálogo de alerta.
  - Feedback curto aparece em toast.
  - `Esc` fecha o diálogo mais interno, o foco fica preso no modal, e as setas navegam a lista de tópicos.
- **Tokens e estilos do modal.** É adicionado um global de estilo do modal de configurações, com cores, tipografia, espaçamentos, raios, sombras, overlay e as especificações de cada componente (item de navegação, seção, cartão, linha, toggle, rádio, segmentado, select, input, botões, pílulas, kbd, alerta, toast). Cada token tem procedência registrada no Figma. Detalhes em `design.md`.
- **Diálogo de proteção de saves.** O diálogo existente perde os controles globais (automático global e destino), que passam para o painel Backups. Ele continua com os controles por jogo.
- **BREAKING (UI):** a propriedade `settings-open` do `AppWindow` deixa de alternar a área de conteúdo e passa a controlar o modal. O preset `settings` do `ui-review` e qualquer automação que dependa do layout antigo precisam ser atualizados.

## Capacidades

### Novas capacidades

- `desktop-settings`: modal de configurações do desktop. Cobre abertura e fechamento, navegação por tópicos, conteúdo de cada painel (Conta, Lojas, Backups, Avançado, Sobre), modelo de aplicação imediata ou com salvar, confirmação de descarte, feedback, teclado e acessibilidade, comportamento em janelas pequenas e o sistema visual do modal.

### Capacidades modificadas

- `desktop-shell`:
  - "Foundation shell and navigation": Settings/About deixa de ser uma view da área principal e passa a ser um modal sobreposto.
  - "GameNode Version 3.0 theme": aceita, além dos tokens Version 3.0, os tokens do modal de configurações com procedência registrada do nó `4987:10202`.

## Impacto

- **UI (Slint):**
  - `crates/app/ui/app.slint`: remove a página inline de settings e monta o modal.
  - Novos `crates/app/ui/settings*.slint`: estilo, componentes e painéis.
  - `crates/app/ui/theme.slint` e `theme-provenance.json`: tokens e procedência.
  - `crates/app/ui/drm-free.slint`: os controles globais saem de `SaveProtection`.
  - Novos ícones SVG 16 px em `crates/app/ui/icons/`.
- **Rust:**
  - `crates/app/src/main.rs` e `library_ui.rs`: ligações das callbacks existentes, contagem de lojas detectadas e estado do painel ativo.
  - Nenhuma mudança de engine, banco ou API. As operações seguem nas portas e workers atuais, sem bloquear a thread de UI.
- **Ferramentas:** `crates/app/examples/ui-review.rs` e `scripts/dev-screen.sh` ganham presets por painel, além de alerta e toast, para revisão visual.
- **i18n:** novas strings em `i18n/en` e `i18n/pt_BR`. As strings existentes são reaproveitadas sempre que o texto não muda.
- **Dependência:** o painel Backups e a seção DRM-free do painel Lojas assumem que `add-drm-free-games-and-local-backups` foi aplicada. Sem `LocalGames.supported` (por exemplo, no Linux), esses blocos mostram um estado "Indisponível neste sistema".
- **Plataforma:** Windows primeiro. Nenhum comportamento exclusivo do Linux é adicionado.
