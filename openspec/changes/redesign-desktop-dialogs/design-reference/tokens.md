# Tokens

All colors in this section already exist (the library redesign added `text-primary`, `text-muted`, `canvas`,
`border-strong`, `border-overlay`, `border-floating`). This change adds **one shadow, one radius alias, three
font sizes, four line heights and two gradients**. Nothing else is new.

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
| `secondary` (dialog body) | #1A1A1A | `--color-secondary` | `bg-secondary` |
| `surface` (inputs, footer, unselected cards) | #262525 | `--color-surface` | `bg-surface` |
| `bg/raised` (segment on, sort button, neutral pill, track) | #222222 | `--color-raised` | `bg-raised` |
| `background` (inset cards, lists, preview pane) | #161616 | `--color-background` | `bg-body` |
| `bg/sidebar` (sign-in art pane) | #111111 | `--color-chrome` | `bg-chrome` |
| `bg/canvas` (segmented track, unchecked checkbox) | #0B0B0B | `--color-canvas` | `bg-canvas` |
| `border/subtle` | #2A2A2A | `--color-border-subtle` | `border-border-subtle` |
| `#333333` literal (dialog outline) | #333333 | `--color-border-overlay` | `border-border-overlay` |
| `#3A3A3A` literal (provider badge on sign-in) | #3A3A3A | `--color-border-floating` | `bg-border-floating` |
| `#4A4A4A` literal (unchecked checkbox border) | #4A4A4A | `--color-border-strong` | `border-border-strong` |
| `success` / `success-subtle` | #3DD68C / #3DD68C1F | `--color-success*` | `success`, `success-subtle` |
| `warning` / `warning-subtle` | #F5A524 / #F5A5241F | `--color-warning*` | `warning`, `warning-subtle` |
| `danger` / `danger-subtle` | #E5484D / #E5484D1F | `--color-danger*` | `danger`, `danger-subtle` |

## Existing size tokens used here

| Value | Use in these frames | Tailwind |
|---|---|---|
| radius 3 | thumb covers (24×32, 30×40, 28×37), progress track/fill | `rounded-thumb` |
| radius 4 | buttons, checkboxes, list covers 36×48 and 30×40 in AddDialog header | `rounded-sm` |
| radius 5 | segment inside segmented control, provider badge | `rounded-segment` |
| radius 6 | inputs, selects, textareas, radio rows, sort button, achievement icon, preview cover 84×112, editor cover 108×144 | `rounded-control` |
| radius 8 | inset cards, option cards, status cards, lists, callouts, provider buttons, segmented track, mural tiles | `rounded-md` |
| radius 12 | dialog outline (both shells) | `rounded-dialog` (new alias, = `rounded-palette`) |
| radius 999 | pills, switch, tone circle, radio | `rounded-full` |
| font 11 | overline labels, option meta, history reason, status line, version label | `text-caption` |
| font 12 | field labels, subtitles, notes, helper text, table cells | `text-xs` |
| font 13 | input values, option titles, row titles, confirmation body | `text-body` |
| font 14 | summary/status card titles, provider buttons, sign-in lede | `text-sm` |
| font 15 | preview pane title, sign-in art body | `text-title` |
| font 16 | FloatingModal title | `text-md` |
| letter spacing 0.8 | uppercase section labels (`SectionLabel`) | `tracking-label` |
| letter spacing 0.6 | Discovery table header | `tracking-overline` |
| spacing 8 / 10 / 12 / 16 / 20 / 24 | gaps and paddings | `compact` / `xs` / `sm` / `md` / `lg` / `control-inline` |
| spacing 2 / 3 / 4 / 6 / 14 / 48 | small gaps, footer padding, art-pane padding | Tailwind default scale: `0.5` / `[3px]` / `1` / `1.5` / `3.5` / `12` |

## New tokens (add to `ui/src/styles/theme.css` `:root` and `ui/tailwind.config.ts`)

| Purpose | CSS variable | Value | Tailwind key |
|---|---|---|---|
| Dialog shadow (both shells) | `--shadow-dialog` | `0 20px 48px rgba(0, 0, 0, 0.55)` (Figma `#0000008C`) | boxShadow `dialog` → `shadow-dialog` |
| Dialog radius alias | `--radius-dialog` | `var(--radius-palette)` (12px) | borderRadius `dialog` → `rounded-dialog` |
| Font 17 (AppModal confirmation title) | `--font-size-dialog-title` | 17px | fontSize `dialog-title` |
| Font 26 (sign-in welcome heading) | `--font-size-display` | 26px | fontSize `display` |
| Font 28 (sign-in art headline) | `--font-size-hero` | 28px | fontSize `hero` |
| Line height 17 (12px helper and option descriptions) | — | 17px | lineHeight `note` → `leading-note` |
| Line height 19 (13px textarea) | — | 19px | lineHeight `field` → `leading-field` |
| Line height 20 (13px confirmation body) | — | 20px | lineHeight `body-relaxed` → `leading-body-relaxed` |
| Line height 22 (15px sign-in art body) | — | 22px | lineHeight `hero` → `leading-hero` |
| Sign-in art side fade (left→right) | `--gradient-signin-side` | `linear-gradient(90deg, rgba(17,17,17,0.25) 0%, rgba(17,17,17,0.95) 100%)` | backgroundImage `signin-side` → `bg-signin-side` |
| Sign-in art bottom fade (top→bottom) | `--gradient-signin-bottom` | `linear-gradient(180deg, rgba(22,22,22,0.6) 0%, rgba(22,22,22,1) 100%)` | backgroundImage `signin-bottom` → `bg-signin-bottom` |

`FloatingModal.Content` and `AppModal.Content` default classes change from `rounded-md … shadow-modal`
to `rounded-dialog border border-border-overlay shadow-dialog bg-secondary`. `shadow-modal` stays for the
Settings modal, which this change does not restyle.

## Tone → classes (tone circle, pills, callouts, checklist icons)

| Tone | Circle / callout background | Icon / text | Callout border |
|---|---|---|---|
| accent | `bg-accent-subtle` | `text-brand` | `border-brand` |
| success | `bg-success-subtle` | `text-success` | `border-success` |
| warning | `bg-warning-subtle` | `text-warning` | `border-warning` |
| danger | `bg-danger-subtle` | `text-danger` | `border-danger` |
| neutral | `bg-raised` | `text-text-muted` | `border-border-subtle` |

## Confidence → tone (match options, discovery suggestion)

Reuse `SuggestionTone` from `crates/core/src/library/suggest.rs`: ≥ 80% `success`, ≥ 50% `warning`,
otherwise `neutral`. Figma: 96% success, 74% warning, 38% neutral.
