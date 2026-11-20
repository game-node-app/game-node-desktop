# Copy

pt-BR comes from Figma (fixture values replaced by `{placeholders}`); English is the source string for
`ui/src/i18n/en.ts`. Reuse existing keys where the meaning is unchanged (status names, Cancel, Close,
Restore, provider names). Plurals use the existing i18n plural helper; numbers, sizes and dates use the
locale formatters in `features/library/formatters.ts`.

## Shared

| Key idea | pt-BR | English |
|---|---|---|
| Subtitle pattern | `{game} · {provider}` | `{game} · {provider}` |
| Close dialog (aria) | Fechar | Close |
| Drag handle (aria, header) | Mover janela | Move window |
| Cancel | Cancelar | Cancel |

## D01 Match dialog

| pt-BR | English | Notes |
|---|---|---|
| Associar a um jogo do GameNode | Match to a GameNode game | title (replaces "Match to a GameNode game") |
| Buscar no GameNode | Search GameNode | field label; Enter searches |
| Várias correspondências possíveis | Several possible matches | list label when the server offered candidates |
| Resultados da busca | Search results | list label when only search results are shown |
| {n} resultado / {n} resultados | {n} result / {n} results | |
| `{year} · {platforms}` | `{year} · {platforms}` | option meta; parts omitted when unknown |
| Por que sugerimos | Why we suggest it | preview section label (no "NOVO") |
| Título {n}% parecido | Title {n}% similar | positive when ≥ 80%, warning otherwise |
| Mesmo ano de lançamento | Same release year | only when a year was detected |
| Ano de lançamento diferente ({year}) | Different release year ({year}) | warning |
| ID da loja reconhecido | Store ID recognized | candidate came from the store id |
| Escolhido por você | Chosen by you | shown on the current chosen match |
| Associação atual | Current match | pill on the option that is the current match |
| Nenhum jogo encontrado. Tente outras palavras. | No games found. Try different words. | existing |
| Buscando… | Searching… | existing |
| Selecione um jogo para ver os detalhes. | Select a game to see its details. | preview empty state |
| Não está no GameNode | Not on GameNode | existing |
| Usar a correspondência automática | Use the automatic match | existing; only when a choice exists |
| Associar | Match | primary, disabled until an option is selected |

## D02 Add to GameNode

| pt-BR | English | Notes |
|---|---|---|
| Adicionar ao GameNode | Add to GameNode | title and primary |
| Editar no GameNode | Edit on GameNode | title when editing an existing entry; primary becomes "Salvar" / "Save" |
| Status | Status | section label |
| Jogando · Zerado · Na lista · Abandonado · Em andamento | Playing · Finished · Planned · Dropped · Ongoing | existing `Library::` keys |
| Sem escolha, o GameNode usará {status}, o status padrão de {source}. | With no choice, GameNode uses {status}, the default status of {source}. | `{source}` is the collection name or the provider default |
| Coleções (opcional) | Collections (optional) | section label |
| Nova coleção | New collection | text button with `+` icon |
| Nome da coleção | Collection name | inline create field placeholder |
| Criar | Create | inline create confirm |
| {n} jogo / {n} jogos | {n} game / {n} games | collection count |
| Não foi possível criar a coleção. | The collection could not be created. | inline error, keeps the typed name |
| Você ainda não tem coleções. | You have no collections yet. | empty list |

## D03 Batch progress

| pt-BR | English |
|---|---|
| Adicionando {n} jogos ao GameNode | Adding {n} games to GameNode |
| Status: {status} · Coleção: {names} | Status: {status} · Collection: {names} |
| {done} de {total} | {done} of {total} |
| Adicionado | Added |
| Adicionando… | Adding… |
| Aguardando | Waiting |
| Já estava na biblioteca | Already in the library |
| Ignorado: sem correspondência no GameNode | Skipped: no GameNode match |
| Falhou: {reason} | Failed: {reason} |
| Pode fechar: o envio continua em segundo plano. | You can close this: adding continues in the background. |
| Ocultar | Hide |

## D04 Achievements

