# Copy

pt-BR comes from Figma (fixture values replaced by `{placeholders}`); English is the source string for
`ui/src/i18n/en.ts`, and pt-BR goes to `ui/src/i18n/pt-BR.ts`. Reuse existing `settings.*` keys where the
meaning is unchanged (Sign out, Sync with GameNode, Store/Internal, Show recent log, Open log folder,
Show license notices, store state labels). Plurals use the existing plural helper; sizes, counts, times
and relative times use the locale formatters in `features/library/formatters.ts`. `(NOVO)`/`NOVO` is never
copy. Strings marked **OUT** are out of scope and must not be added.

## Shell and navigation

| pt-BR | English | Notes |
|---|---|---|
| Configurações | Settings | title-bar title / dialog label |
| Mover janela | Move window | drag handle aria (existing) |
| Fechar | Close | close aria (existing) |
| Buscar configurações | Search settings | search placeholder + aria-label |
| Nenhuma configuração encontrada | No settings found | empty search result |
| Conta | Account | nav group label |
| Biblioteca | Library | nav group label |
| Aplicativo | App | nav group label |
| Conta e sincronização | Account & sync | topic |
| Lojas | Stores | topic |
| Jogos DRM-free | DRM-free games | topic |
| Backups | Backups | topic |
| Geral | General | topic |
| Ferramentas | Tools | topic |
| Plugins | Plugins | topic |
| Sobre | About | topic |
| Requer atenção | Needs attention | aria/tooltip of the Lojas warning marker |
| GameNode Desktop {version} | GameNode Desktop {version} | nav footer |
| Alterado | Changed | accessible description of the changed dot |
| Antes: {value} | Before: {value} | previous saved value on a changed row |
| Alterações não salvas | Unsaved changes | unsaved bar |
| {n} configuração alterada / {n} configurações alteradas | {n} setting changed / {n} settings changed | plural |
| Reverter | Revert | |
| Salvar alterações | Save changes | |
| Configurações salvas | Settings saved | toast after save |

## S1 Conta e sincronização

| pt-BR | English | Notes |
|---|---|---|
| Sua conta GameNode e o que este PC envia para ela. | Your GameNode account and what this PC sends to it. | panel description |
| Sessão salva no cofre do Windows | Session saved in the Windows vault | storage = keyring on Windows |
| Sessão salva no chaveiro do sistema | Session saved in the system keyring | storage = keyring elsewhere |
| Sessão salva em arquivo privado | Session saved in a private file | storage = private file fallback |
| Conectado | Signed in | pill |
| Abrir perfil | Open profile | opens the GameNode profile in the browser |
| Sair | Sign out | existing meaning |
| Tempo de jogo e sincronização | Playtime & sync | card header |
| Sincronizar com o GameNode | Sync with GameNode | existing key |
| O tempo de jogo da loja é enviado à sua conta. Desligado, a biblioteca continua funcionando só neste PC. | Store playtime is sent to your account. When off, the library keeps working on this PC only. | |
| Fonte do tempo de jogo | Playtime source | existing key |
| Use o contador da loja quando existir, ou sempre o contador interno do GameNode. | Use the store's counter when there is one, or always GameNode's own counter. | |
| Loja / Interno | Store / Internal | existing keys |
| Última sincronização | Last sync | |
| {games} jogos · {stores} lojas · nenhum erro | {games} games · {stores} stores · no errors | plurals per part |
| {games} jogos · {stores} lojas · {pending} pendentes | {games} games · {stores} stores · {pending} pending | when totals are held |
| Nunca sincronizado | Never synced | no completed pass |
| Sincronização desligada | Sync is off | when the switch is off; "Sincronizar agora" disabled |
| há {relative} | {relative} ago | relative time formatter |
| Sincronizar agora | Sync now | |
| Sincronizando… | Syncing… | busy |
| O que é sincronizado | What is synced | card header |
| Tempo de jogo e última sessão | Playtime and last session | sent |
| Status e coleções escolhidos | Status and chosen collections | sent |
| Arquivos de save | Save files | never sent |
| Caminhos e executáveis locais | Local paths and executables | never sent |
| Mostrar “Jogando agora” no perfil / Seus amigos no GameNode veem… | — | **OUT** |

