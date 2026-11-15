# Shared components (Figma `5357:677`)

New shared primitives live in `ui/src/components/ui/` (one file each, exported from
`ui/src/components/index.ts`). Each has a Vitest test for its variants and accessibility.
All sizes are px. "row"/"col" = flex. Colors use the Tailwind keys from `tokens.md`.

## Button — `Button.tsx`
Props: `variant: 'primary'|'secondary'|'ghost'|'danger'`, `size: 'md'|'sm'`, `leftIcon?: TablerIcon`,
`fullWidth?`, native button props. Renders `<button type="button">`.

| | MD | SM |
|---|---|---|
| height | 36 | 32 |
| padding-x | 16 | 12 |
| gap / radius / icon | 6 / 4 (`rounded-sm`) / 16 | same |
| label size | 14 | 14 |

| Variant | Background | Border | Label | Icon |
|---|---|---|---|---|
| primary | `brand` (hover `#F3623B` = `--settings-accent-hover`, active `tertiary`) | none | 600 `text-primary` | `text-primary` |
| secondary | `raised` (hover `surface`) | 1px `border-subtle` | 500 `text-default` | `text-default` |
| ghost | transparent (hover `raised`) | none | 500 `text-secondary` (hover `text-default`) | `text-secondary` |
| danger | `danger` (hover 90% opacity) | none | 500 `text-primary` | `text-primary` |

Disabled: opacity .5, `cursor-not-allowed`, keep layout. Focus: 2px `brand` ring offset 2 (`focus-visible:`).
Loading (optional `loading` prop): `IconLoader2` spinning replaces the left icon.

## IconButton — `IconButton.tsx`
Square button with a Tabler icon and a mandatory `label` (aria-label + tooltip) and optional `shortcut`
(`string[]` of keys shown in the Tooltip via Kbd). Sizes: `md` 36 (detail actions), `toolbar` 34,
`sm` 28 (hero close). Variants:
- `outlined`: bg `raised`, 1px `border-subtle`, radius 6, icon 16 `text-secondary` (detail panel actions).
- `toggle`: no bg when off (icon `text-secondary`), bg `raised` + icon `text-primary` when `pressed` (`aria-pressed`). (Detail-pane toggle in toolbar.)
- `overlay`: bg `black/45`, radius 6, icon `text-primary` (hero close).

## Tooltip — `Tooltip.tsx` (wraps Mantine `Tooltip`)
row gap 8, padding 5/8, radius 6, bg `tooltip`, label 12/400 `text-primary`, followed by `Kbd`s.
Open delay 400ms; also on keyboard focus. Every IconButton uses it.

## Kbd — `Kbd.tsx`
height 18, padding-x 5, bg `raised`, 1px `border-subtle`, radius 4, text 11/500 `text-secondary`, min-width 18,
centered. A key combo is a row of Kbds with gap 4 (`<KbdCombo keys={['Ctrl','K']} />`). Platform: show
"Ctrl" on Windows/Linux (this app is Windows-first; do not localize key names except "Clique"→"Click",
"Setas"→"Arrows").

## Pill — `Pill.tsx`
row, padding 2/8, `rounded-full`, gap 4, optional 12px icon, text 12/500. Height 18.

| Tone | bg | text/icon |
|---|---|---|
| neutral | `raised` | `text-default` |
| brand | `accent-subtle` | `brand` |
| success | `success-subtle` | `success` |
| warning | `warning-subtle` | `warning` |
| danger | `danger-subtle` | `danger` |

As a button (e.g. collection "+" pill), add hover bg `surface` and focus ring.

## Switch — Mantine `Switch` restyled (`ui/src/components/ui/switchClasses.ts`)
Track 34×20, radius full. On: track `brand`, thumb white 16. Off: track `#2E2E2E` (`--settings-toggle-track-off`), thumb `text-muted`.
(Matches the existing settings toggle tokens; reuse them.)

## Checkbox — Mantine `Checkbox` restyled
16×16, radius 4. Checked: bg `brand`, white `IconCheck` 12. Unchecked: bg `canvas`, 1px `border-strong`.
Indeterminate: bg `brand`, white 8×2 bar (or `IconMinus` 12). Focus ring `brand`.

## SegmentedControl — Mantine `SegmentedControl` restyled
Track: padding 3, gap 2, bg `canvas`, 1px `border-subtle`, radius 8. Segment: height 26 (filters) / 28×26 icon-only
(view toggle), radius 5, label 12/400 `text-secondary`. Active: bg `raised`, label 12/600 `text-primary`
(icon `text-primary`; inactive icons `text-dimmed`). Segments are equal width when `fullWidth`.