| pt-BR | English | Notes |
|---|---|---|
| Conquistas | Achievements | title |
| {unlocked} de {total} desbloqueadas | {unlocked} of {total} unlocked | |
| {percent}% | {percent}% | |
| Todas · {n} / Desbloqueadas · {n} / Bloqueadas · {n} | All · {n} / Unlocked · {n} / Locked · {n} | segments |
| Ordenar | Sort | aria label of the sort select |
| Mais recentes | Most recent | default sort (current order) |
| Mais raras | Rarest | |
| Mais comuns | Most common | |
| Nome | Name | |
| Bloqueada | Locked | |
| Rara · {percent}% dos jogadores | Rare · {percent}% of players | "Rara" when < 10% |
| {percent}% dos jogadores | {percent}% of players | |
| Progresso {current}/{max} | Progress {current}/{max} | only when the provider reports it |
| Conquista oculta | Hidden achievement | existing behavior |
| Desbloqueie esta conquista para revelá-la. | Unlock this achievement to reveal it. | |
| Atualizado da {provider} há {relative} | Updated from {provider} {relative} | `{relative}` = locale relative time |
| Abrir na {provider} | Open in {provider} | only when the provider has an achievements page |

## D05 Local save protection

| pt-BR | English |
|---|---|
| Proteção local dos saves | Local save protection |
| Proteção ativa | Protection on |
| Proteção desligada | Protection off |
| Cria uma cópia local após cada sessão de jogo. | Makes a local copy after each play session. |
| Só cria cópias quando você pedir. | Only makes copies when you ask. |
| Pasta de saves | Save folder |
| Alterar… | Change… |
| Manter | Keep |
| {n} cópias mais recentes | {n} most recent copies |
| Espaço usado | Space used |
| {size} em {n} cópias | {size} in {n} copies |
| Histórico de backups | Backup history |
| Restaure uma cópia anterior. Um backup de recuperação é criado antes de substituir os arquivos. | Restore an earlier copy. A recovery backup is made before files are replaced. |
| Hoje, {time} / Ontem, {time} / {date}, {time} | Today, {time} / Yesterday, {time} / {date}, {time} |
| Após sessão de jogo · {duration} | After a play session · {duration} |
| Manual | Manual |
| Backup de recuperação | Recovery backup |
| Restaurar | Restore |
| Fazer cópia agora | Back up now |
| Abrir pasta | Open folder |
| Fechar | Close |
| Nenhuma cópia ainda. | No copies yet. |

## D06 Game tools

| pt-BR | English |
|---|---|
| Ferramentas do jogo | Game tools |
| Não instalado / Instalado · {version} / Atualização disponível | Not installed / Installed · {version} / Update available |
| Canal | Channel |
| Estável / Nightly | Stable / Nightly |
| Versão | Version |
| {version} (recomendada) | {version} (recommended) |
| Nome da DLL proxy | Proxy DLL name |
| Executável de destino | Target executable |
| Procurar… | Browse… |
| Executáveis candidatos detectados (selecione para substituir): | Detected candidate executables (select one to replace it): |
| {file} (inicializador) | {file} (launcher) |
| Redefinir {config} para os padrões ao atualizar | Reset {config} to defaults when updating |
| Anti-cheat detectado ({name}) | Anti-cheat detected ({name}) |
| Instalar DLLs proxy em jogos online com anti-cheat pode resultar em banimento. | Installing proxy DLLs in online games with anti-cheat can get you banned. |
| Compreendo o risco do anti-cheat e desejo prosseguir mesmo assim | I understand the anti-cheat risk and want to continue anyway |
| Download de {size}, verificado por SHA-256 | {size} download, verified with SHA-256 |
| Já baixado, verificado por SHA-256 | Already downloaded, verified with SHA-256 |
| Instalar {tool} / Atualizar {tool} / Desinstalar | Install {tool} / Update {tool} / Uninstall |

## D07 Game settings (DRM-free)

| pt-BR | English |
|---|---|
| Configurações do jogo | Game settings |
| Trocar capa… | Change cover… |
| Usar capa do GameNode | Use the GameNode cover |
| Título | Title |
| Executável | Executable |
| Localizar executável | Locate executable |
| Vários executáveis possíveis; escolha um abaixo. | Several possible executables; choose one below. |
| Pasta do jogo | Game folder |
| Diretório de trabalho | Working directory |
| Argumentos | Arguments |
| Avançado · detecção de processo | Advanced · process detection |
| Executáveis do jogo (um por linha) | Game executables (one per line) |
| Executáveis de inicialização (um por linha) | Launcher executables (one per line) |
| Remover da biblioteca… | Remove from library… |
| Salvar | Save |

## D08 Discovered games