## S2 Lojas

| pt-BR | English | Notes |
|---|---|---|
| Contas de loja conectadas a este PC. O GameNode nunca vê suas senhas. | Store accounts connected to this PC. GameNode never sees your passwords. | |
| Conta {account} · sincronizado {relative} | Account {account} · synced {relative} | status line, connected |
| O login expirou. Exibindo sua biblioteca salva ({n} jogos). | Sign-in expired. Showing your saved library ({n} games). | expired |
| Conectado / Sessão expirada / Não conectado / Entrando… / Verificando… / Erro de credencial | Connected / Session expired / Not connected / Signing in… / Checking… / Credential error | state pills; reuse existing state labels where they exist |
| Sincronizar agora | Sync now | store check |
| Entrar / Entrar de novo / Tentar de novo / Cancelar entrada / Mostrar janela de entrada | Sign in / Sign in again / Try again / Cancel sign-in / Show sign-in window | primary/overflow actions |
| Mais ações | More actions | overflow aria |
| Desconectar | Sign out of store | overflow item (existing meaning) |
| na biblioteca | in library | stat |
| compartilhados pela família | shared by family | stat; for other providers use "compartilhados ({sharedLabel})" / "shared ({sharedLabel})" |
| instalados neste PC | installed on this PC | stat |
| Incluir jogos compartilhados pela família | Include family-shared games | provider with shared access |
| Mostra os jogos da Família Steam com o selo “Compartilhado”. | Shows {sharedLabel} games with the “Shared” badge. | `{sharedLabel}` from the provider |
| Remover login salvo | Remove saved sign-in | |
| Os jogos instalados permanecem na sua biblioteca. | Installed games stay in your library. | |
| Remover | Remove | |
| Armazenamento de credenciais | Credential storage | card header |
| Gerenciador de Credenciais do Windows | Windows Credential Manager | Windows keyring |
| Chaveiro do sistema (Secret Service) | System keyring (Secret Service) | Linux keyring |
| Os logins das lojas ficam cifrados pelo Windows, nunca em arquivos do GameNode. | Store sign-ins are encrypted by Windows, never kept in GameNode files. | |
| Disponível / Indisponível | Available / Unavailable | pill |
| Detectadas neste PC | Detected on this PC | card header |
| {path} · {n} bibliotecas | {path} · {n} libraries | plural; path or count omitted when unknown |
| Detectada / Não encontrada / Falha na verificação | Detected / Not found / Scan failed | pills |
| {n} instalados | {n} installed | fallback meta when the install path is unknown |
| Reescanear | Rescan | header action (existing key) |
| Mostrar jogos ocultos | Show hidden games | existing "reset hidden games" action, kept in the Detectadas card |

## S3 Jogos DRM-free

| pt-BR | English | Notes |
|---|---|---|
| Pastas monitoradas e descobertas locais. | Watched folders and local discoveries. | |
| Verificar tudo agora | Check all now | |
| Pastas monitoradas | Watched folders | card header |
| Adicionar pasta… | Add folder… | native folder picker |
| {n} jogos · última verificação {time} | {n} games · last checked {time} | |
| {n} jogos · unidade desconectada desde {relative} | {n} games · drive disconnected since {relative} | unavailable |
| Monitorando / Indisponível / Verificando… | Watching / Unavailable / Checking… | pills |
| Verificar | Check | per folder |
| Parar de monitorar | Stop watching | |
| Nenhuma pasta monitorada | No watched folders | empty state row |
| Descoberta | Discovery | card header |
| Verificar ao iniciar o GameNode | Check when GameNode starts | |
| Procura executáveis novos nas pastas monitoradas. | Looks for new executables in watched folders. | |
| Acompanhar mudanças em tempo real | Watch for changes in real time | |
| Detecta jogos copiados ou removidos sem precisar verificar. | Detects copied or removed games without a manual check. | |
| Descobertas ignoradas | Ignored discoveries | |
| {n} item marcado como “não importar”. / {n} itens marcados como “não importar”. | {n} item marked “don't import”. / {n} items marked “don't import”. | plural |
| Redefinir descobertas ignoradas | Reset ignored discoveries | |
| Adicionar manualmente | Add manually | card header |
| Adicionar a partir do executável | Add from executable | |
| Adicione um jogo sem DRM a partir do executável sem escolher uma correspondência do GameNode primeiro. | Add a DRM-free game from its executable without choosing a GameNode match first. | |
| Escolher executável… | Choose executable… | |

