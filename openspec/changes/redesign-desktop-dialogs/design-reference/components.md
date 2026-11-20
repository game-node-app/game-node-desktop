# Components

Reuse first. Existing primitives in `ui/src/components/ui/` already match the specs they were built for:
`Button` (primary / secondary / ghost / danger; `sm` 32px, `md` 36px; `leftIcon`, `loading`), `Pill`
(neutral / brand / success / warning / danger, 18px, optional icon), `Checkbox` (16px, canvas fill,
`border-strong`, checked = brand, mixed), `Switch` (34×20), `SegmentedControl` (canvas track, 26px segments),
`SectionLabel` (11/600 upper `tracking-label` `text-text-muted`, optional action), `IconButton`, `Banner`,
`Skeleton`, `GameCover`. The pieces below are **new shared components** (put them in
`ui/src/components/ui/` or `ui/src/components/dialog/`, export from the barrel, one Vitest file each).

## 1. Work-surface shell (FloatingModal dialogs D01–D08)

Built from `FloatingModal` subcomponents; add a thin `DialogShell` (or extend the subcomponents' default
classes) so each dialog does not repeat these classes.

| Part | Spec | Classes |
|---|---|---|
| Content | bg secondary, 1px `#333`, radius 12, shadow `0 20 48 .55`, clip, flex column | `flex flex-col overflow-hidden rounded-dialog border border-border-overlay bg-secondary shadow-dialog` |
| Header | row, gap 12, padding 16/20/14/20, items center, bottom border subtle; drag handle | `flex items-center gap-sm px-lg pt-md pb-3.5 border-b border-border-subtle` |
| Header drag icon | `IconDotsVertical` 16, text-muted, decorative | `text-text-muted`, `aria-hidden` |
| Header cover (AddDialog only) | 30×40 radius 4 | `GameCover` / `<img>` `h-10 w-[30px] rounded-sm object-cover` |
| Title block | column gap 2, fills | `flex min-w-0 flex-1 flex-col gap-0.5` |
| Title | 16/600 text-primary, single line ellipsis | `truncate text-md font-semibold text-text-primary` |
| Subtitle | 12/400 text-dimmed, ellipsis | `truncate text-xs text-text-dimmed` |
| Close | `IconX` 16 text-secondary in a 28×28 `IconButton`; hover `bg-raised` | `FloatingModal.CloseButton` restyled |
| Body | column gap 16, padding 20, scrolls | `flex min-h-0 flex-1 flex-col gap-md overflow-y-auto p-lg` |
| Footer | row gap 8, padding 14/20, bg surface, top border, items center; spacer pushes right actions | `flex items-center gap-compact px-lg py-3.5 bg-surface border-t border-border-subtle` |
| Footer note (left) | optional `Icon` 14 text-dimmed + 12/400 text-dimmed | `flex items-center gap-1.5 text-xs text-text-dimmed` |

Default sizes (`defaultSize`, `minSize`): D01 820×540 / 640×420 · D02 520×560 / 440×420 · D03 uses D02's
window · D04 640×600 / 480×400 · D05 580×700 / 480×480 · D06 600×680 / 500×480 · D07 640×660 / 520×480 ·
D08 720×460 / 600×320. Heights add ~10% to the Figma content height so the default shows the whole design;
bodies scroll when the window is smaller. Keep each dialog's existing `geometryKey` so saved geometry still applies.

## 2. Confirmation shell (AppModal dialogs D09–D13) — `ConfirmDialog`

| Part | Spec | Classes |
|---|---|---|
| Content | column gap 14, padding 24, bg secondary, 1px `#333`, radius 12, shadow dialog; width per dialog (460/400/520/480/420) | `flex flex-col gap-3.5 p-control-inline rounded-dialog border border-border-overlay bg-secondary shadow-dialog` |
| Tone circle | 40×40 full radius, tone-subtle bg, tone `Icon` 20 centered | `flex h-10 w-10 items-center justify-center rounded-full` + tone classes (`tokens.md`) |
| Title | 17/600 text-primary (dialog label) | `text-dialog-title font-semibold text-text-primary` |
| Body | 13/400 text-secondary, line height 20 (dialog description) | `text-body leading-body-relaxed text-text-secondary` |
| Content slot | option cards, checklist, side-by-side cards, status row | — |
| Actions | row gap 8, padding-top 6, end-aligned; an optional left Ghost action followed by a spacer | `flex items-center justify-end gap-compact pt-1.5` |

Props: `tone`, `icon`, `title`, `description`, `children`, `actions`, `initialFocus`. It renders through
`AppModal` (layer stack, focus trap, Escape, focus restore). Danger dialogs put initial focus on Cancel.

## 3. Field, TextInput, SelectField, TextArea

