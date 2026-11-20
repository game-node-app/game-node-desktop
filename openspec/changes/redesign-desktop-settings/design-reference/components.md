# Components

Reuse first. These primitives in `ui/src/components/ui/` already match the Settings frames and need no
visual change: `Button` (`sm` = Figma "Size=SM"), `Pill` (all five tones, optional 12px icon), `Kbd`,
`NavItem` (32px, icon 16, label 14, `count`, `warning`, active = `bg-accent-subtle` + brand icon),
`SelectField`, `IconButton`, `Tooltip`, `MenuSurface`/`menuClasses.ts`, `ProgressBar`, `Callout`.

Two primitives need a small adjustment that also matches the shared Figma component sets:

| Primitive | Change | Why |
|---|---|---|
| `Switch` | Track padding 3 and thumb 14 (`--switch-thumb-size:14px`, `--switch-track-label-padding:3px`) | Figma `Switch` (5357:774) is 34×20 with p3 and a 14px knob; today it is 16px/2px. Applies everywhere `Switch` is used (same component set). |
| `SegmentedControl` | Add a `segmentPadding` prop: `compact` (6px, default, library) · `regular` (12px, Settings rows) · `filter` (10px, Plugins filter) | Settings segments are `p0/12`, Plugins filter `p0/10`; the library keeps 6px. |

The pieces below are **new**. Put the settings-only ones in `ui/src/features/settings/components/` and export
them from that folder's `index.ts`; one Vitest file each. Classes assume the tokens in `tokens.md`.

## 1. Modal shell — `SettingsModal`

| Part | Spec | Classes |
|---|---|---|
| Content | 960×720 default, min 720×480; bg canvas; 1px #333; radius 12; shadow `0 24 64 .6`; clip; column | `FloatingModal.Content` + `flex flex-col overflow-hidden rounded-dialog border border-border-overlay bg-canvas shadow-alert` |
| Title bar (drag handle) | h48, row gap10, padding 0/10/0/14, items center, bg sidebar, bottom border subtle | `FloatingModal.Header` + `flex h-[48px] shrink-0 items-center gap-xs pl-[14px] pr-xs bg-chrome border-b border-border-subtle` |
| — drag icon | `IconDotsVertical` 16 text-muted, decorative | `text-text-muted` `aria-hidden` |
| — settings icon | `IconSettings` 16 text-secondary, decorative | `text-text-secondary` |
| — title | "Configurações" 14/600 text-primary; it is the dialog label | `FloatingModal.Title` `text-sm font-semibold text-text-primary` |
| — Esc hint | `Kbd` "Esc", decorative (`aria-hidden`) | pushed right with `ml-auto` |
| — close | 32×32 radius 6, `IconX` 16 text-secondary, hover `bg-raised` | `FloatingModal.CloseButton` / `IconButton` |
| Body | row, fills | `flex min-h-0 flex-1` |
| Navigation | 232 wide, column gap16, padding 12, bg sidebar, right border subtle; scrolls when short | `flex w-settings-nav shrink-0 flex-col gap-md p-sm bg-chrome border-r border-border-subtle overflow-y-auto` |
| Nav group | column gap2; label row padding 0/10/4/10, 10/600 upper tracking 0.8 text-muted | `flex flex-col gap-[2px]`; label `px-xs pb-[4px] text-overline font-semibold uppercase tracking-label text-text-muted` |
| Nav item | existing `NavItem`; Lojas gets `warning` (alert-triangle 14 warning); Plugins gets `count` | — |
| Nav footer | "GameNode Desktop {version}" 11/400 text-muted, pinned bottom | `mt-auto text-caption text-text-muted` |
| Content area | relative column, fills | `relative flex min-w-0 flex-1 flex-col` |
| Panel (scroller) | column gap20, padding 28/32/32/32 | `flex min-h-0 flex-1 flex-col gap-lg overflow-y-auto pt-[28px] px-xl pb-xl` (+ extra bottom padding = 56px while the unsaved bar shows) |
| Scroll fade | 56px, pinned to the bottom of the content area (above the unsaved bar), hidden when scrolled to the end, `pointer-events-none` | `pointer-events-none absolute inset-x-0 bottom-0 h-[56px] bg-settings-fade` |

Default sizes stay `defaultSize 960×720`, `minSize 720×480`, `geometryKey "settings"`, `zIndex 1000`.
Below ~820px wide the navigation keeps 232px and the panel shrinks; rows wrap their control under the text
(see SettingsRow).

## 2. `SettingsSearch` (navigation search field)

