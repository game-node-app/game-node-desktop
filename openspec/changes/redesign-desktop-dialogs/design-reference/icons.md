# Icons

All icons come from `@tabler/icons-react` (outline, stroke 2 unless noted). Size and color are given as
`size:color-token`. Decorative icons get `aria-hidden="true"`; icon-only buttons get an `aria-label`.

| Figma name | Component | Where (size:color) |
|---|---|---|
| `dots-vertical` | `IconDotsVertical` | Every FloatingModal header, drag affordance (16:text-muted) |
| `x` | `IconX` | Every FloatingModal close button (16:text-secondary) |
| `search` | `IconSearch` | D01 search field (14:text-dimmed) |
| `circle-check` | `IconCircleCheck` | D01 positive reason, D12 "sends" rows (14:success); D03 added item (16:success) |
| `alert-triangle` | `IconAlertTriangle` | D01 warning reason (14:warning); D03 skipped item (16:warning); D06 anti-cheat callout (16:danger); D08 warning/neutral pills; D10 tone circle (20:warning) |
| `circle-x` | `IconCircleX` | D01 "Não está no GameNode" button; D12 "stays on this PC" rows (14:text-muted) |
| `sparkles` | `IconSparkles` | D01 "Usar a correspondência automática" button |
| `link` | `IconLink` | D01 "Associar" button; D08 matched pill; D11 tone circle (20:warning) |
| `device-gamepad-2` | `IconDeviceGamepad2` | D02 status Jogando (18) |
| `trophy` | `IconTrophy` | D02 status Zerado (18); D04 summary (24:warning) |
| `bookmark` | `IconBookmark` | D02 status Na lista (18) |
| `archive` | `IconArchive` | D02 status Abandonado (18); D05 manual backup row (16:text-secondary) and "Fazer cópia agora" |
| — (not in Figma) | `IconInfinity` | D02 status Em andamento / Ongoing (18) — the fifth status Figma omits |
| `check` | `IconCheck` | Checkbox checked mark; D07 "Salvar" button |
| `plus` | `IconPlus` | D02 "Adicionar ao GameNode" and "+ Nova coleção"; D08 "Adicionar N jogos" |
| `minus` | `IconMinus` | Checkbox mixed mark (D08 header) |
| `loader-2` | `IconLoader2` | D03 item in progress, D13 waiting row (16:brand, `animate-spin`, `motion-reduce:animate-none`) |
| `arrows-sort` | `IconArrowsSort` | D04 sort select (14:text-secondary) |
| `chevron-down` | `IconChevronDown` | Selects (14 or 12:text-dimmed); D07 "Avançado" disclosure (14:text-dimmed; rotate −90° when collapsed) |
| `lock` | `IconLock` | D04 locked achievement without gray icon (18:text-muted); D14 security note (14:text-dimmed) |
| `clock` | `IconClock` | D04 footer "Atualizado…" (14:text-dimmed) |
| `external-link` | `IconExternalLink` | D04 "Abrir na Steam"; D13 tone circle (20:brand) and "Abrir de novo"; D14 provider buttons (14:text-dimmed) |
| `shield-check` | `IconShieldCheck` | D05 status card active (20:success); D05 recovery backup row (16:warning) |
| — (not in Figma) | `IconShieldOff` | D05 status card when protection is off (20:text-muted) |
| `folder` | `IconFolder` | D05 save path; D07 game folder / working directory (14:text-dimmed); D11 DRM-free card (12:text-dimmed) |
| `database` | `IconDatabase` | D05 used space (14:text-dimmed) |
| `history` | `IconHistory` | D05 automatic (after session) backup rows (16:text-secondary) |
| `arrow-back-up` | `IconArrowBackUp` | D05 "Restaurar" |
| `folder-open` | `IconFolderOpen` | D05 "Abrir pasta" |
| `bolt` | `IconBolt` | D06 tool icon box (20:brand) |
| `file-text` | `IconFileText` | D06 target exe, D07 executable (14:text-dimmed) |
| `download` | `IconDownload` | D06 footer note (14:text-dimmed) and "Instalar …" |
| `info-circle` | `IconInfoCircle` | D07 "Vários executáveis possíveis" (14:warning) |
| `terminal-2` | `IconTerminal2` | D07 arguments (14:text-dimmed) |
| `trash` | `IconTrash` | D07 "Remover da biblioteca…"; D09 tone circle (20:danger) and "Remover" |
| `brand-steam` | `IconBrandSteam` | D11 store card provider (12:text-dimmed). Other providers: `IconBrandEpicGames` / existing provider icon map |
| `chevron-right` | `IconChevronRight` | D11 between the two cards (16:text-muted) |
| `cloud-check` | `IconCloudCheck` | D12 tone circle (20:success) |
| `books` | `IconBooks` | D14 "Usar sem conta" — **out of scope**, do not use |