| Part | Spec | Classes |
|---|---|---|
| Field | column gap 6; label 12/500 text-secondary | `flex flex-col gap-1.5`; label `text-xs font-medium text-text-secondary` |
| Input box | 34px tall, padding 0/10, gap 8, bg surface, 1px subtle, radius 6; focus-within border primary | `flex h-[34px] items-center gap-compact px-xs rounded-control border border-border-subtle bg-surface focus-within:border-brand` |
| Leading icon | 14 text-dimmed | `text-text-dimmed shrink-0` |
| Value | 13/400 text-primary, ellipsis; placeholder text-dimmed | `min-w-0 flex-1 truncate bg-transparent text-body text-text-primary outline-none placeholder:text-text-dimmed` |
| Read-only | same box, no focus border, value not editable | `aria-readonly`, no hover |
| Select | same box + trailing `IconChevronDown` 14 text-dimmed; opens a Mantine `Menu`/`Combobox` styled with `menuClasses.ts` | — |
| TextArea | 64px tall, padding 8/10/0/10, 13/400 line height 19 | `h-16 resize-none rounded-control border border-border-subtle bg-surface px-xs pt-compact text-body leading-field text-text-primary focus:border-brand` |
| Field + button | row gap 8 aligned to the bottom; the button is `Button size="md" variant="secondary"` | `flex items-end gap-compact` |
| Error / helper | 12/400; helper text-dimmed, warning `text-warning` with `IconInfoCircle` 14 | `flex items-center gap-compact text-xs` |

The label is a real `<label>` bound to the control (`htmlFor` / `aria-labelledby`).

## 4. OptionCard (selectable card) and RadioCard

Used by: AddDialog status cards (D02), MatchDialog option rows (D01), RemoveLocalGame choices (D09).

| State | Background | Border | Title |
|---|---|---|---|
| Unselected | `bg-surface` (status, remove) or none (match rows) | `border-border-subtle` | 13/400 text-default (status) · 13/600 text-primary (match, remove) |
| Selected | `bg-accent-subtle` | `border-brand` | 13/600 text-primary |
| Hover (unselected) | `bg-raised` | `border-border-subtle` | — |
| Focus-visible | — | 2px focus ring `ring-brand ring-offset-0` | — |

- **Status card** (D02): column gap 6, padding 10, radius 8, equal widths (`grid grid-cols-5 gap-compact` for
  five statuses); icon 18 (`text-brand` selected, `text-text-secondary` otherwise) above the label. It is a
  radio group (`role="radiogroup"`, arrow keys move selection).
- **Match option row** (D01): row gap 10, padding 8, radius 8, 66px tall; cover 36×48 radius 4; title
  13/600 text-primary; meta 11/400 text-dimmed `"<year> · <developer?> · <platforms>"` (developer omitted —
  see design.md); trailing confidence `Pill`. A listbox option (`role="option"`, `aria-selected`).
- **RadioCard** (D09): row gap 12, padding 12, radius 8; Radio 16 + column gap 3 (title 13/600 text-primary,
  description 12/400 text-dimmed line height 17).

**Radio** (16×16, full radius): unselected 1.5px `border-text-muted`; selected 5px `border-brand` (ring look,
transparent center). Build on Mantine `Radio` restyled through `classNames` so keyboard arrows work.
Also used by D06 candidate rows: row 32px, gap 10, padding 0/10, radius 6, selected row `bg-raised`, label
12/400 (`text-text-primary` selected, `text-text-default` otherwise).

## 5. ProgressBar

Track 6px tall, `bg-raised`, radius 3; fill `bg-brand` radius 3, width = runtime percent (inline style
allowed). `role="progressbar"` with `aria-valuenow/min/max` and an accessible label. Used by D03 (with
`"2 de 3"` 12/600 text-primary to the right, gap 10) and D04 (inside the summary card).

## 6. InsetCard / SummaryCard / ToolCard

Row gap 12–14, padding 12, `bg-body`, 1px subtle, radius 8, items center. Leading icon (D04 `IconTrophy` 24
`text-warning`; D05 `IconShieldCheck` 20 `text-success`, or `IconShieldOff`/`text-text-muted` when off) or an
icon box (D06: 40×40 `bg-accent-subtle` radius 8 with `IconBolt` 20 `text-brand`). Title 14/600
text-primary, description 12/400 text-dimmed, optional trailing control (`Switch`, `Pill`).

## 7. Callout (danger)

D06 anti-cheat: column gap 8, padding 12, `bg-danger-subtle`, 1px `border-danger`, radius 8. Header row gap 8:
`IconAlertTriangle` 16 `text-danger` + 13/600 text-primary. Body 12/400 text-secondary line height 17.
Confirmation row gap 10: `Checkbox` + 12/400 text-primary label. `role="alert"` is **not** used (it would
re-announce on every snapshot); use a labelled `region`.