## Chip (toggle chip) — `ChoiceChip.tsx`
height 26, padding-x 10, radius full, bg `surface`, 1px `border-subtle`, text 12/400 `text-default`.
Selected (`aria-pressed=true`): bg `accent-subtle`, 1px `brand`, text 12/600 `text-primary`.
Used for single-choice facets (Última sessão, Lançamento, Adicionado): clicking the selected chip clears it.

## FilterChip (active filter chip) — `FilterChip.tsx`
height 28, padding 0/6/0/10, gap 6, radius full, bg `accent-subtle`, 1px `brand`.
`label:` 12/400 `text-secondary`, value 12/600 `text-primary`, remove button `IconX` 14 `text-secondary`
(aria-label "Remover filtro {label}").

## Nav item — `NavItem.tsx`
height 32, padding-x 10, gap 10, radius 6, icon 16, label 14 (truncate), count 12 right-aligned.
- Default: label 400 `text-default`, icon `text-secondary`, count `text-dimmed`.
- Hover: bg `raised`.
- Active (`aria-current="page"`): bg `accent-subtle`, label 600 `text-primary`, icon `brand`, count `text-default`.
- Optional `warning` slot: `IconAlertTriangle` 14 `warning` placed just before the count (with an sr-only reason).
- Optional `countTone: 'brand'` renders the count in `brand` (used for "Jogos descobertos").
- Collapsed sidebar (72px): icon only, label in Tooltip, count as a small badge.

## SectionLabel — `SectionLabel.tsx`
Uppercase 11/600 letter-spacing 0.8 `text-muted`, padding-x 10 in the sidebar (0 in panels). Optional
trailing action (sidebar "+" `IconPlus` 14 `text-dimmed`; panel link text 12/500 `brand`).

## StatCard — `StatCard.tsx`
col gap 4, padding 10, bg `body`, 1px `border-subtle`, radius 6, flex 1.
Label 10/600 ls .6 uppercase `text-muted`; value 15/600 `text-primary`; hint 11/400 `text-dimmed` or a 4px
progress bar (track `raised` r2, fill `brand` r2, width = ratio). Three in a row with gap 8.

## KeyValueRow — `KeyValueRow.tsx`
row gap 8, padding 6/0, align center, bottom border 1px `border-subtle` (none on the last row).
Key 13/400 `text-secondary`; spacer; value 13/500 `text-default` (or a Pill, or "Nenhuma" 13/400 `text-dimmed`);
optional trailing `IconChevronRight` 14 `text-dimmed` when the row is a button (whole row clickable, hover bg `raised`).

## Menu surface — `menuClasses.ts` for Mantine `Menu`
Dropdown: padding 6, gap 1, bg `secondary`, 1px `border-overlay`, radius 8, `shadow-toast`.
Label: padding 6/10/4/10, 10/600 ls .8 uppercase `text-muted`.
Item: height 32, padding-x 10, gap 10, radius 6, left section 16 (icon `text-secondary` or an empty 16px box to align),
label 13/400 `text-default`, right section Kbd(s) or `IconChevronRight` 14 `text-dimmed` for submenus.
Hovered/keyboard-focused item: bg `raised`, label + icon `text-primary`. Checked: left `IconCheck` 16 `brand`, label 500.
Divider: margin 4/0, 1px `border-subtle`. Danger item: label + icon `danger`.

## Game tile — `GameTile.tsx` (grid)
Outer: width 172, padding 6, col gap 8, radius 8. Default bg transparent.
- Cover: 160×214, radius 6, bg `surface`, `<CoverImage>` (`object-cover`, lazy, async). Placeholder: centered
  title (existing behaviour).
- Cover shade (bottom 50%): gradient transparent → `black/55`.
- Title overlay inside the cover only when there is **no** cover image (placeholder): 18/700 white,
  line-height 1.05, left/right 10, bottom 12, max 3 lines. With a real cover, no overlay title.
- Top-left badge stack (top 6, left 6, gap 4): Installed badge — height 20, padding-x 6, radius 4, bg `black/60`,
  `IconDeviceDesktop` 12 `success` + "Instalado" 11/500 white. Running badge replaces it — bg `success`,
  `IconPlayerPlayFilled` 12 + "Em execução" 11/600 `canvas`. Family-shared (not installed): `IconUsersGroup` 12
  white + "Família". Hidden (when shown): `IconEyeOff` 12 + "Oculto".
- Top-right favorite badge (top 6, right 6): 22 circle, bg `black/60`, `IconStarFilled` 12 `warning`. Only when favorite.
- Info: name 14/500 lh 18 `text-default` (white when selected), 1 line ellipsis. Meta row (gap 6, 12/400
  `text-dimmed`): 6px match dot (success matched / warning ambiguous / `text-dimmed` unmatched / `text-muted`
  noGame, with sr-only label), 12px store icon, store name, spacer, playtime short ("42 h", "—" unknown).
- Hover: bg `raised`; cover gets `black/40` overlay and a centered 48 circle bg `brand` with
  `IconPlayerPlayFilled` 20 white (click = Play; only when Play is available; `IconDownload` when Install is the
  primary action).
