# Tokens

Every color in the Settings section already exists in code (library and dialog redesigns). This change
adds **one radius alias, one spacing step, one width, one max-width and two gradients**, and retires the
`--settings-*` legacy variables that nothing references after the migration. Nothing else is new.

The app root font size is **14px**, so Tailwind's default rem scale does not match Figma pixels
(`p-4` = 14px, not 16px). Use the token keys below or arbitrary px values (`pt-[28px]`); never the rem scale
for Figma sizes.

## Figma variables → existing code tokens

| Figma variable | Hex | CSS variable (`ui/src/styles/theme.css`) | Tailwind |
|---|---|---|---|
| `primary` | #F15025 | `--color-primary` | `brand` (`bg-brand`, `text-brand`, `border-brand`) |
| `accent/subtle` | #F150251F | `--color-accent-subtle` | `accent-subtle` |
| `text/primary` | #FFFFFF | `--color-text-primary` | `text-text-primary` |
| `text-default` | #D9D9D9 | `--color-text-default` | `text-text-default` |
| `text/secondary` | #ADADAD | `--color-text-secondary` | `text-text-secondary` |
| `text-dimmed` | #808080 | `--color-text-dimmed` | `text-text-dimmed` |
| `text/muted` | #7A7A7A | `--color-text-muted` | `text-text-muted` |
| `bg/canvas` (modal body, content area, stat tiles, fields, segmented track) | #0B0B0B | `--color-canvas` | `bg-canvas` |
| `bg/sidebar` (title bar, navigation) | #111111 | `--color-chrome` | `bg-chrome` |
| `secondary` (cards) | #1A1A1A | `--color-secondary` | `bg-secondary` |
| `bg/raised` (icon boxes, active segment, Kbd, neutral pill, unsaved bar, usage track) | #222222 | `--color-raised` | `bg-raised` |
| `surface` (community note box) | #262525 | `--color-surface` | `bg-surface` |
| `background` | #161616 | `--color-background` | `bg-body` |
| `border/subtle` | #2A2A2A | `--color-border-subtle` | `border-border-subtle` |
| `#333333` literal (modal outline) | #333333 | `--color-border-overlay` | `border-border-overlay` |
| `#3A3A3A` literal (unsaved bar top border) | #3A3A3A | `--color-border-floating` | `border-border-floating` |
| `#2E2E2E` literal (switch off track) | #2E2E2E | `--settings-toggle-track-off` (kept; `Switch` uses it) | — |
| `success` / `success-subtle` | #3DD68C / #3DD68C1F | `--color-success*` | `success`, `success-subtle` |
| `warning` / `warning-subtle` | #F5A524 / #F5A5241F | `--color-warning*` | `warning`, `warning-subtle` |
| `danger` / `danger-subtle` | #E5484D / #E5484D1F | `--color-danger*` | `danger`, `danger-subtle` |
| `#0000009E` overlay | — | `--settings-overlay` | `FloatingModal.Overlay backgroundOpacity={0.62}` (existing) |
| `#872C13` (avatar gradient end) | #872C13 | — | `tertiary` |

## Existing size tokens used here

| Value | Use in these frames | Tailwind |
|---|---|---|
| radius 3 | usage track, `NOVO` badge (annotation, not shipped) | `rounded-thumb` |
| radius 4 | buttons, Kbd | `rounded-sm` |
| radius 5 | segment inside segmented control | `rounded-segment` |
| radius 6 | nav items, search field, path field, select, stat tiles, overflow button, close button | `rounded-control` |
| radius 8 | 40×40 icon boxes, segmented track, community note box | `rounded-md` |
| radius 10 | **every card** | `rounded-card` (new alias, = `rounded-popover`) |
| radius 12 | modal outline | `rounded-dialog` |
| radius 999 | pills, switch, avatar, changed dot | `rounded-full` |
| font 9 / 10 / 11 | NOVO badge (not shipped) / nav section labels / card headers, Kbd, stat captions, footer version | `text-badge` / `text-overline` / `text-caption` |
| font 12 / 13 / 14 | help text, pills, links / folder paths, select values, cache rows / row labels, nav labels, title-bar title | `text-xs` / `text-body` / `text-sm` |
| font 15 / 16 / 22 | tool and plugin names / account name, store name, stat values, version line / panel title | `text-title` / `text-md` / `text-heading` |
| line height 17 | wrapped 12px help text | `leading-note` |
| letter spacing 0.8 | nav section labels, card headers | `tracking-label` |
| spacing 8 / 10 / 12 / 16 / 20 / 24 / 32 | gaps and paddings | `compact` / `xs` / `sm` / `md` / `lg` / `control-inline` / `xl` |
| spacing 14 | row padding-y and row gap, card header top padding | `row` (**new**) |
| spacing 2 / 3 / 4 / 6 / 18 / 28 | small gaps, About version card gap, panel top padding | arbitrary px: `gap-[2px]`, `gap-[3px]`, `gap-[4px]`, `gap-[6px]`, `gap-[18px]`, `pt-[28px]` |
| shadow `0 24 64 #00000099` | modal | `shadow-alert` (existing, `rgba(0,0,0,0.6)`) — replaces `shadow-modal` on Settings |
| width 216 → 232 | navigation column | `w-settings-nav` (**new**) |

## New tokens (add to `ui/src/styles/theme.css` `:root` and `ui/tailwind.config.ts`)

| Purpose | CSS variable | Value | Tailwind key |
|---|---|---|---|
| Card radius alias | `--radius-card` | `var(--radius-popover)` (10px) | borderRadius `card` → `rounded-card` |
| Row rhythm (row padding-y, row gap, header top padding) | `--spacing-row` | 14px | spacing `row` → `py-row`, `gap-row`, `pt-row` |
| Settings navigation width | `--settings-nav-width` | 232px | width `settings-nav` → `w-settings-nav` |
| Help-text measure | `--settings-copy-max-width` | 400px | maxWidth `settings-copy` → `max-w-settings-copy` |
| Bottom scroll fade (56px, content area) | `--gradient-settings-fade` | `linear-gradient(180deg, rgba(11,11,11,0) 0%, #0B0B0B 100%)` | backgroundImage `settings-fade` → `bg-settings-fade` |
| Account avatar fallback | `--gradient-avatar` | `linear-gradient(135deg, #F15025 0%, #872C13 100%)` | backgroundImage `avatar` → `bg-avatar` |

Settings uses `FloatingModal` with `rounded-dialog border border-border-overlay bg-canvas shadow-alert`.
After the migration, delete `ui/src/features/settings/settingsClasses.ts` and every `--settings-*` variable
with no remaining reference. `--settings-toggle-track-off` (Switch) and `--settings-accent-hover` (Button
primary hover) stay unless the implementer renames them to `--color-switch-track-off` / `--color-primary-hover`
and updates every consumer (`Button`, `Switch`, `GameTile`, `ErrorBoundary`, `RecentLogModal`,
`LicenseNoticesModal`) in the same task.

## Tone → classes (pills, icon boxes, card borders, strips)

| Tone | Background | Icon / text | Card border |
|---|---|---|---|
| brand | `bg-accent-subtle` | `text-brand` | `border-brand` (changed path field) |
| success | `bg-success-subtle` | `text-success` | — |
| warning | `bg-warning-subtle` | `text-warning` | `border-warning` (expired store / plugin card) |
| danger | `bg-danger-subtle` | `text-danger` | `border-danger` (credential/persist failure, scan failure) |
| neutral | `bg-raised` | `text-text-default` | `border-border-subtle` |