## S4 Backups

| pt-BR | English | Notes |
|---|---|---|
| Proteção local dos saves para jogos DRM-free. | Local save protection for DRM-free games. | |
| Proteção automática | Automatic protection | card header |
| Proteger saves automaticamente | Protect saves automatically | |
| Cria uma cópia local após cada sessão de jogo nos jogos com pasta de saves conhecida. | Makes a local copy after each play session for games with a known save folder. | |
| Destino dos backups | Backup destination | |
| Escolher pasta… | Choose folder… | |
| Cópias mantidas por jogo | Copies kept per game | |
| As mais antigas são apagadas primeiro. | The oldest are deleted first. | |
| {n} cópias | {n} copies | select options 5/10/20/50 |
| Uso do espaço | Space used | card header |
| {size} · {n} cópias | {size} · {n} copies | total line and per-game rows |
| Nenhum backup ainda | No backups yet | empty |
| Limite de espaço / Avisa antes de passar do limite. / "de {quota}" / "{n}%" | — | **OUT** |

## S5 Ferramentas

| pt-BR | English | Notes |
|---|---|---|
| Ferramentas de terceiros que o GameNode instala e atualiza por jogo. | Third-party tools GameNode installs and updates per game. | |
| Ferramentas suportadas | Supported tools | card header |
| Upscaling FSR, XeSS e DLSS em jogos compatíveis. Instale pelo painel do jogo. | FSR, XeSS and DLSS upscaling in compatible games. Install it from the game's panel. | OptiScaler summary (catalog summary may override) |
| Em {n} jogo / Em {n} jogos | In {n} game / In {n} games | pill |
| Não instalado | Not installed | neutral pill when 0 |
| Ver jogos | View games | menu of games with the tool |
| Licença {license} · Código-fonte | License {license} · Source code | kept from the current panel (meta line under the summary) |
| Procurar atualizações automaticamente | Check for updates automatically | tool releases, not the app updater |
| Avisa quando houver versão estável nova; nada é instalado sem confirmação. | Tells you when a new stable version is out; nothing is installed without confirmation. | |
| Canal padrão | Default channel | |
| Usado ao instalar em um jogo novo. | Used when installing in a new game. | |
| Estável / Nightly | Stable / Nightly | |
| Cache de downloads | Download cache | card header |
| Limpar cache | Clear cache | existing |
| {size} · {n} versões em cache | {size} · {n} cached versions | |
| Limpar o cache nunca remove ferramentas instaladas. | Clearing the cache never removes installed tools. | |
| Em uso | In use | |
| verificado SHA-256 | SHA-256 verified | |
| Ferramentas auxiliares / Cache de ports | Helper tools / Ports cache | existing Windows-only groups, restyled |

## S6 Plugins

