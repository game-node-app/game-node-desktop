# GameNode Desktop design

This is the visual and interaction design guide for the GameNode Desktop UI (`ui/`). It covers
product principles, design tokens, layout, components, states and accessibility, and says where
each of them lives in code.

- **Token source:** the **Version 3.0** page (`3994:677`) of the
  [GameNode Figma file](https://www.figma.com/design/ikcThguwowTdynGz0L2YkS/GameNode?node-id=3994-677),
  plus the settings-modal section **Menus · Premium v2 (Sam)** (`4987:10202`).
- **Recorded snapshot:** `ui/src/assets/theme-provenance.json`. It lists each token's Figma variable,
  node ID and alias chain. Builds never contact Figma.
- **Code:** `ui/src/styles/theme.css` (CSS variables), `ui/tailwind.config.ts` (utility names) and
  `appTheme` in `ui/src/AppProviders.tsx` (Mantine).

When this document and the code disagree, the document wins. Fix whichever one is wrong in the same
change.

---

## 1. Principles

1. **A native program with GameNode's look, not the website in a window.** Figma screens show the
   web product (feeds, reviews, profiles, premium, social). Desktop borrows their visual treatment
   only. A feature does not ship because it appears in a reference frame.
2. **The library comes first.** Covers and titles carry the colour. The UI around them stays dark,
   flat and quiet.
3. **Keyboard first.** Every action works from the keyboard, and the focus ring is always visible.
   Desktop idioms are expected: context menus, multi-select, split panes, resizable panes and
   remembered layout.
4. **Dense but readable.** The base size is 14 px. Panels sit edge to edge, separated by 1 px
   borders. Large padding is kept for modals and empty states.
5. **One accent.** Brand orange `#F15025` marks the primary action, the current selection and focus.
   Do not use it for decoration.
6. **Dark only.** There is no light theme, theme switcher or premium palette.
7. **Never block the UI thread.** This is a design constraint as much as an engineering one.
   Loading, scanning and syncing appear as inline status, never as a frozen window (see
   `AGENTS.md`).

---

## 2. Colour

All values are sRGB. A `1F` alpha suffix (≈12 %) is the Figma source's own value. Do not round it to
15 %.

### 2.1 Core palette (`GameNode / Semantic`, Dark)

| Role | Value | CSS variable | Tailwind | Use |
| --- | --- | --- | --- | --- |
| Primary / accent | `#F15025` | `--color-primary` | `brand`, `brand-5` | Primary button, selection, focus ring, active nav item |
| Window background | `#161616` | `--color-background` | `body`, `paper` | App canvas, library viewport |
| Surface | `#262525` | `--color-surface` | `surface`, `paper-1` | Inputs in the shell, hover fill, badges |
| Secondary | `#1A1A1A` | `--color-secondary` | `secondary` | Detail pane, cards, popovers |
| Raised | `#222222` | `--color-raised` | `raised` | Selected or raised items inside a card, secondary buttons |
| Tertiary | `#872C13` | `--color-tertiary` | `tertiary` | Pressed primary state, deep brand fills |
| Chrome | `#111111` | `--color-chrome` | `chrome` | Top bar, sidebar, toolbar, settings nav |
| Chrome border | `#202020` | `--color-chrome-border` | `chrome-border` | Measured top-bar stroke |
| Subtle border | `#2A2A2A` | `--color-border-subtle` | `border-subtle` | Every 1 px divider, card border and input border |

### 2.2 Text

| Role | Value | Tailwind | Use |
| --- | --- | --- | --- |
| Default | `#D9D9D9` | `text-text-default` | Body text, titles and controls in the shell and library |
| Dimmed | `#808080` | `text-text-dimmed` | Metadata, counts, group headers, placeholders |
| On primary | `#EDEDED` | `text-on-primary` | Labels on brand-filled buttons |

Dimmed text is for secondary information only. Instructions and control labels use the default
colour.

### 2.3 Status

| Role | Solid | Subtle (`1F`) | Use |
| --- | --- | --- | --- |
| Success | `#3DD68C` | `success-subtle` | Signed in, matched, running, saved |
| Warning | `#F5A524` | `warning-subtle` | Ambiguous match, provider sync failure, unsaved changes, favourite star |
| Danger | `#E5484D` | `danger-subtle` | Destructive actions, errors |
| Accent | `#F15025` | `accent-subtle` | Selected row or tile fill, brand pills |

Pair a status colour with an icon or text. Colour alone is never the only signal. The library's
match dot, for example, has a `title` tooltip and a status label in the detail pane.

### 2.4 Brand scale

The Mantine and Tailwind `brand` scale (`brand-0` … `brand-9`) runs from `#ffede5` to `#a82301`.
`brand-5` is the primary colour, and `brand-4` (`#f36742`) is its hover state. Use the other steps
only for Mantine's derived states.

### 2.5 Settings-modal and dialog palette

The settings modal follows the newer "Menus · Premium v2" designs, which use a darker base and
higher-contrast text. These tokens are scoped to the settings modal and the dialogs styled with it.
Do not use them in the library.

| Token (`--settings-*`) | Value | Use |
| --- | --- | --- |
| `bg-base` | `#0B0B0B` | Modal body, inputs, stats |
| `bg-sidebar` | `#111111` | Settings navigation |
| `bg-surface` | `#1A1A1A` | Cards |
| `bg-raised` | `#222222` | Selected nav item, secondary button |
| `text-primary` | `#FFFFFF` | Titles, labels, button text |
| `text-secondary` | `#ADADAD` | Descriptions, nav items, ghost buttons |
| `text-muted` | `#7A7A7A` | Group headers, metadata, footer |
| `accent-hover` | `#F3623B` | Primary hover |
| `accent-pressed` | `#872C13` | Primary pressed |
| `overlay` | `#0000009E` (62 % black) | Backdrop behind every modal |
| `scrollbar-thumb` | `#FFFFFF38` | Scrollbar thumbs app-wide |
| `toggle-*` | track on `#1A1A1A`, track off and border `#2E2E2E`, knob on `#F15025`, knob off `#7A7A7A` | Switches |

The off knob is `#7A7A7A` rather than the source's `#1D1D1D`. The source reached about 1.2:1 against
its track, which fails the WCAG 1.4.11 minimum of 3:1. This is a recorded desktop adaptation.

---

## 3. Typography

**Roboto Variable** is bundled at `ui/src/assets/fonts/Roboto-Variable.ttf` with weights 100–900, so
it never loads from the network. The fallback stack is
`-apple-system, BlinkMacSystemFont, 'Segoe UI', Oxygen, Ubuntu, Cantarell, sans-serif`. The body
default is 14 px with a line height of 1.4 and antialiased rendering.

| Style | Size / weight | Tailwind | Figma source | Use |
| --- | --- | --- | --- | --- |
| Page / panel title | 20 / 500 | `text-xl font-medium` | `text/xl/medium` | Settings panel title, alert title |
| Pane title | 18 / 600 | `text-lg font-semibold` | (desktop) | Detail pane header, game title |
| Section heading | 16 / 500–600 | `text-md font-medium` | `text/md/medium` | Settings section, detail-pane section |
| Label | 14 / 600 | `text-sm font-semibold` | `text/sm/semibold` | Signed-in user, active nav item, row title |
| Body | 14 / 400 | `text-sm` | `text/sm/regular` | Menus, list rows, inputs, paragraphs |
| Caption | 13 / 400, 18 px line height | `text-[13px]` | `Landing V3/Caption` | Descriptions, help text, user name in the top bar |
| Small / meta | 12 / 400 | `text-xs` | `text/xs/regular` | Provider name, playtime, counts, pills |
| Group header | 11 / 600, uppercase, 0.4–0.8 px tracking | `text-[11px] font-semibold uppercase tracking-[0.8px]` | settings nav header | "YOUR SPACE", field labels |
| Primary button | 14 / 700 | `font-bold` | button `4331:3200` | Large primary call to action |

Rules:

- Use the token sizes `xs` / `sm` / `md` / `lg` / `xl` (12 / 14 / 16 / 18 / 20). The 13 px caption
  and 11 px group header are the only named exceptions.
- Letter spacing is 0 except on uppercase group headers and the `DESKTOP` badge.
- Uppercase labels are written in uppercase in the translation catalogue (as with `YOUR SPACE`), so
  each language controls its own casing.
- Truncate single-line labels with `truncate`. Game titles in tiles get `line-clamp-2` and a `title`
  attribute containing the full text.

---

## 4. Spacing, radii and sizes

### 4.1 Spacing

The `GameNode / Dimensions` scale, exposed as Tailwind spacing keys:

| Key | Value | Typical use |
| --- | --- | --- |
| `compact` | 8 px | Gaps between inline items, tight padding, card padding (`4330:3082`) |
| `xs` | 10 px | Toolbar vertical padding, info-box padding |
| `sm` | 12 px | Gap between rows, nav item horizontal padding, grid gutter |
| `md` | 16 px | Panel padding, top-bar horizontal padding, section gap |
| `lg` | 20 px | Settings row horizontal padding |
| `xl` | 32 px | Settings section gap, empty-state offset |
| `control-inline` | 24 px | Horizontal padding of the large primary button |

Tailwind's numeric scale (`gap-1`, `p-2`, …) is fine for 2–6 px micro-adjustments inside a
component. Use the named keys for layout rhythm.

### 4.2 Radii

| Key | Value | Use |
| --- | --- | --- |
| `rounded-xs` | 2 px | Badges, small inline buttons, scrollbar thumb |
| `rounded-sm` | 4 px | Buttons, inputs, cards, covers, toasts (default for controls) |
| `rounded-md` | 8 px | Nav items, game tiles, modals, popovers (Mantine `defaultRadius`) |
| `rounded-lg` | 16 px | Large panels, alert dialogs |
| `rounded-xl` | 32 px | Reserved |
| `rounded-full` | 9999 px | Pills, avatars, status dots, switches |

### 4.3 Fixed sizes

| Token | Value | Notes |
| --- | --- | --- |
| `h-topbar` | 54 px | Top bar (`4330:1986`) |
| `h-control` | 36 px | Primary control height (`4331:3200`) |
| Toolbar control | 32 px (`h-8`) | Search field and toolbar toggles |
| `w-sidebar` / `w-sidebar-compact` | 216 / 176 px | Settings nav. The library sidebar resizes from 160 to 360 px |
| `max-w-modal` × `max-h-modal` | 960 × 720 px | Settings modal default size |
| Modal margin | 24 px (16 px compact) | Gap kept between a modal and the window edge |
| Logo | 72.8 × 28 px | Preserve the wordmark's aspect ratio |
| Game tile | 166 × 294 px, cover 150 × 200 px | Grid gutter 12 px |
| Scrollbar | 6 px, transparent track | Global, in `theme.css` |

Figma pixels map 1:1 to CSS pixels. The OS and WebView handle DPI scaling. Reference frame widths
(1440 px) are not window requirements.

### 4.4 Elevation

The UI is flat. Panels are separated by borders, not shadows. Only floating layers use shadows:

| Tailwind | Value | Use |
| --- | --- | --- |
| `shadow-modal` | `0 32px 80px rgba(0,0,0,.55)` | `FloatingModal` work surfaces |
| `shadow-alert` | `0 24px 64px rgba(0,0,0,.6)` | Alerts, popovers, filter panels |
| `shadow-toast` | `0 12px 32px rgba(0,0,0,.5)` | Toasts |

Stacking order, bottom to top: content → top bar (`z-[100]`) → modals (from `z-1000`, newest on
top, managed by `modalLayer`) → toasts (`z-[10000]`).

---

## 5. Layout

```
┌ Top bar · 54 px · chrome · drag region ───────────────────────────────────────┐
│ GameNode [DESKTOP]                                    ● username   [≡ menu]   │
├──────────────┬───────────────────────────────────────────┬────────────────────┤
│ Sidebar      │ Notice banners (accent / warning subtle)  │▏Detail pane        │
│ chrome       ├───────────────────────────────────────────┤▏secondary          │
│ 160–360 px   │ Toolbar · chrome · search, filters, view  │▏resizable          │
│ resizable    ├───────────────────────────────────────────┤▏                   │
│              │                                           │▏ cover 180 px      │
│ YOUR SPACE   │  Library grid / list (virtualized)        │▏ title, metadata   │
│ ▸ All games  │  background #161616                       │▏ Play / Install    │
│   Steam      │                                           │▏ playtime, tools   │
│   Local      │                                           │▏                   │
│ …            │                                           │▏                   │
│ ⚙ Settings v │                                           │▏                   │
└──────────────┴───────────────────────────────────────────┴────────────────────┘
```

- **Top bar.** `bg-chrome`, 1 px bottom border, `px-md`, `data-tauri-drag-region` on the bar and on
  non-interactive children. On the left are the brand and the `DESKTOP` badge. On the right are the
  online dot with the user name, then the application-menu button (32 px, bordered).
- **Sidebar.** `bg-chrome` with a right border. It has an uppercase group header followed by
  navigation items. The active item uses `border-brand bg-accent-subtle text-brand font-semibold`.
  Counts are right-aligned at `text-xs`. Settings and the version sit at the bottom.
- **Toolbar.** `bg-chrome`, bottom border, `px-md py-xs`, wraps on narrow windows. Search comes first
  and grows to fill the row. Filter popovers use `bg-secondary`, `rounded-md` and `shadow-alert`.
- **Content.** `bg-body`. The grid and list must use `@tanstack/react-virtual`.
- **Detail pane.** `bg-secondary` with a left border, resizable through an 8 px hit area. Its 1 px
  divider turns `brand` on hover or focus. The header is `text-lg font-semibold`, the body `p-md`,
  `gap-md`.
- **Resizing.** Pane widths are runtime values, so they are the accepted use of inline `style`.
  They are clamped and persisted through the engine's layout state.
- **Narrow windows.** The toolbar wraps, panes clamp to their minimums, and the settings modal
  switches to its compact sidebar below 760 px. There are no web breakpoints or mobile layouts.

---

## 6. Components

Reuse what exists before creating anything new. Shared primitives live in `ui/src/components/`.
Settings-styled utility strings are in `ui/src/features/settings/settingsClasses.ts`.

### 6.1 Overlays

| Need | Use | Notes |
| --- | --- | --- |
| Large work surface (settings, game tools, local-game editor, discovery) | `FloatingModal` | Draggable by `[data-floating-modal-handle]`, resizable, geometry persisted by `geometryKey`. 62 % overlay. `rounded-md`, `border-border-subtle`, `shadow-modal` |
| Confirmation, notice, short form | `AppModal` | Centred Mantine `Modal.Root`, no transition, 62 % overlay, header with bottom border |
| Context or app menu | Mantine `Menu` | Right-click on rows and tiles, plus the top-bar menu |
| Transient result | `ToastContainer` (Mantine `Notification`) | Bottom centre, 24 px from the edge. Success or danger border and icon. Never takes focus |
| Persistent condition | Notice banner at the top of the library | `bg-accent-subtle` (info) or `bg-warning-subtle text-warning` (action needed), with an optional inline bordered action |

Both modal primitives share the last-opened-on-top layer stack, focus trapping, Escape handling and
trigger-focus restoration. Do not build these behaviours yourself, and do not stack an `AppModal`
under a `FloatingModal` by hand. Escape closes only the topmost layer. A dirty settings form asks
before discarding.

### 6.2 Buttons

| Variant | Classes (summary) | When |
| --- | --- | --- |
| Primary | `bg-brand text-on-primary` (shell) or `text-white` (settings), hover `bg-brand-4` | The one main action in a view: Play, Save, Sign in |
| Secondary | `border border-border-subtle bg-raised` (settings) or `bg-transparent` (detail pane), hover one step lighter | Alternatives: Install, Cancel |
| Ghost | No fill, `text-[var(--settings-text-secondary)]`, hover `bg-raised text-white` | Low-emphasis actions such as Discard or Close |
| Danger | `bg-danger text-white` | Destructive confirmation only, inside a dialog |
| Icon | 32 px square, `rounded-sm`, hover `bg-surface` or `bg-raised` | Toolbar and header actions. **Requires** `aria-label` |

Shared base: `inline-flex items-center justify-center gap-[6px] rounded-sm text-sm font-medium
transition-colors`, with `disabled:opacity-50 disabled:cursor-not-allowed`. Show at most one primary
button per surface. When an action is unavailable, keep the button visible and disabled, and give
the reason in a caption directly below it, as the detail pane does with `reasonClass`.

### 6.3 Form controls

- **Text input:** `rounded-sm border border-border-subtle`, `bg-surface` in the shell or
  `bg-[var(--settings-bg-base)]` in settings, `focus:border-brand`, placeholder in `text-dimmed`.
  Field labels are 11 px uppercase dimmed text above the input.
- **Switch, segmented control and checkbox:** Mantine `Switch`, `SegmentedControl` and `Checkbox`,
  themed by `appTheme`. A switch row is a `settingsRow`, with title and description on the left and
  the control on the right. Clicking the label toggles the control.
- **Pills:** `rounded-full px-compact py-[2px] text-xs font-medium` in four variants: neutral
  (`bg-raised`), brand (`bg-accent-subtle text-brand`), success and warning (each on its subtle
  background).
- **Info box:** a 3 px left border in brand or warning on `bg-secondary`, `text-xs`.

### 6.4 Cards and settings rows

- `settingsCard`: `rounded-sm border border-border-subtle bg-secondary p-md`, `flex-col gap-sm`.
- `settingsSection`: title (`text-md font-semibold`), description (`text-sm`, secondary), then cards.
- `settingsRow`: `flex items-center justify-between gap-md`. The info column has a title
  (`text-sm font-medium`) and a description (`text-xs`, secondary).
- Settings apply predictably. Each panel either applies changes immediately (switches) or shows an
  action bar with "Unsaved changes", Discard and Save. Do not mix the two models within one section.

### 6.5 Library

- **Game tile** (grid): 166 × 294 px, `rounded-md p-2`. The cover is 150 × 200 px with `rounded-sm`.
  Below it are the title (`text-sm font-medium`, two lines, 17 px line height) and a meta line with
  the match dot, provider name and right-aligned 12 px status icons (favourite, installed, family
  shared). Playtime goes last.
- **Selection:** a selected item gets `border-brand bg-accent-subtle`. The keyboard-focused item
  gets a 2 px `border-text-default`. Multi-select uses Ctrl or Shift click and the keyboard.
- **Running:** append "· Running" in `text-success font-semibold` to the title.
- **Match dot:** success when matched, warning when ambiguous, dimmed when unmatched.
- **List rows:** the same information as tiles in a single row, virtualized with the same selection
  treatment.
- **Empty and loading states:** centred `text-sm text-text-dimmed` with `pt-xl`. Offer the next
  action (rescan, add a game, sign in) instead of an illustration.

### 6.6 Covers and artwork

- Load covers only through the asset protocol: `gamenode-asset://localhost/cover/<file>` and
  `gamenode-asset://localhost/artwork/<game_id>`.
- Always use `<img loading="lazy" decoding="async" />` with `object-cover`, inside a container whose
  size is fixed, so the layout never shifts.
- Missing art falls back to a `bg-surface` placeholder that shows the title or detected name. Never
  show a broken-image icon.
- Covers are 3:4 (150 × 200 in tiles). The detail-pane banner is 180 px high, full width.

---

## 7. States and interaction

| State | Treatment |
| --- | --- |
| Hover | One surface step lighter: transparent → `surface` (shell) or `raised` (settings), `raised` → `#2E2E2E`. Brand → `brand-4`. Use `transition-colors` only |
| Pressed | Primary → `tertiary` (`#872C13`). Other controls darken slightly |
| Focus | `focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand`. Destructive controls use `ring-danger`. Inputs change their border to `brand` instead of showing a ring |
| Selected | `bg-accent-subtle` with a `brand` border and brand text (nav), or a brand border (tiles) |
| Disabled | `opacity-50` (settings) or 0.45 (dialogs), `cursor-not-allowed`, plus a reason caption when the cause isn't obvious |
| Busy | Keep the label and dim the control. Report progress inline in a status line or banner, never with a full-window spinner |
| Error | Danger text below the field or in a toast. Provider failures show a warning triangle beside the provider with a tooltip |

Motion is minimal. Use colour transitions (Tailwind's default ~150 ms), switch knob travel, and
nothing else. Modals open without a transition (`duration: 0`). Do not animate layout, covers or
scrolling.

Hover and focus states must come from CSS (`hover:`, `focus-visible:`, `group-hover:`), never from
JavaScript listeners.

---

## 8. Iconography and brand

- **Icons:** `@tabler/icons-react` only. Do not add loose SVG files.
  - Sizes: 12 px for inline status in tiles, 14–16 px for buttons, menus and navigation, 18 px for
    row tiles.
  - Stroke: `1.75` in the shell. Filled variants (`IconStarFilled`) are for state, not decoration.
  - A decorative icon gets `aria-hidden`. An icon-only control gets an `aria-label`. A meaningful
    standalone icon gets `role="img"`, an `aria-label` and a `title`.
- **Wordmark:** `ui/src/assets/icons/gamenode-logo.svg` is the exact Figma export. Keep its 72.8:28
  ratio and do not redraw or recolour it.
- **App icon:** `gamenode.ico` and `gamenode.png`.
- **`DESKTOP` badge:** 10 px bold, 0.8 px tracking, `bg-surface text-text-dimmed rounded-xs`. It
  tells the desktop product apart from the web app.

---

## 9. Accessibility

- Every action can be reached and activated from the keyboard, with a visible focus ring. Settings
  opens with `Ctrl+,`.
- Modals trap focus, close with Escape (topmost layer only) and return focus to the control that
  opened them. All of this comes from the shared primitives.
- Text contrast:
  - Default text `#D9D9D9` on `#161616` is about 12.8:1.
  - Dimmed text `#808080` on `#161616` is about 4.6:1. That meets AA, but only just.
  - Dimmed text on `surface` (`#262525`) drops to about 3.9:1, which fails AA for body text. Use
    default text on `surface` fills.
  - Non-text UI such as borders around controls, switch knobs and focus rings must reach 3:1.
- Lists and grids expose `role="list"` / `listitem` and `aria-selected`. Menus use `aria-haspopup`
  and `aria-expanded`.
- Respect the OS text scale. Because sizes are in pixels and the WebView zooms them, never fix the
  height of a container that holds wrapping text.

---

## 10. Content and localisation

- English and Brazilian Portuguese ship at launch. All copy goes through `useI18n().t()` and the
  typed catalogues in `ui/src/i18n/`. The OS locale decides the language, with English as the
  fallback.
- Lay out for Portuguese, which runs roughly 20–30 % longer than English. Labels truncate or wrap.
  They never overflow, and controls never depend on the width of an English string.
- Show game titles, collection names and store data exactly as received. Never translate them.
- Use sentence case for buttons and titles ("Add to GameNode", "All games"). Uppercase is for group
  headers only.
- Errors state what happened and what to do next. Do not surface raw error codes as the primary
  message.

---

## 11. Implementation rules

These rules restate the binding guardrails in `AGENTS.md`.

- Style with Tailwind utilities backed by the tokens above. Do **not** use static inline `style`
  objects for layout, colour, type, borders or states. Inline styles are only for runtime values:
  virtualizer offsets, measured pane widths and pointer-anchored positions.
- Prefer a named token (`bg-secondary`, `p-md`, `rounded-sm`). An arbitrary value (`text-[13px]`,
  `py-[7px]`) is acceptable only when it reproduces a measured Figma value that has no token. When
  the same arbitrary value appears in three places, promote it to a token.
- To read a settings token from Tailwind, use `text-[var(--settings-text-secondary)]`. That keeps
  the value traceable to `theme.css`.
- Use Mantine v9 for complex controls (`Menu`, `Notification`, `Switch`, `SegmentedControl`,
  `Checkbox`, `Modal`, `FloatingWindow`). It is themed by `appTheme`. Flat palettes
  (`secondary`, `tertiary`) use `appVariantColorResolver` so that `variant="light"` tints with the
  colour itself.
- Use `@tanstack/react-query` for engine data, and `@tanstack/react-store` (`uiStore`) for
  responsive UI state such as drag widths and open menus.

### Adding or changing a token

1. Find the source in Figma: a variable in `GameNode / Semantic` or `Dimensions`, or a measured node
   on the Version 3.0 page or in section `4987:10202`.
2. Add the CSS variable to `ui/src/styles/theme.css` and expose it in `ui/tailwind.config.ts`. Add it
   to `appTheme` as well if Mantine needs it.
3. Record its provenance in `ui/src/assets/theme-provenance.json`: the variable or node ID, the
   resolved value, and a `reason` for any desktop adaptation.
4. Keep `GameNode / Semantic` as the foundation. Do not import `Landing V3` values (`#0B0B0B`
   background, pure-white text, the 8/16/24/40/64 spacing scale) into the shell. They are recorded
   as alternatives only.
5. Keep the shared values (brand, paper, secondary, tertiary) identical to `game-node-clients`.

---

## 12. Known deviations

These are places where the current code departs from this guide. Fix them when you touch the code.
Do not copy them.

- The top bar draws the brand as text ("GameNode", `text-lg font-bold text-brand`) instead of the
  exported wordmark SVG, and its border uses `border-subtle` instead of the measured
  `chrome-border` (`#202020`).
- Some hover and background colours are hard-coded hex values that have no token: secondary hover
  `#2e2e2e`, danger hover `#f05a5f`, toast fills `#1e3a2f` and `#3d1c1d`, and fallback hex values in
  `getMatchDotColor`.
- Settings cards use `rounded-sm` (4 px), while the settings reference specifies 12 px cards and
  8 px controls. The 4 px value is the current, consistent choice. Change it everywhere at once or
  not at all.
- The `--color-chrome-border` variable and the `paper-2` … `paper-8` steps exist in the Tailwind
  config but are not used yet.

---

## 13. Figma reference map

File key `ikcThguwowTdynGz0L2YkS`. Use these frames for visual treatment only.

| Node | Name | Borrow | Ignore |
| --- | --- | --- | --- |
| `3994:677` | Version 3.0 page | Tokens, chrome, cards, buttons | — |
| `4945:752` / `4945:758` / `4945:728` | Semantic / Dimensions / Primitives variable collections | All core tokens | — |
| `4330:1986` | Top Bar | Height, padding, chrome fill and border, wordmark | Search and notification icons |
| `4331:3200` | Button / Primary | 36 px height, 24 px inline padding, 14/700 label | — |
| `4330:3082` | Cards / Info Card | 8 px padding, label-over-value pattern | — |
| `4330:1930` | Game Info · Overview | Detail layout: cover, metadata cards, Play row | Reviews, tabs, social, achievements as web features |
| `4117:3918` | Home | Cover rhythm, dark canvas | Feed, trending, blog |
| `4987:10202` | Menus · Premium v2 (Sam) | Settings modal structure, nav item, section, card, row, switch, pill, kbd, alert, toast | Premium, profile editing, notifications, MCP, privacy, appearance content |
| `4856:3048` | Configurações (Sam) / Tokens | Raised, subtle-border and status tokens | — |
