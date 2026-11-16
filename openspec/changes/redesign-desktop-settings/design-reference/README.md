# Design reference — 03 · Configurações (redesign)

Everything this change needs from Figma is extracted here, so the implementer does **not** need Figma
access. Source file: `ikcThguwowTdynGz0L2YkS` (GameNode), section `5373:3196` "03 · Configurações"
(3120×4340). The library (`redesign-desktop-library`) and dialog (`redesign-desktop-dialogs`) redesigns are
implemented; their tokens, primitives, `FloatingModal`/`AppModal` shells and `DiscardChangesDialog` are the
base this change builds on.

| File | What it holds |
|---|---|
| `screens/S1…S8-*.png` | Rendered modal (960×720 plus 64px of shadow margin) for each topic — visual target. `settings-overview.png` is the whole section with the designer's annotations. |
| `tokens.md` | Figma variable → CSS variable → Tailwind class, plus the **few new tokens** this change adds and the legacy `--settings-*` cleanup. |
| `components.md` | Exact specs of the shell, search, panel header, card, row, icon box, stat tile, path field, select, unsaved-changes bar and per-panel list rows; adjustments to `Switch` and `SegmentedControl`. |
| `icons.md` | Figma icon name → `@tabler/icons-react` component and where each is used; native-access → permission pill map. |
| `copy.md` | Every user-facing string, pt-BR (from Figma) + English, per topic; out-of-scope strings marked **OUT**. |
| `raw/*.txt` | Compact node dumps (layout, sizes, paddings, gaps, colors, typography, icons, copy). Start with `raw/00-shell-nav.txt`; `raw/09-components.txt` has the component sets. |

## Frame index

| # | Topic | Code (after this change) | Figma frame (modal) | Raw | Screenshot |
|---|---|---|---|---|---|
| — | Shell + navigation (all frames) | `features/settings/SettingsModal.tsx` | every modal below | `00` | all |
| S1 | Conta e sincronização | `panels/AccountPanel.tsx` | `5373:3197` (`5373:3436`) | `01` | `S1-account-sync.png` |
| S2 | Lojas | `panels/StoresPanel.tsx`, `StoreAccountCard.tsx` | `5374:3847` (`5374:4086`) | `02` | `S2-stores.png` |
| S3 | Jogos DRM-free | `panels/DrmFreePanel.tsx` (**new**, moved out of Lojas) | `5374:5012` (`5374:5251`) | `03` | `S3-drm-free.png` |
| S4 | Backups (with pending changes) | `panels/BackupsPanel.tsx` | `5375:5208` (`5375:5447`) | `04` | `S4-backups.png` |
| S5 | Ferramentas | `panels/ToolsPanel.tsx` | `5375:6321` (`5375:6560`) | `05` | `S5-tools.png` |
| S6 | Plugins | `panels/PluginsPanel.tsx` (replaces `PluginInventory.tsx` list/detail) | `5376:6506` (`5376:6745`) | `06` | `S6-plugins.png` |
| S7 | Sobre | `panels/AboutPanel.tsx` | `5376:7732` (`5376:7971`) | `07` | `S7-about.png` |
| S8 | Geral | `panels/GeneralPanel.tsx` (**new**) | `5377:7917` (`5377:8156`) | `08` | `S8-general.png` |

Each frame is 1440×900: the library window, the `#0000009E` overlay and the modal at (240, 90). Figma shows
one scroll position per topic; content below the fold is in the raw dumps.

## How to read `raw/*.txt`

Each line is one node, indented by depth.

- `F [name] WxH …` frame · `T "text" size/weight …` text · `I icon-name size:color` Tabler icon ·
  `C <Component Variant> WxH "texts" icons=…` instance of a shared component · `REC`/`ELL` shapes.
- Layout: `row`/`col`, `gapN`, `pT/R/B/L` (or `pN` when equal, `pV/H` for two values), `jc:` justify,
  `ai:` align (`ai:max` = end). Sizing pair: `FF` fill/fill, `FH` fill width/hug height, `HH`, `XH` fixed
  width (first letter = horizontal); `XH(400)` = fixed 400px width (max text measure).
- Paint: `bg=` fill, `bd1=` 1px stroke (or `bdT1` top only / `bd0/0/1/0` per side T/R/B/L), `rN` radius,
  `sh(x,y,blur,spread,color)`.
- Colors are **Figma variable names** (see `tokens.md`) or raw hex when the designer used a literal.
  `accent/subtle`, `success-subtle`, `warning-subtle` already include their alpha — never add opacity.
- Text: `13/600` = 13px weight 600 (Roboto), `lhN` line height px, `lsN` letter spacing px, `upper`, `ellipsis`.
- `# …` comments are extraction notes (scope, fixture, behavior), not design.

## Rules that apply everywhere

1. Fixture names, numbers, dates and paths (`cassi`, `164`, `D:\Jogos`, `0.9.0`, `há 2 min`) are sample data.
   Render real data with the locale formatters.
2. `NOVO` / `(NOVO)` in Figma is a **design annotation** marking a new feature (badges in nav, rows and card
   headers). Never ship the word or the badge.
3. Out of scope — do not render: the **Privacidade** card ("Mostrar “Jogando agora” no perfil"); the app
   **updater** ("Atualizado" pill, "Procurar atualizações", "canal Estável" in the build line); the backup
   **Limite de espaço** row and every quota part of "Uso do espaço" (`de 5 GB`, `46%`, the bar).
4. Gradient rectangles are placeholders for real images (backup usage covers over
   `gamenode-asset://localhost/cover/<file>` with `<img loading="lazy" decoding="async">`). The account
   avatar gradient is real (fallback when there is no avatar image) and the bottom scroll fade is real.
5. Tailwind classes backed by tokens; inline `style` only for runtime values (measured widths, pointer
   positions). Root font size is 14px — see `tokens.md`.
6. Mantine v9 through the wrappers in `ui/src/components/ui/` for `Switch`, `SegmentedControl`, selects
   (`SelectField`), menus and notifications. The modal stays a `FloatingModal`; confirmations stay `AppModal`
   (`DiscardChangesDialog`, existing remove/sign-out confirmations). Do not hand-roll layer stack, focus
   trap, Escape or focus restoration.
7. The whole title bar is the drag handle (`FloatingModal.Header`); the `dots-vertical` icon is its affordance.
   The `Esc` key cap is a decorative hint, not a button.
8. Provider names, icons and shared labels come from provider metadata (`ProviderCount`, `StoreAccountView`,
   plugin descriptors). Never hard-code "Steam"/"Epic" in panel code beyond the icon map.
9. Primary buttons render disabled at the existing `Button` disabled opacity (0.5).
