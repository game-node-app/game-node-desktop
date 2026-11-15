# Tokens

## Figma variables → existing code tokens

| Figma variable | Hex | CSS variable (theme.css) | Tailwind |
|---|---|---|---|
| `primary` | #F15025 | `--color-primary` | `brand` (`bg-brand`, `text-brand`, `border-brand`) |
| `accent/subtle` | #F150251F | `--color-accent-subtle` | `accent-subtle` |
| `bg/sidebar` | #111111 | `--color-chrome` | `chrome` |
| `bg/raised` | #222222 | `--color-raised` | `raised` |
| `surface` | #262525 | `--color-surface` | `surface` |
| `background` | #161616 | `--color-background` | `body` |
| `secondary` | #1A1A1A | `--color-secondary` | `secondary` |
| `border/subtle` | #2A2A2A | `--color-border-subtle` | `border-subtle` |
| `#202020` (top bar bottom border) | #202020 | `--color-chrome-border` | `chrome-border` |
| `text-default` | #D9D9D9 | `--color-text-default` | `text-default` |
| `text/secondary` | #ADADAD | `--color-text-secondary` | `text-secondary` |
| `text-dimmed` | #808080 | `--color-text-dimmed` | `text-dimmed` |
| `success` / `success-subtle` | #3DD68C / #3DD68C1F | `--color-success*` | `success`, `success-subtle` |
| `warning` / `warning-subtle` | #F5A524 / #F5A5241F | `--color-warning*` | `warning`, `warning-subtle` |
| `danger` / `danger-subtle` | #E5484D / #E5484D1F | `--color-danger*` | `danger`, `danger-subtle` |
| `tertiary` (avatar gradient end) | #872C13 | `--color-tertiary` | `tertiary` |

## New tokens (add to `ui/src/styles/theme.css` `:root` and `ui/tailwind.config.ts`)

| Purpose | CSS variable | Value | Tailwind key |
|---|---|---|---|
| Figma `text/primary` (titles, selected labels) | `--color-text-primary` | #FFFFFF | colors `text-primary` → `text-text-primary` |
| Figma `text/muted` (section labels, table headers) | `--color-text-muted` | #7A7A7A | colors `text-muted` → `text-text-muted` |
| Figma `bg/canvas` (segmented track, checkbox off) | `--color-canvas` | #0B0B0B | colors `canvas` |
| Checkbox unchecked border | `--color-border-strong` | #4A4A4A | colors `border-strong` |
| Menu / popover / palette border | `--color-border-overlay` | #333333 | colors `border-overlay` |
| Floating batch bar border | `--color-border-floating` | #3A3A3A | colors `border-floating` |
| Tooltip background | `--color-tooltip` | #2E2E2E | colors `tooltip` |
| Radius 3 (thumb covers, checkbox mixed) | `--radius-thumb` | 3px | borderRadius `thumb` |
| Radius 5 (segment inside segmented control) | `--radius-segment` | 5px | borderRadius `segment` |
| Radius 6 (controls, inputs, nav items, icon buttons, cards) | `--radius-control` | 6px | borderRadius `control` |
| Radius 10 (filter popover, dialogs, banners-floating bar) | `--radius-popover` | 10px | borderRadius `popover` |
| Radius 12 (command palette) | `--radius-palette` | 12px | borderRadius `palette` |
| Font 9 (badges) | `--font-size-badge` | 9px | fontSize `badge` |
| Font 10 (overline: stat labels, menu section labels) | `--font-size-overline` | 10px | fontSize `overline` |
| Font 11 (caption: group headers, kbd, status bar) | `--font-size-caption` | 11px | fontSize `caption` |
| Font 13 (body: toolbar, menus, rows) | `--font-size-body` | 13px | fontSize `body` |
| Font 15 (title: popover/dialog titles, stat values) | `--font-size-title` | 15px | fontSize `title` |
| Font 22 (heading: main header title) | `--font-size-heading` | 22px | fontSize `heading` |
| Letter spacing 0.8px (uppercase section labels, DESKTOP badge) | — | 0.8px | letterSpacing `label` |
| Letter spacing 0.6px (stat labels, table headers) | — | 0.6px | letterSpacing `overline` |
| Filter popover shadow | `--shadow-popover` | 0 16px 40px rgba(0,0,0,.55) | boxShadow `popover` |
| Notification shadow | `--shadow-notification` | 0 10px 28px rgba(0,0,0,.45) | boxShadow `notification` |
| Menu / palette / batch bar shadow | existing `--shadow-toast` | 0 12px 32px rgba(0,0,0,.5) | `shadow-toast` (alias `shadow-menu` may be added) |
| Status bar height | `--size-statusbar-height` | 26px | height `statusbar` |
| Window control cell | `--size-window-control` | 46px | width `window-control` |

## Changed tokens

| Token | Old | New | Why |
|---|---|---|---|
| `--size-topbar-height` | 54px | **48px** | Figma top bar is 48px. Used by TopBar and any `h-topbar`/`top-topbar`. |
| `DEFAULT_SIDEBAR_WIDTH` (Rust `crates/core/src/library/layout.rs`) | 248 | **240** | Figma sidebar 240. Saved user widths are kept. |
| `DEFAULT_DETAIL_WIDTH` (Rust) | 340 | **360** | Figma detail panel 360. Saved user widths are kept. |

`--sidebar-width` (216px) belongs to the Settings modal and is **not** the library sidebar. Do not change it.

## Literal colors that stay literal (use Tailwind opacity modifiers)

| Use | Figma | Tailwind |
|---|---|---|
| Tile cover bottom shade | transparent → rgba(0,0,0,.55) | `bg-gradient-to-b from-transparent to-black/55` |
| Tile badges (favorite, installed) | rgba(0,0,0,.6) | `bg-black/60` |
| Tile hover overlay | rgba(0,0,0,.4) | `bg-black/40` |
| Hero close button | #00000073 | `bg-black/45` |
| Hero bottom fade | #1A1A1A00 → #1A1A1A | `bg-gradient-to-b from-transparent to-secondary` |
| Running badge text on success | #0B0B0B | `text-canvas` |
| Avatar placeholder | #F15025 → #872C13 | `bg-gradient-to-br from-brand to-tertiary` |
| Filter popover warning note (design only) | #2A2410 | not shipped |