- Selected (`aria-selected`): bg `accent-subtle`, 1.5px `brand` border (use `ring-[1.5px] ring-brand ring-inset`).
- Focused (keyboard): 2px `brand` focus-visible outline.
- Hidden entry (show-hidden on): whole tile `opacity-50`.
- Tile height varies (one-line names) — virtualizer rows use a fixed row height of 270 + 16 gap; titles are
  single-line so all tiles are 270 tall.

## Game row — `GameRow.tsx` (list)
height 52, padding-x 12, gap 12, radius 6, align center. Columns (fixed widths, title fills):
1. checkbox 16 (Checkbox; checked when selected; clicking toggles selection like Ctrl+click)
2. cover 30×40 radius 3
3. title (fill): name 14/500 `text-default` ellipsis; flags row 12/400 `text-dimmed` with 12 icon:
   `IconDeviceDesktop success` "Instalado" · `IconDownload text-secondary` "Não instalado" ·
   `IconUsersGroup` "Compartilhado pela família" · `IconPlayerPlayFilled success` "Em execução · 0:42:10" (live) ·
   `IconEyeOff` "Oculto"
4. store 130: 14 store icon `text-secondary` + name 13 `text-default`
5. GameNode status 120: Pill with the tone map in `copy.md` (Jogando → brand, Zerado → success, Na lista → neutral,
   Em andamento → neutral, Abandonado → danger); "—" 13 `text-dimmed` when not in GameNode or unmatched; an empty
   cell while the account library has never been read.
6. match 130: 6px dot + label 13 (`Associado`, `Revisar`, `Sem correspondência`, `Não é jogo`)
7. playtime 90: 13 `text-default` long format ("42 h 18 min"), "—" unknown
8. last session 110: 13 `text-default` relative ("Agora", "Ontem", "3 dias atrás", "Semana passada", "Mar. 2026", "2025")
9. favorite 28: `IconStarFilled` 14 `warning` when favorite, `IconStar` 14 `text-muted` otherwise; button toggles favorite
   (disabled with tooltip when unmatched).
- Hover: bg `raised`. Selected: bg `accent-subtle`, 1px `brand` border. Hidden: `opacity-50`.
- Header row (sticky, height 34, border-bottom `border-subtle`): select-all Checkbox (checked/indeterminate/
  unchecked), then column labels 11/600 ls .6 uppercase `text-muted`. Sortable headers are buttons; the sorted one
  is `text-primary` with `IconSortDescending`/`IconSortAscending` 12 `brand`. Clicking a header sorts by it, and
  clicking the sorted header flips the direction. Mapping: Título→`title`, Loja→`provider`, No GameNode→`gamenodeStatus`
  (new SortKey), Correspondência→`matchStatus`, Tempo→`playtime`, Última sessão→`lastPlayed`. Cover, checkbox and
  favorite columns are not sortable.

## Group header — `GroupHeader.tsx` (grid and list)
height 36, padding-x 4, gap 8, align center, full width, `<button aria-expanded>`.
`IconChevronDown`/`IconChevronRight` 14 `text-dimmed`, group icon 16 `text-secondary`, name 13/600 `text-primary`,
count "164 jogos" 12/400 `text-dimmed`, then a 1px `border-subtle` rule filling the remaining width.

## Banner — `Banner.tsx`
row gap 12, padding 12, radius 8, align center, 1px border. Icon 18. Text col gap 2: title 13/600 `text-primary`,
body 12/400 `text-secondary`. Optional action Button SM. Dismiss `IconX` 14 `text-dimmed` (aria-label "Dispensar").
Tones: warning (`warning-subtle`/`warning`), danger (`danger-subtle`/`danger`), neutral (`surface`/`text-muted`),
brand (`accent-subtle`/`brand`). Role `status` (neutral/brand) or `alert` (warning/danger).

## Notification — `ToastContainer.tsx` re-implemented with Mantine `Notification`
Stack bottom-right, 16 from edges, gap 10, width 380, newest at bottom. Card: padding 12, col gap 8, bg `secondary`,
1px `border-overlay`, radius 8, `shadow-notification`. Head row gap 10: 3×36 accent bar radius 2 (success `success`,
info/progress `brand`, danger `danger`), icon 18 (`IconCircleCheck` success / `IconDownload` or `IconInfoCircle` brand /
`IconCircleX` danger), text col (title 13/600 `text-primary`, body 12/400 `text-secondary`), close `IconX` 14.
Actions row: padding-left 13, gap 12, link buttons 12/600 `brand`. Progress: 4px track `raised`, fill `brand`.
Success/info auto-close after 6s (paused on hover/focus); danger persists until dismissed; progress persists until done.
Region `aria-label="Notifications"` (keep the existing test contract).
