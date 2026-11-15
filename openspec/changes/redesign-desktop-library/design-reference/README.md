# Design reference — 01 · Biblioteca (redesign)

Everything needed from Figma is extracted here, so an implementer does **not** need Figma access.
Source file: `ikcThguwowTdynGz0L2YkS` (GameNode). Section `5361:766` (Biblioteca), components `5357:677`,
Tabler icons `5356:678`, overview `5377:9022`.

| File | What it holds |
|---|---|
| `screens/*.png` | Rendered frames (visual target). L1 grid+detail, L2 list grouped + multi-select, L3 filter popover, L4 menus/popovers, L5 states, L6 detail variations, components sheet, icon sheet, overview. |
| `tokens.md` | Figma variable → CSS variable → Tailwind class mapping, plus the **new tokens** this change adds. |
| `components.md` | Exact specs of the shared primitives (Button, Pill, Switch, Checkbox, Nav item, Kbd, Game tile, Game row, IconButton, Menu, Tooltip, StatCard, SectionHeader, KeyValueRow, Banner, Notification). |
| `icons.md` | Figma icon name → `@tabler/icons-react` component, and which icon each feature uses. |
| `copy.md` | Every user-facing string, pt-BR (from Figma) + English, grouped by surface. |
| `catalog-view.md` | Spec for the in-app GameNode catalog ("Explorar no GameNode"), which has **no Figma frame**. |
| `raw/*.txt` | Compact node dumps straight from Figma (layout, sizes, paddings, gaps, colors, typography, icons, copy). |

## How to read `raw/*.txt`

Each line is one node, indented by depth.

- `F [name] WxH …` frame · `T "text" size/weight …` text · `I icon-name size:color` Tabler icon ·
  `C <Component Variant> WxH "texts" icons=…` instance of a shared component · `REC`/`ELL` shapes.
- Layout: `row`/`col` (flex direction), `gapN`, `pT/R/B/L` (or `pN` when equal), `jc:` justify, `ai:` align,
  `wrap`. Sizing pair: `FF` fill/fill, `FH` fill width/hug height, `HF`, `HH` (first letter = horizontal).
- Paint: `bg=` fill, `bdW=` stroke (W = weight, or `t/r/b/l` per side), `rN` radius, `sh(x,y,blur,spread,color)`.
- Colors are **Figma variable names** (see `tokens.md`) or raw hex when the designer used a literal.
- Text: `13/500` = 13px weight 500, `lhN` line height px, `lsN` letter spacing px, `upper` uppercase,
  `ellipsis` single-line truncation, `fillW` grows to fill.
- Notes in parentheses `( … )` were added during extraction to explain behaviour.

## Rules that apply everywhere

1. Fixture names/numbers in Figma (`Emberfall`, `248 jogos`, `cassi`) are sample data. Render real data.
2. `NOVO` badges in Figma are **design annotations** marking new features. Do not ship them.
3. Decorative gradients on covers/heroes/achievements are placeholders for real images
   (`gamenode-asset://localhost/cover/<file>`, `…/artwork/<game_id>`, achievement icons). Keep the
   bottom fade gradient overlays; they are real.
4. Use Tailwind classes backed by tokens; inline `style` only for runtime values (virtualizer offsets,
   pane widths, progress widths, pointer positions).
5. Use Mantine v9 for `Menu`, `Popover`, `Switch`, `SegmentedControl`, `Checkbox`, `Notification`,
   `Tooltip`, restyled through `classNames` to these specs. Use `AppModal` for the save-view dialog
   and the keyboard-shortcuts dialog, and a Mantine `Modal`-free palette built on the shared modal layer
   for the command palette (see design.md).
6. Ports (`Ports para PC`) keep their current screen; only the sidebar entry is restyled.
