# Copy (English + Brazilian Portuguese)

Keys follow the existing catalog convention `Namespace::English text` (see `ui/src/i18n/en.ts`). Placeholders are
positional `{}` filled with `formatI18n`. Plurals: add separate `…one`/`…other` keys only where shown. pt-BR text is
taken from Figma verbatim; English is the source key. Reuse an existing key when its English text already matches.
Data (game titles, collection names, store names from manifests, usernames) is never translated.

## Top bar (`Shell::`)
| en | pt-BR |
|---|---|
| DESKTOP | DESKTOP |
| Search games, actions and settings | Buscar jogos, ações e configurações |
| Synced | Sincronizado |
| Syncing… | Sincronizando… |
| Sync off | Sincronização desativada |
| Offline | Offline |
| Account menu | Menu da conta |
| Minimize | Minimizar |
| Maximize | Maximizar |
| Restore | Restaurar |
| Close | Fechar |

## Sidebar (`Sidebar::`)
| en | pt-BR |
|---|---|
| Your space | Seu espaço |
| All games | Todos os jogos |
| Installed | Instalados |
| Favorites | Favoritos |
| Recently played | Jogados recentemente |
| Running | Em execução |
| Stores | Lojas |
| DRM-free games | Jogos DRM-free |
| Collections and views | Coleções e visões |
| New view from current filters | Nova visão a partir dos filtros atuais |
| No collections or views yet | Nenhuma coleção ou visão ainda |
| Browse | Navegar |
| Discovered games | Jogos descobertos |
| Explore GameNode | Explorar no GameNode |
| PC ports | Ports para PC |
| Add DRM-free game | Adicionar jogo DRM-free |
| Settings | Configurações |
| {} needs attention | {} precisa de atenção |
| Rename view | Renomear visão |
| Delete view | Excluir visão |
| Open collection on GameNode | Abrir coleção no GameNode |
| Collapse sidebar | Recolher barra lateral |
| Expand sidebar | Expandir barra lateral |

## Status bar (`StatusBar::`)
| en | pt-BR |
|---|---|
| {} · synced {} | {} · sincronizado {} |
| {} · session expired | {} · sessão expirada |
| {} · unreachable | {} · inacessível |
| {} · checking… | {} · verificando… |
| {} · not synced yet | {} · ainda não sincronizado |
| Watched folders · last check {} | Pastas monitoradas · última verificação {} |
| Watched folders · checking… | Pastas monitoradas · verificando… |
| Running: {} · {} | Em execução: {} · {} |
| just now | agora |
| {} min ago | há {} min |
| {} h ago | há {} h |

## Library header and toolbar (`Library::`)
| en | pt-BR |
|---|---|
| {} games · {} installed on this PC · {} stores | {} jogos · {} instalados neste PC · {} lojas |
| {} games · grouped by {} · {} selected | {} jogos · agrupados por {} · {} selecionados |
| 1 game | 1 jogo |
| {} selected | {} selecionados |
| Rescan | Verificar novamente |
| Add to GameNode | Adicionar ao GameNode |
| Search your games | Buscar nos seus jogos |
| Clear search | Limpar busca |
| Filters | Filtros |
| Sort | Ordenar |
| No grouping | Sem grupos |
| Group: {} | Agrupar: {} |
| Grid | Grade |
| List | Lista |
| Details panel | Painel de detalhes |
| Clear filters | Limpar filtros |
| Showing {} of {} | Mostrando {} de {} |
| Save as view | Salvar como visão |
| Remove filter {} | Remover filtro {} |