| Part | Spec | Classes |
|---|---|---|
| Box | h32, row gap8, padding 0/10, bg canvas, 1px subtle, radius 6; focus-within border brand | `flex h-[32px] items-center gap-compact px-xs rounded-control border border-border-subtle bg-canvas focus-within:border-brand` |
| Icon | `IconSearch` 14 text-dimmed | |
| Input | 12/400 text-primary; placeholder "Buscar configurações" text-dimmed; `type="search"`, `aria-label` | `min-w-0 flex-1 bg-transparent text-xs text-text-primary outline-none placeholder:text-text-dimmed` |

Behavior (design.md › Search): filters the navigation to topics whose title, description or row labels match
(accent- and case-insensitive); the first match is selected; matching row labels in the open panel get a brand
underline/highlight; Escape in the field clears it before closing the modal; empty result shows
"Nenhuma configuração encontrada" (12 text-dimmed) in the nav.

## 3. `SettingsPanel` (panel header)

| Part | Spec | Classes |
|---|---|---|
| Header | column gap4; with an action: row gap12, align end | `flex flex-col gap-[4px]` / `flex items-end gap-sm` |
| Title | 22/600 text-primary (`h2`, labelled region) | `text-heading font-semibold text-text-primary` |
| Description | 13/400 text-secondary | `text-body text-text-secondary` |
| Action slot | right-aligned `Button sm` or `SegmentedControl` | `ml-auto shrink-0` |

## 4. `SettingsCard`

| Part | Spec | Classes |
|---|---|---|
| Card | column, bg secondary, 1px subtle, radius 10, clip; tone border variant (warning/danger) | `flex flex-col overflow-hidden rounded-card border border-border-subtle bg-secondary` (+ `border-warning`) |
| Header (37) | padding 14/16/10/16; 11/600 upper tracking 0.8 text-muted (`h3`) | `px-md pt-row pb-xs text-caption font-semibold uppercase tracking-label text-text-muted` |
| Header with action (56) | row gap12 padding 12/16, items center; title fills; `Button sm` right | `flex items-center gap-sm px-md py-sm` |
| Children | rows; every row after the header has a 1px top border subtle | `[&>*+*]:border-t` pattern or per-row `border-t border-border-subtle` |
| Card gap | 20 between cards (panel gap) | — |

## 5. `SettingsRow`

| Part | Spec | Classes |
|---|---|---|
| Row | row gap16, padding 14/16, items center, top border subtle | `relative flex items-center gap-md px-md py-row border-t border-border-subtle` |
| Compact row (lists) | row gap14, padding 12/16 | `gap-row py-sm` |
| Leading icon (optional) | 18 text-secondary (warning tone when unavailable) | `shrink-0 text-text-secondary` |
| Text | column gap3, fills | `flex min-w-0 flex-1 flex-col gap-[3px]` |
| Label | 14/500 text-primary (13/500 for folder paths, ellipsis) | `text-sm font-medium text-text-primary` |
| Help | 12/400 text-dimmed, line height 17, max width 400 | `max-w-settings-copy text-xs leading-note text-text-dimmed` |
| Control | right, no shrink | `ml-auto flex shrink-0 items-center gap-xs` |
| Changed dot | 6×6 brand circle in the left padding (x=6 inside the 16px gutter), vertically centered, decorative; the row's accessible description gets "Alterado" | `absolute left-[6px] top-1/2 h-[6px] w-[6px] -translate-y-1/2 rounded-full bg-brand` |
| Previous value | while changed, the help line becomes "Antes: {saved value}" (12 text-dimmed, wraps) | — |
| Narrow (< 560px panel) | control wraps below the text | `flex-wrap` |

Props: `label`, `help`, `icon`, `control`, `changed`, `previous`, `htmlFor`/`labelId` (the switch/select is
labelled by the row label and described by the help).

## 6. `IconBox` and `StatTile`

| Part | Spec | Classes |
|---|---|---|
| IconBox | 40×40, radius 8, bg raised, 1px subtle, icon 20 text-primary; `accent` tone: bg accent-subtle, no border, icon brand | `flex h-[40px] w-[40px] shrink-0 items-center justify-center rounded-md border border-border-subtle bg-raised` |
| StatTile | column gap2 padding 10, bg canvas, 1px subtle, radius 6; value 16/600 text-primary; caption 11/400 text-dimmed; fills (3 per row, gap 8) | `flex min-w-0 flex-1 flex-col gap-[2px] p-xs rounded-control border border-border-subtle bg-canvas` |
| Stats row | row gap8, padding 0/16/14/16 | `flex gap-compact px-md pb-row` |

`StatTile` differs from the library `StatCard` (bg body, other type) — keep both.

## 7. `PathField` (Backups destination)

