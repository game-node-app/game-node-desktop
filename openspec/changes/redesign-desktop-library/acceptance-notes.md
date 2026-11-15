# Acceptance evidence — 2026-11-15

## Browser mock (21.2)

Reviewed the L1–L6 reference screens, component/token references and catalog
requirements against the running Vite mock at 1440×900 in English and pt-BR.
The additional 960×600 checks covered the list, filters and selection actions.
The mock uses fictional games and generated cover placeholders; content differs
from the reference games, while the component geometry, tokens and localized
copy are the comparison targets.

| Reference | Exercised and reviewed |
| --- | --- |
| L1 | Grid, selection, covers, sidebar, top/status bars and detail. The detail occupies x=1080, y=48, width=360, height=826 at 1440×900. |
| L2 | Store groups, three selected rows, multi-selection detail and batch actions in both locales. Header and rows share a horizontal scrollport with a 974 px surface; the final columns remain reachable with the detail open. |
| L3 | Three filter columns, counts, options, search, footer and Escape/focus behavior. The dropdown is 760×565 at 1440×900. At 960×600 it shrinks to available height, ends at y=592, and keeps Clear/Done visible while the body scrolls. |
| L4 | App, sort, group and context menus; command palette, family-member settings result, save-view dialog, rename and tooltips. Palette width=600, top=96; save dialog width=340. Save failure stops the spinner, keeps the dialog open and accepts retry. Rename from the default layout succeeds. |
| L5 | First-run, empty library, no installed games, no search results, scanning, discoveries, expired/unreachable account banners, hide notifications and shortcuts in both locales. Show all leaves the Installed scope. |
| L6 | Running DRM-free game with session start, arguments, profile and backups; unmatched suggestions; ambiguous suggestions and Use; chosen no-game state; family-shared uninstalled game. Direct Not a game changes Drift to noGame and removes suggestions without opening a dialog. |
| Catalog | Search, actual release year, library/local badges, detail and View/Show actions. Tiles do not invent a library match label or unknown playtime. |

The batch bar wraps when its actions cannot fit beside the detail, including
pt-BR and 960×600. The list scrolls horizontally rather than dropping columns.
These responsive behaviors preserve access to the actions and data.

Browser captures are retained in the T3 session artifacts. Representative pairs:

- L2: `browser-screenshot-localhost-mwi272nu-fdc87fb9.png` (en),
  `browser-screenshot-localhost-mwi235fk-e0303f3d.png` (pt-BR).
- Filters: `browser-screenshot-localhost-mwi23cd8-7632ff30.png` (1440×900),
  `browser-screenshot-localhost-mwi23bzb-6ba34071.png` (960×600, pt-BR).
- App menu: `browser-screenshot-localhost-mwi2xvk2-cbbdea20.png` (en),
  `browser-screenshot-localhost-mwi2bve3-af2c1f1d.png` (pt-BR).
- Shortcuts: `browser-screenshot-localhost-mwi28h09-58797bbe.png` (en),
  `browser-screenshot-localhost-mwi26rat-fd06f7ef.png` (pt-BR).
- Notifications: `browser-screenshot-localhost-mwi291nm-992246d1.png` (en),
  `browser-screenshot-localhost-mwi29zqn-ac1013dd.png` (pt-BR).
- Scanning: `browser-screenshot-localhost-mwi2anmh-8505917a.png` (en),
  `browser-screenshot-localhost-mwi2af7g-e17a59ff.png` (pt-BR).
- Catalog: `browser-screenshot-localhost-mwi29l7g-6f7dcdc5.png` (en),
  `browser-screenshot-localhost-mwi0w06t-d7540207.png` (pt-BR).
- Save failure/retry: `browser-screenshot-localhost-mwi24rvh-2fdfe062.png` (pt-BR).

## Integrated checks

All commands exited 0 on the final production-code state:

- `cargo test --locked -p gamenode-core -p gamenode-api -p gamenode-platform -p gamenode-app`:
  core 563 + 8 serialization, API 57, platform 326 + 4 integration, app 251 + 5 main tests.
- `pnpm --dir ui test --run`: 84 files, 836 tests.
- `pnpm --dir ui build`: includes `tsc -b`; successful. The existing main-chunk
  size warning remains.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `cargo fmt --all -- --check` and `git diff --check`.
- `openspec validate redesign-desktop-library --strict`.

## Native Windows (21.3)

The current release package was installed and launched with
`scripts/install-windows-wsl.sh`. The installed executable's SHA-256 matched
the package. Native checks and the remaining DPI checks are recorded in
`docs/windows-verification.md`. Task 21.3 remains unchecked until 100% and
125% DPI acceptance is performed; browser resizing does not verify OS DPI.