| pt-BR | English |
|---|---|
| Jogos descobertos | Discovered games |
| {n} jogos novos nas pastas monitoradas | {n} new games in watched folders |
| Jogo / Executável / GameNode | Game / Executable / GameNode |
| Selecionar todos | Select all (aria) |
| {title} ({year}) | {title} ({year}) |
| Sem correspondência | No match |
| Provavelmente não é jogo | Probably not a game |
| Permitir redescoberta em verificações futuras | Allow rediscovery in future scans |
| Ignorar selecionados | Ignore selected |
| Adicionar {n} jogos / Adicionar 1 jogo | Add {n} games / Add 1 game |

## D09 Remove game

| pt-BR | English | Notes |
|---|---|---|
| Remover {game} da biblioteca? | Remove {game} from the library? | |
| Os arquivos do jogo no disco não são apagados. | Game files on disk are not deleted. | |
| Apenas remover | Just remove | `allowRediscovery = true` |
| Pode voltar na próxima verificação das pastas monitoradas. | It can come back at the next watched-folder scan. | |
| Remover e não importar mais | Remove and don't import again | `allowRediscovery = false` |
| A pasta passa a ser ignorada. Desfaça em Configurações › Jogos DRM-free. | The folder will be ignored. Undo it in Settings › DRM-free games. | |
| Remover | Remove | danger |

## D10 Discard changes

| pt-BR | English |
|---|---|
| Descartar alterações? | Discard changes? |
| Você tem alterações não salvas. Deseja descartá-las? | You have unsaved changes. Discard them? |
| Continuar editando | Keep editing |
| Descartar | Discard |
| Field names listed: Destino dos backups · Proteção automática · Título · Executável · … | Backup destination · Automatic protection · Title · Executable · … |

## D11 Claim conflict

| pt-BR | English |
|---|---|
| Este jogo já é monitorado por uma loja | This game is already tracked by a store |
| Use o item existente ou transfira o monitoramento para o cadastro DRM-free. O tempo será contado uma única vez. | Use the existing item or move tracking to the DRM-free entry. Playtime is counted only once. |
| {provider} · {playtime} | {provider} · {playtime} |
| DRM-free · {path} | DRM-free · {path} |
| Usar item existente | Use existing item |
| Transferir monitoramento | Move tracking |

## D12 Sync explanation

| pt-BR | English | Notes |
|---|---|---|
| Tempo de jogo e sincronização | Playtime and sync | |
| A sincronização está ativada. O tempo de jogo é enviado à sua conta GameNode. Você pode desativá-la a qualquer momento nas Configurações e continuar usando sua biblioteca. | Sync is on. Your playtime is sent to your GameNode account. You can turn it off at any time in Settings and keep using your library. | Figma said "da loja"; playtime from the internal counter is sent too |
| Tempo de jogo e sessões das lojas e do contador interno | Playtime and sessions from stores and the built-in tracker | sends |
| Jogos detectados neste PC e suas correspondências | Games detected on this PC and their matches | sends |
| Favoritos | Favorites | sends |
| Arquivos de save (ficam só neste PC) | Save files (stay on this PC) | stays |
| Caminhos de instalação e executáveis | Install paths and executables | stays |
| Desativar sincronização | Turn sync off | |
| Manter sincronização | Keep sync on | |
| Existing tray note (when background is supported) | unchanged | kept under the list |

## D13 Sign-in pending

| pt-BR | English |
|---|---|
| Conclua a entrada no navegador. | Finish signing in in your browser. |
| Abrimos a página do GameNode no seu navegador padrão. Volte para cá quando terminar. | We opened the GameNode page in your default browser. Come back here when you're done. |
| Aguardando confirmação… | Waiting for confirmation… |
| Confirmando sua conta… | Confirming your account… |
| Abrir de novo | Open again |
| Cancelar entrada | Cancel sign-in |

## D14 Signed-out view

| pt-BR | English |
|---|---|
| Todas as suas lojas, um só lugar. | All your stores in one place. |
| Steam, Epic Games Store e jogos DRM-free sincronizados com o seu perfil GameNode. | Steam, Epic Games Store and DRM-free games synced with your GameNode profile. |
| Boas-vindas ao GameNode | Welcome to GameNode |
| Entre com sua conta GameNode pelo navegador. | Sign in with your GameNode account in your browser. |
| Continuar com {provider} | Continue with {provider} |
| Entre com segurança pelo seu navegador padrão. | Sign in securely through your default browser. |
| GameNode Desktop · versão {version} | GameNode Desktop · version {version} |