| Part | Spec | Classes |
|---|---|---|
| Box | 230×32, row gap6, padding 0/10, bg canvas, 1px subtle, radius 6; changed or focused → border brand | `flex h-[32px] w-[230px] items-center gap-[6px] px-xs rounded-control border bg-canvas` |
| Icon | `IconFolder` 14 text-dimmed | |
| Value | 12/400 text-primary, single line, ellipsis (full path in `title`) | `min-w-0 flex-1 truncate bg-transparent text-xs text-text-primary outline-none` |
| Button | `Button sm secondary` "Escolher pasta…" right of the box, gap 10 | |

The field is an editable text input (paths can be typed or pasted) and the button opens the native picker.

## 8. `SettingsSelect`

`SelectField size="compact"` restyled to Figma: h32, gap6, padding 0/10, bg canvas, 1px subtle, radius 6,
value 13/400 text-primary, `IconChevronDown` 14 text-dimmed; widths 140 (copies) / 180 (Abrir em, Idioma).
Dropdown uses `menuClasses.ts`.

## 9. `UnsavedChangesBar`

| Part | Spec | Classes |
|---|---|---|
| Bar | h56, row gap10, padding 0/20, items center, bg raised, top border #3A3A3A, pinned to the bottom of the content area, above the fade | `absolute inset-x-0 bottom-0 flex h-[56px] items-center gap-xs px-lg bg-raised border-t border-border-floating` |
| Icon | `IconInfoCircle` 16 brand | |
| Text | column gap1: "Alterações não salvas" 13/600 text-primary; "{n} configurações alteradas" 11/400 text-dimmed | |
| Actions | `Button sm ghost` "Reverter" (`IconArrowBackUp`) · `Button sm primary` "Salvar alterações" (`IconCheck`, `loading` while saving) | `ml-auto flex gap-compact` |

It is a `role="region"` with `aria-live="polite"` label; Ctrl+S / Cmd+S saves while it is visible.

## 10. List rows inside cards

| Row | Spec |
|---|---|
| Account identity (S1) | row gap14 p16: 48×48 avatar (image or `bg-avatar` circle), name 16/600, `IconLock` 12 + storage line 12 text-dimmed, `Pill success` "Conectado", `Button sm secondary` "Abrir perfil" (`IconExternalLink`), `Button sm ghost` "Sair" (`IconLogout`) |
| Synced items (S1) | row gap24 p14/16; two columns gap8; each item `IconCircleCheck` 14 success + 12 text-default, or `IconCircleX` 14 text-muted + 12 text-dimmed |
| Store header (S2) | row gap14 p16: IconBox, name 16/600 + status line 12 text-dimmed, state `Pill`, primary action `Button sm`, overflow 30×30 bordered `IconButton` (`IconDots`) opening a `Menu` |
| Detected store (S2) | compact row, icon 18 text-secondary, name 14/500, "{path} · {n} bibliotecas" 12 text-dimmed, `Pill neutral` "Detectada" (`IconCheck`) |
| Watched folder (S3) | compact row, `IconFolder` 18 (or `IconAlertTriangle` 18 warning), path 13/500 ellipsis, meta 12 text-dimmed, `Pill success` "Monitorando" / `Pill warning` "Indisponível" / `Pill neutral` "Verificando…", `Button sm ghost` "Verificar", `Button sm ghost` "Parar de monitorar" |
| Backup usage (S4) | row gap10: cover 18×24 radius 2 (`rounded-[2px]`, `<img loading="lazy" decoding="async">` over `gamenode-asset://`, neutral placeholder when absent), title 13 text-default ellipsis, "{size} · {n} cópias" 12 text-dimmed |
| Cache version (S5) | row gap10 p8/0 top border: `IconPackage` 16 text-secondary, "{tool} {version}" 13 text-default, optional `Pill success` "Em uso" / `Pill warning` "Nightly", "{size} · verificado SHA-256" 12 text-dimmed |
| Plugin card (S6) | header row gap14 p16 (IconBox, name 15/600 + version 12 text-dimmed + origin `Pill`, copyable id 12 text-dimmed with `IconCopy` 12), session `Pill`, `Button sm secondary` Parar/Iniciar; permissions row gap8 p12/16 ("Permissões" 12 text-secondary, neutral pills, "Detalhes" 12/500 brand link-button right); optional warning strip row gap8 p10/16 `bg-warning-subtle` |
| Note box (S6) | row gap10 p12, bg surface, 1px subtle, radius 8, `IconInfoCircle` 16 text-secondary, 12 text-secondary leading-note |
| Footer links (S7) | row gap20 padding 0/4: link-buttons 12/500 brand + `IconExternalLink` 12 brand; open in the system browser |
