# Icons

All icons come from `@tabler/icons-react` (outline, stroke 2). Every name below exists in the installed
package. Size and color are `size:color-token`. Decorative icons get `aria-hidden="true"`; icon-only
buttons get an `aria-label`.

| Figma name | Component | Where (size:color) |
|---|---|---|
| `dots-vertical` | `IconDotsVertical` | Title bar drag affordance (16:text-muted) |
| `settings` | `IconSettings` | Title bar (16:text-secondary); nav "Geral" (16) |
| `x` | `IconX` | Close button (16:text-secondary); "Parar de monitorar" (16) |
| `search` | `IconSearch` | Navigation search (14:text-dimmed) |
| `user` | `IconUser` | Nav "Conta e sincronização" |
| `device-gamepad-2` | `IconDeviceGamepad2` | Nav "Lojas"; generic store icon in store cards, detected stores and plugin cards (20 / 18) |
| `folder` | `IconFolder` | Nav "Jogos DRM-free"; watched folder rows (18:text-secondary); path field (14:text-dimmed); permission pill "Arquivos" (12) |
| `archive` | `IconArchive` | Nav "Backups" |
| `tool` | `IconTool` | Nav "Ferramentas" |
| `puzzle` | `IconPuzzle` | Nav "Plugins"; community plugin icon box (20) |
| `info-circle` | `IconInfoCircle` | Nav "Sobre"; unsaved bar (16:brand); community note (16:text-secondary) |
| `alert-triangle` | `IconAlertTriangle` | Nav attention on Lojas (14:warning); "Sessão expirada" pill (12); unavailable folder (18:warning); plugin warning strip (14:warning) |
| `brand-steam` | `IconBrandSteam` | Steam store card and plugin card (20:text-primary); detected Steam (18:text-secondary). Provider icon comes from the provider id map, not hard-coded per panel |
| `lock` | `IconLock` | Account storage line (12:text-dimmed); credential storage row (18:text-secondary); "Disponível" pill (12) |
| `circle-check` | `IconCircleCheck` | Success pills "Conectado", "Em N jogos", "Sessão ativa" (12); synced items (14:success) |
| `circle-x` | `IconCircleX` | Not-synced items (14:text-muted); "Parada" pill (12) |
| `external-link` | `IconExternalLink` | "Abrir perfil", "Entrar de novo" (16); footer links (12:brand) |
| `logout` | `IconLogout` | "Sair" |
| `refresh` | `IconRefresh` | "Sincronizar agora", "Verificar tudo agora", "Verificar" |
| `dots` | `IconDots` | Store card overflow menu (16:text-secondary) |
| `trash` | `IconTrash` | "Remover" (saved login), "Limpar cache" |
| `check` | `IconCheck` | "Detectada" pill (12); "Salvar alterações" |
| `plus` | `IconPlus` | "Adicionar pasta…"; manual add row (18:text-secondary) |
| `arrow-back-up` | `IconArrowBackUp` | "Redefinir descobertas ignoradas", "Reverter" |
| `file-text` | `IconFileText` | "Escolher executável…", "Mostrar log recente", "Mostrar avisos de licença"; license row (18:text-secondary) |
| `chevron-down` | `IconChevronDown` | Selects (14:text-dimmed) |
| `bolt` | `IconBolt` | OptiScaler icon box (20:brand on accent-subtle) |
| `package` | `IconPackage` | Cache version rows (16:text-secondary) |
| `shield-check` | `IconShieldCheck` | "Oficial" pill (12) |
| `users-group` | `IconUsersGroup` | "Comunidade" pill (12) |
| `copy` | `IconCopy` | Plugin id (12:text-dimmed); "Copiar" system info |
| `square` | `IconSquare` | "Parar" plugin |
| `player-play` | `IconPlayerPlay` | "Iniciar" plugin |
| `world` | `IconWorld` | Permission pill "Rede" |
| `cpu` | `IconCpu` | Permission pill "Processos"; system info row (18:text-secondary) |
| `database` | `IconDatabase` | Permission pill "Registro" |
| `terminal-2` | `IconTerminal2` | Recent log row (18:text-secondary) |
| `folder-open` | `IconFolderOpen` | "Abrir pasta de logs" |
| `keyboard` | `IconKeyboard` | "Ver atalhos" |
| — (not in Figma) | `IconLoader2` | Busy states: "Verificando…" pills and buttons (`animate-spin motion-reduce:animate-none`) |
| `brand/gamenode-wordmark` | existing GameNode logo asset used by the current About panel / top bar | About version card (83×32) |

Permission pill mapping. The pills show the plugin's **native access** declarations from its trusted
descriptor (`plugins/*.toml` `native_access`, surfaced as `PluginInfo.nativeAccess`): `network` → Rede
(`IconWorld`), `files` → Arquivos (`IconFolder`), `process` → Processos (`IconCpu`), `registry` → Registro
(`IconDatabase`), `dbus` → D-Bus (`IconPlugConnected`). Unknown values render as a neutral pill with the raw
value and no icon. API grants (`capabilities`) stay in the "Detalhes" section, separate from native access.