## Sort menu (`Library::`)
| en | pt-BR |
|---|---|
| Sort by | Ordenar por |
| Title (A–Z) | Título (A–Z) |
| Title (Z–A) | Título (Z–A) |
| Recently played | Jogado recentemente |
| Playtime | Tempo de jogo |
| Recently added | Adicionado recentemente |
| Release year | Ano de lançamento |
| Match status | Status de correspondência |
| Store | Loja |
| GameNode status | Status no GameNode |
| Descending order | Ordem decrescente |
(The sort button shows the checked item's label. "Title (A–Z)/(Z–A)" both map to key `title` with a direction.)

## Group menu (`Library::`)
| en | pt-BR |
|---|---|
| Group by | Agrupar por |
| No grouping | Sem grupos |
| Store | Loja |
| GameNode status | Status no GameNode |
| Match | Correspondência |
| Installation | Instalação |
| Release decade | Década de lançamento |
| Collapse all groups | Recolher todos os grupos |
| Expand all groups | Expandir todos os grupos |
| {} games (group count) | {} jogos |
| Installed on this PC (group) | Instalados neste PC |
| Not installed (group) | Não instalados |
| Not on GameNode (group) | Fora do GameNode |
| Unknown release (group) | Lançamento desconhecido |
| Before 1990 (group) | Antes de 1990 |
| {}s (decade, e.g. "2010s") | Anos {} |

## Filters popover (`Library::`)
| en | pt-BR |
|---|---|
| Filters | Filtros |
| Search a filter | Buscar um filtro |
| Filters apply immediately | Os filtros são aplicados na hora |
| Availability | Disponibilidade |
| Installed on this PC | Instalado neste PC |
| In library | Na biblioteca |
| Family shared | Compartilhado pela família |
| Store | Loja |
| Shortcuts | Atalhos |
| Favorites only | Somente favoritos |
| Running only | Somente em execução |
| Show hidden | Mostrar ocultos |
| Match | Correspondência |
| Matched | Associado |
| Needs review | Precisa de revisão |
| No match | Sem correspondência |
| Marked as not a game | Marcado como não-jogo |
| GameNode status | Status no GameNode |
| Playing | Jogando |
| Finished | Zerado |
| Planned | Na lista |
| Ongoing | Em andamento |
| Dropped | Abandonado |
| Not on GameNode | Fora do GameNode |
| Playtime | Tempo de jogo |
| Any | Qualquer |
| 10 h+ | 10 h+ |
| 100 h+ | 100 h+ |
| History | Histórico |
| All | Todos |
| Played | Jogados |
| Never played | Nunca jogados |
| Last session | Última sessão |
| 7 days | 7 dias |
| 30 days | 30 dias |
| 90 days | 90 dias |
| Last year | Último ano |
| Older | Mais antigo |
| Release | Lançamento |
| Before 1990 | Antes de 1990 |
| Unknown | Desconhecido |
| Added | Adicionado |
| This week | Esta semana |
| This month | Este mês |
| This year | Este ano |
| Earlier | Antes |
| Clear all | Limpar tudo |
| Done | Concluído |
| No filter matches “{}” | Nenhum filtro corresponde a “{}” |
Chip labels: `Store:` `Loja:` · `Playtime:` `Tempo de jogo:` · `History:` `Histórico:` · `Last session:` `Última sessão:` ·
`Release:` `Lançamento:` · `Added:` `Adicionado:` · `Availability:` `Disponibilidade:` · `Match:` `Correspondência:` ·
`GameNode status:` `Status no GameNode:` · `Favorites only` `Somente favoritos` · `Running only` `Somente em execução` ·
`Show hidden` `Mostrar ocultos`. Multi-valued chips join values with ", ".

## GameNode status tone map (pills)
| status | en / pt-BR pill | detail pill (brand pill in detail state row) | tone |
|---|---|---|---|
| playing | Playing / Jogando | Playing on GameNode / Jogando no GameNode | brand |
| finished | Finished / Zerado | Finished on GameNode / Zerado no GameNode | success |
| planned | Planned / Na lista | Planned on GameNode / Na lista do GameNode | neutral |
| ongoing | Ongoing / Em andamento | Ongoing on GameNode / Em andamento no GameNode | neutral |
| dropped | Dropped / Abandonado | Dropped on GameNode / Abandonado no GameNode | danger |
| in library, no status | In library / Na biblioteca | In your GameNode library / Na sua biblioteca do GameNode | neutral |
| not in library | — | (no pill) | — |

## Grid / list (`Library::`)
| en | pt-BR |
|---|---|
| Installed | Instalado |
| Not installed | Não instalado |
| Running | Em execução |
| Running · {} | Em execução · {} |
| Family | Família |
| Family shared | Compartilhado pela família |
| Hidden | Oculto |
| Title | Título |
| Store | Loja |
| On GameNode | No GameNode |
| Match | Correspondência |
| Time | Tempo |
| Last session | Última sessão |
| Select all | Selecionar tudo |
| Matched | Associado |
| Review | Revisar |
| No match | Sem correspondência |
| Not a game | Não é jogo |
| Now | Agora |
| Yesterday | Ontem |
| {} days ago | {} dias atrás |
| Last week | Semana passada |
| Never | Nunca |
Month-year ("Mar. 2026") and year ("2025") use `Intl.DateTimeFormat(locale, {month:'short', year:'numeric'})`; pt-BR
capitalizes the month ("Mar. 2026"). Short playtime "42 h" / "45 min"; long "42 h 18 min".

## Batch bar (`Library::`)
| en | pt-BR |
|---|---|
| games selected | jogos selecionados |
| Add to GameNode… | Adicionar ao GameNode… |
| Change status | Alterar status |
| Favorite | Favoritar |
| Unfavorite | Desfavoritar |
| Match… | Associar… |
| Clear | Limpar |

## Context menu (`Library::`)
| en | pt-BR |
|---|---|
| Play | Jogar |
| Install | Instalar |
| Open install folder | Abrir pasta de instalação |
| Add to GameNode… | Adicionar ao GameNode… |
| Edit on GameNode… | Editar no GameNode… |
| Change match… | Alterar correspondência… |
| Match… | Associar… |
| Add to favorites | Adicionar aos favoritos |
| Remove from favorites | Remover dos favoritos |
| Add to collection | Adicionar à coleção |
| Add to GameNode first | Adicione ao GameNode primeiro |
| No collections | Nenhuma coleção |
| Game tools… | Ferramentas do jogo… |
| Local save protection | Proteção local dos saves |
| View achievements | Ver conquistas |
| Hide from library | Ocultar da biblioteca |
| Show in library | Mostrar na biblioteca |
| Game settings… | Configurações do jogo… |
| Locate executable… | Localizar executável… |
| Remove from library… | Remover da biblioteca… |
| {} · Installed | {} · Instalado |
| {} · Not installed | {} · Não instalado |

## Detail panel (`Detail::`)
| en | pt-BR |
|---|---|
| Close details | Fechar detalhes |
| Play | Jogar |
| Install | Instalar |
| Running · {} | Em execução · {} |
| More actions | Mais ações |
| Installed on this PC | Instalado neste PC |
| Not installed | Não instalado |
| Matched | Associado |
| DRM-free | DRM-free |
| Saves protected | Saves protegidos |
| Playtime | Tempo de jogo |
| Time | Tempo |
| Last session | Última sessão |
| Session | Sessão |
| Achievements | Conquistas |
| Saves | Saves |
| Steam counter | Contador da Steam |
| Internal counter | Contador interno |
| {} copies | {} cópias |
| Last {} | Última {} |
| Started {} | Iniciada {} |
| On GameNode | No GameNode |
| Edit on GameNode | Editar no GameNode |
| Status | Status |
| Collections | Coleções |
| None | Nenhuma |
| Not in your GameNode library | Fora da sua biblioteca do GameNode |
| Match | Correspondência |
| Change… | Alterar… |
| Matched automatically by store id | Associado automaticamente pelo id da loja |
| Matched automatically by title and year | Associado automaticamente pelo título e ano |
| Chosen by you | Escolhido por você |
| See all | Ver todas |
| On this PC | Neste PC |
| Install folder | Pasta de instalação |
| Playtime counter | Contador de tempo |
| Internal | Interno |
| Game tools | Ferramentas do jogo |
| Game settings | Configurações do jogo |
| Edit… | Editar… |
| Executable | Executável |
| Arguments | Argumentos |
| Save folder | Pasta de saves |
| Tools and protection | Ferramentas e proteção |
| Local save protection | Proteção local dos saves |
| Active | Ativa |
| Off | Desativada |
| Remove from library… | Remover da biblioteca… |
| No match on GameNode | Sem correspondência no GameNode |
| Match to sync status, collections and playtime with your profile. | Associe para sincronizar status, coleções e tempo de jogo com o seu perfil. |
| Match… | Associar… |
| Not a game | Não é um jogo |
| Match needs review | Correspondência precisa de revisão |
| Review… | Revisar… |
| Suggestions | Sugestões |
| Use | Usar |
| This game comes from Steam Family. Playtime and achievements are your own; access may pause while the owner is playing. | Este jogo vem da Família Steam. O tempo de jogo e as conquistas são da sua conta; o acesso pode ser pausado quando o dono estiver jogando. |
| {} games selected | {} jogos selecionados |
| {} · all installed | {} · todos instalados |
| {} · {} installed | {} · {} instalados |
| {} stores | {} lojas |
| Total time | Tempo total |
| Sum of stores | Soma das lojas |
| {} of {} | {} de {} |
| {} not on GameNode | {} fora |
| Change status on GameNode | Alterar status no GameNode |
| Favorite all | Favoritar todos |
| Unfavorite all | Desfavoritar todos |
| Save to collection | Salvar em coleção |
| Selection | Seleção |
| Remove {} from selection | Remover {} da seleção |

## States (`Library::`)
| en | pt-BR |
|---|---|
| Your library is empty | Sua biblioteca está vazia |
| Connect a store or choose a DRM-free games folder to get started. | Conecte uma loja ou escolha uma pasta de jogos DRM-free para começar. |
| Sign in to GameNode | Entrar no GameNode |
| Account {} connected | Conta {} conectada |
| Connect a store | Conectar uma loja |
| Steam or Epic Games Store | Steam ou Epic Games Store |
| Connect | Conectar |
| Add a DRM-free folder | Adicionar pasta DRM-free |
| GameNode finds the executables | O GameNode encontra os executáveis |
| Choose folder… | Escolher pasta… |
| No game matches “{}” | Nenhum jogo corresponde a “{}” |
| No game matches the filters | Nenhum jogo corresponde aos filtros |
| 1 active filter may be hiding results. | 1 filtro ativo pode estar escondendo resultados. |
| {} active filters may be hiding results. | {} filtros ativos podem estar escondendo resultados. |
| Search GameNode | Buscar no GameNode |
| Scanning the library… | Verificando a biblioteca… |
| Cancel | Cancelar |
| DRM-free folders | Pastas DRM-free |
| The {} session expired | A sessão da {} expirou |
| Sign in again to refresh the {} games from this store. | Entre de novo para atualizar os {} jogos desta loja. |
| Sign in again | Entrar novamente |
| Couldn't reach {} | Não foi possível acessar a {} |
| We'll try again in {} min. Your games are still available. | Tentaremos de novo em {} min. Os jogos continuam disponíveis. |
| Try now | Tentar agora |
| You're offline | Você está offline |
| Showing the last synced copy. Playing still works. | Mostrando a última cópia sincronizada. Jogar continua funcionando. |
| {} new games discovered | {} jogos novos descobertos |
| 1 new game discovered | 1 jogo novo descoberto |
| Found in watched folders. | Encontrados nas pastas monitoradas. |
| Review | Revisar |
| Dismiss | Dispensar |
(The existing "no installed games for provider X" and "empty Steam library" states keep their current copy, restyled with
the same icon-circle layout.)

## Notifications (`Notify::`)
| en | pt-BR |
|---|---|
| {} games added to GameNode | {} jogos adicionados ao GameNode |
| 1 game added to GameNode | 1 jogo adicionado ao GameNode |
| Status: {} · Collection: {} | Status: {} · Coleção: {} |
| Undo | Desfazer |
| View on GameNode | Ver no GameNode |
| Add undone | Adição desfeita |
| Couldn't undo the add | Não foi possível desfazer a adição |
| Status changed to {} for {} games | Status alterado para {} em {} jogos |
| {} not on GameNode were skipped | {} fora do GameNode foram ignorados |
| Added to {} | Adicionado a {} |
| Couldn't update GameNode | Não foi possível atualizar o GameNode |
| {} hidden from the library | {} ocultado da biblioteca |
| Show again | Mostrar de novo |
| Downloading {} | Baixando {} |
| {} · {}% | {} · {}% |
| Couldn't start {} | Não foi possível iniciar {} |
| The executable was not found. | O executável não foi encontrado. |
| Locate executable | Localizar executável |
| View log | Ver log |
| View “{}” saved | Visão “{}” salva |

## App menu (`AppMenu::`)
| en | pt-BR |
|---|---|
| Connected to GameNode | Conectado ao GameNode |
| Offline | Offline |
| Library | Biblioteca |
| Add a game | Adicionar um jogo |
| Rescan library | Verificar biblioteca novamente |
| Settings | Configurações |
| Keyboard shortcuts | Atalhos de teclado |
| About and licenses | Sobre e licenças |
| Sign out | Sair da conta |
| Quit GameNode | Encerrar GameNode |

## Command palette (`Palette::`)
| en | pt-BR |
|---|---|
| Search games, actions and settings | Buscar jogos, ações e configurações |
| Games | Jogos |
| Actions | Ações |
| Settings | Configurações |
| Play | Jogar |
| Open | Abrir |
| Install | Instalar |
| {} · Installed · {} | {} · Instalado · {} |
| {} · Not installed | {} · Não instalado |
| Add DRM-free game… | Adicionar jogo DRM-free… |
| Rescan library | Verificar biblioteca novamente |
| Search “{}” on GameNode | Buscar “{}” no GameNode |
| Open keyboard shortcuts | Abrir atalhos de teclado |
| Toggle grid / list | Alternar grade / lista |
| Arrows | Setas |
| navigate | navegar |
| open | abrir |
| game actions | ações do jogo |
| No results | Nenhum resultado |
Settings results use `Settings › {topic} › {item}` built from the existing settings topic/item labels
(e.g. "Configurações › Lojas › Membro da família").

## Save view dialog (`Views::`)
| en | pt-BR |
|---|---|
| Save as view | Salvar como visão |
| Name | Nome |
| Includes | Inclui |
| Appears under “Collections and views” in the sidebar | Aparece em “Coleções e visões” na barra lateral |
| Cancel | Cancelar |
| Save view | Salvar visão |
| Sort: {} | Ordenar: {} |
| Group: {} | Agrupar: {} |
| Rename view | Renomear visão |
| Delete view “{}”? | Excluir a visão “{}”? |
| Delete | Excluir |
| A view with this name already exists | Já existe uma visão com esse nome |

## Keyboard shortcuts dialog (`Shortcuts::`)
| en | pt-BR |
|---|---|
| Keyboard shortcuts | Atalhos de teclado |
| General | Geral |
| Command palette | Paleta de comandos |
| Settings | Configurações |
| Rescan | Verificar novamente |
| Quit | Encerrar |
| Library | Biblioteca |
| Search | Buscar |
| Filters | Filtros |
| Grid / list | Grade / lista |
| Selected game | Jogo selecionado |
| Play | Jogar |
| Favorite | Favoritar |
| Details panel | Painel de detalhes |
| Context menu | Menu de contexto |
| Select several | Selecionar vários |
| Select all | Selecionar tudo |
| Click (key name) | Clique |
Key caps: Ctrl, Shift, Enter, Esc, Tab, F5, F10 are not translated.

## Catalog view (`Catalog::`) — see `catalog-view.md`
| en | pt-BR |
|---|---|
| Explore GameNode | Explorar no GameNode |
| Search the GameNode catalog | Buscar no catálogo do GameNode |
| Find games that aren't on this PC and add them to your GameNode library. | Encontre jogos que não estão neste PC e adicione-os à sua biblioteca do GameNode. |
| Type at least 2 characters | Digite pelo menos 2 caracteres |
| Searching… | Buscando… |
| No results for “{}” | Nenhum resultado para “{}” |
| Couldn't search GameNode | Não foi possível buscar no GameNode |
| Try again | Tentar novamente |
| GameNode can't be reached. Search needs a connection. | Não foi possível acessar o GameNode. A busca precisa de conexão. |
| In your library | Na sua biblioteca |
| On this PC | Neste PC |
| Add to GameNode… | Adicionar ao GameNode… |
| Show in library | Mostrar na biblioteca |
| View on GameNode | Ver no GameNode |
| Results may be incomplete. Refine the search. | Os resultados podem estar incompletos. Refine a busca. |
