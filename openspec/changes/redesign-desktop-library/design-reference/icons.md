# Icons — `@tabler/icons-react` (already a dependency, v3.50)

Figma uses the Tabler set at 16px, stroke 2 (`5356:678`). The React name is `Icon` + PascalCase of the Figma
name, e.g. `device-gamepad-2` → `IconDeviceGamepad2`, `star-filled` → `IconStarFilled`. Pass `size` and
`stroke={2}`; color comes from a Tailwind `text-*` class on the icon (`className="text-text-secondary"`).
Do not add SVG files.

| Figma | React | Used for |
|---|---|---|
| search | IconSearch | command search, library search, filter search, no-results state |
| adjustments-horizontal | IconAdjustmentsHorizontal | Filtros button |
| filter | IconFilter | saved view nav item |
| layout-grid / layout-list | IconLayoutGrid / IconLayoutList | view toggle |
| layout-sidebar-right | IconLayoutSidebarRight | detail pane toggle |
| refresh | IconRefresh | Verificar novamente, app menu, palette |
| arrows-sort | IconArrowsSort | Ordenar button |
| sort-descending / sort-ascending | IconSortDescending / IconSortAscending | sort direction item, sorted column |
| category | IconCategory | Agrupar button |
| star / star-filled | IconStar / IconStarFilled | favorite off/on, Favoritos nav |
| device-desktop | IconDeviceDesktop | installed badge/flag, Instalados nav, availability option |
| users-group | IconUsersGroup | family shared |
| download | IconDownload | not installed, Instalar, download notification |
| chevron-right / chevron-down / chevron-left | IconChevronRight / IconChevronDown / IconChevronLeft | rows, dropdowns, groups, sidebar collapse |
| x | IconX | close, clear, dismiss, remove chip, window close |
| plus | IconPlus | Adicionar ao GameNode, Adicionar jogo DRM-free, new view |
| minus | IconMinus | window minimize, collapse all groups |
| square / copy | IconSquare / IconCopy | window maximize / restore (when maximized) |
| books | IconBooks | Todos os jogos, empty state, app menu Biblioteca, Na biblioteca |
| history | IconHistory | Jogados recentemente |
| player-play / player-play-filled | IconPlayerPlay / IconPlayerPlayFilled | Em execução nav, Jogar, running badge |
| brand-steam | IconBrandSteam | Steam (store icon) |
| device-gamepad-2 | IconDeviceGamepad2 | Epic Games Store and any provider without a brand icon; Jogando pill |
| folder | IconFolder | DRM-free store icon, add DRM-free in palette |
| folder-open | IconFolderOpen | open install folder |
| folder-search | IconFolderSearch | watched folders in status bar |
| bookmark | IconBookmark | collections nav, Alterar status, save-view hint |
| bookmark-plus | IconBookmarkPlus | Salvar como visão, Adicionar à coleção, Salvar em coleção |
| compass | IconCompass | Explorar no GameNode, Buscar no GameNode |
| radar | IconRadar | Jogos descobertos, discovery banner |
| package | IconPackage | Ports para PC |
| settings | IconSettings | Configurações |
| keyboard | IconKeyboard | Atalhos de teclado |
| info-circle | IconInfoCircle | Sobre e licenças, family info box |
| logout | IconLogout | Sair da conta |
| power | IconPower | Encerrar GameNode |
| alert-triangle | IconAlertTriangle | store warnings, unmatched card, warning banner |
| check | IconCheck | checked menu item, checkbox, done step |
| circle-check | IconCircleCheck | status bar store ok, success notification |
| circle-x | IconCircleX | error notification |
| cloud-check / cloud-off | IconCloudCheck / IconCloudOff | top bar sync status / unreachable store banner |
| wifi-off | IconWifiOff | offline banner |
| link | IconLink | Associado pill, Associar…, Alterar correspondência |
| tool | IconTool | Ferramentas do jogo |
| shield-check | IconShieldCheck | save protection |
| trophy | IconTrophy | Ver conquistas |
| eye / eye-off | IconEye / IconEyeOff | Ocultar/Mostrar na biblioteca, Mostrar ocultos, hidden flag |
| trash | IconTrash | Remover da biblioteca… |
| dots | IconDots | detail "more" button |
| loader-2 | IconLoader2 | scanning (`animate-spin`) |
| arrow-back-up | IconArrowBackUp | Limpar filtros / Limpar tudo |
| pencil | IconPencil | Editar… (game settings) |
| external-link | IconExternalLink | links that open the browser (Editar/Ver no GameNode) |
| world-search | IconWorldSearch | Explorar no GameNode nav, catalog idle state, Buscar no GameNode |
| mood-empty | IconMoodEmpty | catalog no results |
| calendar / clock | IconCalendar / IconClock | catalog result meta (release, playtime) |

## Store icons
`providerIcon(providerId)` in `ui/src/features/library/providerIcon.ts`: `steam` → IconBrandSteam,
`manual` (the DRM-free provider id) → IconFolder, anything else → IconDeviceGamepad2.