## 8. Checklist

D12 and D01 reasons. Rows gap 8, items center. Positive: `IconCircleCheck` 14 `text-success` + 12/400
text-default. Negative / stays local: `IconCircleX` 14 `text-text-muted` + 12/400 text-dimmed. Warning reason:
`IconAlertTriangle` 14 `text-warning` + 12/400 text-default. Inside an inset list (D12: column gap 8, padding 12,
`bg-body`, border subtle, radius 8). D10 variant: bullet `6×6` full-radius `bg-brand` + 12/400 text-default,
column gap 4, padding 10.

## 9. List rows

| Surface | Row | Content |
|---|---|---|
| D02 collections | 34px, row gap 10, padding 0/8, radius 6, hover `bg-raised`; list column gap 2, padding 4, `bg-body`, border subtle, radius 8, scrolls after ~6 rows | `Checkbox`, name 13/400 (checked text-primary, else text-default) fills, count 12/400 text-dimmed |
| D03 batch items | row gap 10, padding 8/0, bottom border subtle | cover 24×32 radius 3, title 13/500 text-primary, status 11/400 (tone color), trailing icon 16 |
| D04 achievements | 65px (virtualized, fixed), row gap 12, padding 10/0, bottom border subtle | icon 44×44 radius 6; name 13/600; description 12/400 text-dimmed (2 lines max, clamp); right column end-aligned gap 2: date 12/400 text-default, meta 11/400 text-dimmed |
| D05 history | row gap 12, padding 10/12, bottom border subtle (not on the last), list border subtle radius 8 clip | icon 16, time 13/500 text-primary, reason 11/400 text-dimmed, size 12/400 text-dimmed, `Button sm secondary` Restore |

## 10. Discovery table (D08)

Header row: gap 12, padding 8/0, bottom border subtle; `Checkbox` (mixed when partial) and labels 11/600
text-muted uppercase `tracking-overline`. Columns: checkbox 16 · cover 30×40 radius 3 · game (fills, min 224;
title 13/600 text-primary, path 11/400 text-dimmed ellipsis) · executable 190 (compact select: 30px tall,
padding 0/8, gap 6, `bg-surface`, border subtle, radius 6, 12/400 text-default ellipsis, `IconChevronDown` 12) ·
GameNode 170 (`Pill`: success with `IconLink` "<title> (<year>)", warning `IconAlertTriangle` "Sem
correspondência", neutral `IconAlertTriangle` "Provavelmente não é jogo"). Rows gap 12, padding 10/0, bottom
border subtle. Use a semantic `<table>` or a `role="grid"`; the list is short (no virtualization needed below 100
rows; virtualize with `@tanstack/react-virtual` beyond that).

## 11. Sort select (D04)

32px, row gap 6, padding 0/10, `bg-raised`, border subtle, radius 6; `IconArrowsSort` 14 text-secondary,
label 12/400 text-default, `IconChevronDown` 12 text-dimmed. Opens a Mantine `Menu` with the sort options.

## 12. Sign-in (D14)

- Window split: art pane 760 fixed-ratio (≈53%; use `basis-[53%] max-w-[760px]` and let the sign-in pane
  fill), `bg-chrome`, padding 48, content at the bottom. Below 1100px window width the art pane shrinks;
  below 860px it is hidden.
- **Cover mural**: 5 columns of tiles 150×200 radius 8 opacity 0.55, column pitch 162, row pitch 212, odd
  columns start at y −60 and even at y 30, first column at x −30 (see `raw/10`). Tiles show cached cover
  files (`gamenode-asset://localhost/cover/<file>`, lazy, async decoding, `alt=""`, `aria-hidden` on the
  whole mural). Missing tiles use `bg-raised`. Two overlay layers: `bg-signin-side` then `bg-signin-bottom`.
  No animation.
- Headline 28/600 text-primary (`text-hero`), body 15/400 text-secondary line height 22, column gap 8, max 520.
- Sign-in column: 400 wide, column gap 20, centered. Wordmark 83×32 (existing `ui/src/assets/icons/gamenode-logo.svg`, as the top bar uses it). Heading 26/600
  (`text-display`) + lede 14/400 text-secondary, gap 6.
- **ProviderButton**: 400×44, row gap 12, padding 0/14, `bg-raised`, border subtle, radius 8, hover
  `bg-surface` + `border-border-strong`; badge 22×22 `bg-border-floating` radius 5 with the provider initial
  12/700 text-primary; label 14/500 text-primary fills; `IconExternalLink` 14 text-dimmed. Buttons column gap 10.
- Security note: `IconLock` 14 + 12/400 text-dimmed, gap 8.
- Version label: 11/400 text-muted, bottom 20 right 24 of the sign-in pane.
- Error and restore states keep their current behavior, rendered as a `Banner` above the provider buttons.
