# Design reference — 02 · Diálogos e modais (redesign)

Everything this change needs from Figma is extracted here, so the implementer does **not** need Figma
access. Source file: `ikcThguwowTdynGz0L2YkS` (GameNode), page "Desktop · Redesign 2026" (`5356:677`),
section `5369:2801` "02 · Diálogos e modais". The library redesign (`redesign-desktop-library`) is
already implemented; its tokens and primitives are the base this change builds on.

| File | What it holds |
|---|---|
| `screens/D01…D14-*.png` | Rendered frames with the designer's title and annotation on top (visual target). `dialogs-overview.png` is the whole section. |
| `tokens.md` | Figma variable → CSS variable → Tailwind class, plus the **few new tokens** this change adds. |
| `components.md` | Exact specs of the shared dialog pieces: work-surface shell, confirmation shell, fields, option cards, radio, progress bar, callout, checklist, table, cover mural. |
| `icons.md` | Figma icon name → `@tabler/icons-react` component and where each is used. |
| `copy.md` | Every user-facing string, pt-BR (from Figma) + English, per surface, with the strings that change meaning. |
| `raw/*.txt` | Compact node dumps from Figma (layout, sizes, paddings, gaps, colors, typography, icons, copy). Start with `raw/00-shared-shell.txt`. |

## Frame index

| # | Screen | Component in code | Figma frame | Shell | Raw | Screenshot |
|---|---|---|---|---|---|---|
| D01 | Associar a um jogo do GameNode | `features/dialogs/MatchDialog.tsx` | `5369:2803` (820×474) | FloatingModal | `01` | `D01-match-dialog.png` |
| D02 | Adicionar ao GameNode | `features/dialogs/AddDialog.tsx` | `5369:2924` (520×492) | FloatingModal | `02` | `D02-add-dialog.png` |
| D03 | Adicionando N jogos (progresso) | `AddDialog.tsx` (progress mode) | `5369:3008` (520×346) | FloatingModal | `03` | `D03-add-batch-progress.png` |
| D04 | Conquistas | `features/library/AchievementListModal.tsx` | `5369:3062` (640×549) | FloatingModal | `04` | `D04-achievements.png` |
| D05 | Proteção local dos saves | `features/drm-free/SaveProtectionModal.tsx` | `5370:2923` (580×647) | FloatingModal | `05` | `D05-save-protection.png` |
| D06 | Ferramentas do jogo | `features/tools/GameToolsModal.tsx` | `5370:3062` (600×616) | FloatingModal | `06` | `D06-game-tools.png` |
| D07 | Configurações do jogo | `features/drm-free/LocalGameEditor.tsx` | `5370:3171` (640×605) | FloatingModal | `07` | `D07-local-game-editor.png` |
| D08 | Jogos descobertos | `features/drm-free/DiscoveryModal.tsx` | `5370:3276` (720×361) | FloatingModal | `08` | `D08-discovery.png` |
| D09 | Remover jogo | `features/drm-free/RemoveLocalGameModal.tsx` | `5371:3091` (460×377) | AppModal | `09` | `D09-remove-local-game.png` |
| D10 | Descartar alterações | `features/settings/DiscardChangesDialog.tsx` | `5371:3131` (400×278) | AppModal | `09` | `D10-discard-changes.png` |
| D11 | Conflito de monitoramento | `features/drm-free/ClaimConflictModal.tsx` | `5371:3161` (520×303) | AppModal | `09` | `D11-claim-conflict.png` |
| D12 | Tempo de jogo e sincronização | `features/shell/SyncExplanationModal.tsx` | `5371:3210` (480×370) | AppModal | `09` | `D12-sync-explanation.png` |
| D13 | Entrada em andamento | `features/auth/SignedOutView.tsx` (pending) | `5371:3254` (420×282) | AppModal | `09` | `D13-sign-in-pending.png` |
| D14 | Tela de entrada | `features/auth/SignedOutView.tsx` | `5371:3285` (1440×900) | full window | `10` | `D14-signed-out-view.png` |

Widths in the table are the **default** sizes. FloatingModal dialogs keep their saved geometry, so use these
as `defaultSize.width`; heights are content heights in Figma and become `defaultSize.height` rounded up
(see `components.md` › Work-surface shell).

## How to read `raw/*.txt`

Each line is one node, indented by depth.

- `F [name] WxH …` frame · `T "text" size/weight …` text · `I icon-name size:color` Tabler icon ·
  `C <Component Variant> WxH "texts" icons=…` instance of a shared component · `REC`/`ELL` shapes.
- Layout: `row`/`col`, `gapN`, `pT/R/B/L` (or `pN` when equal), `jc:` justify, `ai:` align (`ai:max` = end).
  Sizing pair: `FF` fill/fill, `FH` fill width/hug height, `HH`, `XH` fixed width (first letter = horizontal).
- Paint: `bg=` fill, `bdW=` stroke (or `bdT/R/B/L=` per side), `rN` radius, `sh(x,y,blur,spread,color)`, `opN` opacity.
- Colors are **Figma variable names** (see `tokens.md`) or raw hex when the designer used a literal.
- `accent/subtle@0.12`, `success-subtle@0.12` etc. are just the subtle tokens (the alpha is already in the
  token). Use `bg-accent-subtle`, `bg-success-subtle`; never add another opacity.
- Text: `13/600` = 13px weight 600 (Roboto), `lhN` line height px, `lsN` letter spacing px, `upper`, `ellipsis`.

## Rules that apply everywhere

1. Fixture names, numbers, dates and paths (`Hollow Mountain`, `84 MB`, `D:\Jogos\…`) are sample data. Render real data.
2. `NOVO` / `(NOVO)` in Figma is a **design annotation** marking a new feature. Never ship the word.
3. Gradient rectangles are placeholders for real images: game covers over `gamenode-asset://localhost/cover/<file>`
   (`<img loading="lazy" decoding="async">`), achievement icons, and the sign-in cover mural. The two overlay
   gradients on the sign-in art pane are real.
4. Tailwind classes backed by tokens; inline `style` only for runtime values (progress widths, virtualizer offsets,
   mural tile positions if computed).
5. Mantine v9 for `Switch`, `SegmentedControl`, `Checkbox`, `Menu`/`Select`-like dropdowns and `Notification`,
   restyled to these specs through the existing wrappers in `ui/src/components/ui/`.
   Use `FloatingModal` for D01–D08 and `AppModal` for D09–D13. Do not hand-roll layer stack, focus trap, Escape
   or focus restoration.
6. The `dots-vertical` icon at the left of every FloatingModal header is the drag affordance; the whole header
   stays the drag handle (existing `FloatingModal.Header` behavior).
7. "Usar sem conta" (D14) and its divider are **out of scope** (separate proposal). Do not render them.
8. Primary buttons render disabled at the existing `Button` disabled opacity (0.5). Figma's 0.45 is not a new token.