| pt-BR | English | Notes |
|---|---|---|
| Integrações de loja que rodam isoladas, com permissões explícitas. | Store integrations that run as separate processes with declared permissions. | must not claim sandboxing (desktop-plugins) |
| Todos · {n} / Oficiais · {n} / Comunidade · {n} | All · {n} / Official · {n} / Community · {n} | filter |
| Oficial / Comunidade | Official / Community | origin pills |
| Copiar identificador | Copy identifier | aria of the id button; toast "Identificador copiado" / "Identifier copied" |
| Sessão ativa / Parada / Indisponível / Falhou / Incompatível / Ausente | Session active / Stopped / Unavailable / Failed / Incompatible / Missing | status pill |
| Parar / Iniciar | Stop / Start | |
| Permissões | Permissions | |
| Rede / Arquivos / Processos / Registro / D-Bus | Network / Files / Processes / Registry / D-Bus | native access pills |
| Detalhes / Ocultar detalhes | Details / Hide details | |
| Permissões da API / Acesso nativo ao sistema | API permissions / Native system access | detail section labels (existing meaning) |
| A sessão desta loja expirou. Entre de novo em Lojas. | This store's session expired. Sign in again in Stores. | "Lojas" navigates to the Stores topic |
| Plugins da comunidade não são revisados pelo GameNode. Conceda só as permissões que fizerem sentido. | Community plugins aren't reviewed by GameNode. Only grant permissions that make sense. | |

## S7 Sobre

| pt-BR | English | Notes |
|---|---|---|
| Versão, diagnóstico e licenças. | Version, diagnostics and licenses. | |
| Build {date} · {os} {arch} | Build {date} · {os} {arch} | no channel |
| Atualizado / Procurar atualizações / canal Estável | — | **OUT** |
| Diagnóstico | Diagnostics | card header |
| Log recente | Recent log | |
| Últimas 500 linhas, sem dados de login. | Last 500 lines, without sign-in data. | |
| Mostrar log recente / Abrir pasta de logs | Show recent log / Open log folder | existing |
| Informações do sistema | System information | |
| Versão, plugins e lojas detectadas, para anexar em um relato de problema. | Version, plugins and detected stores, to attach to a bug report. | |
| Copiar | Copy | toast "Informações copiadas" / "Information copied" |
| Licenças | Licenses | card header |
| Avisos de licença | License notices | |
| O GameNode Desktop inclui software de código aberto. | GameNode Desktop includes open-source software. | |
| Mostrar avisos de licença | Show license notices | existing |
| Site do GameNode / Termos de uso / Privacidade / Relatar um problema | GameNode website / Terms of use / Privacy / Report a problem | footer links |

## S8 Geral

| pt-BR | English | Notes |
|---|---|---|
| Inicialização, aparência da biblioteca e idioma. | Startup, library appearance and language. | |
| Inicialização | Startup | card header |
| Iniciar com o Windows | Start with Windows | Windows only |
| Mantém o contador de tempo ativo desde o login. | Keeps the playtime counter running from sign-in. | |
| Iniciar minimizado na bandeja | Start minimized to the tray | applies to login launches |
| Fechar a janela minimiza para a bandeja | Closing the window minimizes to the tray | |
| O GameNode continua contando o tempo de jogo em segundo plano. | GameNode keeps counting playtime in the background. | |
| Desligado, fechar a janela encerra o GameNode e para a contagem de tempo. | When off, closing the window quits GameNode and stops counting playtime. | shown as help when the switch is off |
| Biblioteca | Library | card header |
| Visualização padrão | Default view | |
| Grade / Lista | Grid / List | |
| Tamanho das capas | Cover size | |
| P / M / G | S / M / L | segment labels; aria "Pequeno/Médio/Grande" / "Small/Medium/Large" |
| Abrir em | Open in | |
| Última visualização / Todos os jogos / Instalados / Favoritos / Recentes | Last view / All games / Installed / Favorites / Recent | startup scope options |
| Mostrar barra de status | Show status bar | |
| Idioma e atalhos | Language & shortcuts | card header |
| Idioma | Language | |
| Reinicia a interface sem fechar jogos em execução. | Reloads the interface without closing running games. | |
| Idioma do sistema / English / Português (Brasil) | System language / English / Português (Brasil) | language names are shown in their own language |
| Atalhos de teclado | Keyboard shortcuts | |
| Ctrl+K, Ctrl+F, Ctrl+, e outros. | Ctrl+K, Ctrl+F, Ctrl+, and more. | use the platform modifier label |
| Ver atalhos | View shortcuts | opens `ShortcutsModal` |
