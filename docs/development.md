# Native development

See [CONTRIBUTING.md](../CONTRIBUTING.md) for the commit-message convention used
by developers and AI agents.

Build from the repository root. `rust-toolchain.toml` pins Rust 1.99.1 with
rustfmt and Clippy. Install Rust using rustup, then let rustup install the toolchain
when invoking Cargo. Keep `Cargo.lock` and use `--locked`. The workspace has
eight packages: Core, platform, API, app, the public plugin protocol, the public
plugin SDK, the Steam plugin and the Linux package manager plugin. Core has no Tauri
or OS adapter dependency.
Protobuf generation uses the build-host compiler selected from the pinned
vendored dependency; a system `protoc` installation is not required.
Node.js (v24+) and pnpm are used for the `ui/` frontend workspace.

## Linux (Ubuntu/Debian, native x86-64)

Install native prerequisites:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config cmake ninja-build clang libclang-dev python3 git curl \
  libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
```

The desktop application is powered by Tauri v2 with WebKitGTK on Linux. Running
the graphical application requires a logged-in graphical session and display connection.
Secret Service uses the user D-Bus session and an installed service such as
GNOME Keyring or KWallet. No Secret Service activates the documented private-file
fallback; a locked service must not do so. SQLite is bundled and does not require
a separately installed SQLite development package. The HTTP client uses Rustls,
not OpenSSL.

## Windows (native x86-64 MSVC)

Install Visual Studio 2022 Build Tools with **Desktop development with C++**,
the x64 MSVC tools and Windows SDK. Install LLVM (including libclang), CMake,
Ninja, Python 3.11+, Git and rustup. Use an x64 Native Tools command prompt or a
PowerShell session with that developer environment initialized. Ensure these
tools are on PATH; if libclang cannot be located, set `LIBCLANG_PATH` to the LLVM
`bin` directory. Use the `x86_64-pc-windows-msvc` Rust host toolchain. Windows
Credential Manager is the credential backend and has no file fallback. Tauri v2
uses WebView2 on Windows.

Native Windows development with `scripts/run-desktop.ps1` requires Python 3.11+
on `PATH` (available as `py -3` or `python`). The launcher uses the standard-library
helper `scripts/bundled_plugins.py` to derive the bundled plugin executables from
`plugins/*.toml`, and fails with an actionable message when Python 3.11+ is missing.

Native Linux and Windows builds require their corresponding prerequisites above.
This change does not configure hosted CI runners; its verification commands are
run locally, and cross-compilation does not replace native OS acceptance.

## Required checks (both platforms)

```sh
pnpm --dir ui build
cargo metadata --no-deps --format-version 1
cargo check --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

To create the unsigned Linux per-user tar package from a native Linux build, run
`scripts/package-linux.sh`; it writes
`dist/linux/gamenode-desktop-<build-version>-<target>.tar.gz`, with a SHA-256
sidecar and a latest-build alias at `dist/linux/gamenode-desktop.tar.gz`.
The package build version is shared by the UI, `VERSION` and
`build-manifest.json`; it includes the workspace version, Git revision, dirty
state and UTC build time. Python 3.11+ is needed by the packaging script.
Extract it and run `./gamenode.sh` to
register the per-user XDG application and URI handler before launching. Use
`./install.sh` instead for a stable-path installation. The matching
`uninstall.sh` removes owned integration without touching application data.
See `packaging/linux/README.md` for experimental Arch + KDE support boundaries
and `docs/linux-tar-launcher-plan.md` for issue #2 acceptance checks.

Use temporary directories for tests; never use production credentials or local
application data. Unit/mock tests do not prove real-provider OAuth, native
installation, keyboard navigation, or graphical X11/Wayland behavior. Record
those separately using the change's acceptance tasks.

## Frontend Development and Preview Workflow (Vite & T3 Code Preview)

For day-to-day UI iteration, visual inspection, keyboard/mouse testing, and
AI-agent verification—including DRM-free and local save-protection screens—prefer
the Vite development server and interactive browser preview over cross-building
and reinstalling the Windows package on every edit.

The frontend is located in `ui/` and is built with React 19, `@tauri-apps/api`,
`@tanstack/react-query`, `@tanstack/react-virtual`, `@tanstack/react-store`,
Tailwind CSS 3.4, Mantine v9, and `@tabler/icons-react`.

### Architecture Guardrails

- **Async Tauri Commands**: Handlers implemented in Rust must be asynchronous
  `#[tauri::command]` functions that offload heavy computation or blocking I/O to
  worker threads, keeping IPC responsive.
- **Custom Asset Protocol**: Cover images and artwork are loaded via the custom
  `gamenode-asset://` protocol (`gamenode-asset://localhost/cover/<file>` and
  `gamenode-asset://localhost/artwork/<game_id>`). In React, render covers using
  `<img loading="lazy" decoding="async" />`.
- **Virtualized Views**: The game library grid and list views utilize `@tanstack/react-virtual`
  for high-performance DOM virtualization with large game collections.
- **State & Query Bridge**: Use `@tanstack/react-query` to bridge engine snapshot events
  and query cache updates, and `@tanstack/react-store` for client-side UI state.

### Styling with Tailwind CSS and Mantine

Tailwind handles layout and visual styling; Mantine supplies accessible overlay and
form primitives. Both use the Version 3.0 design tokens shared with
`game-node-clients`.

- `ui/tailwind.config.ts` defines the utility tokens: `brand`, `paper`, `secondary`,
  `tertiary`, `chrome`, `chrome-border`, `surface`, `raised`, `border-subtle`,
  `text-default`, `text-dimmed`, `danger`, `success` and `warning`, plus the
  project spacing, radii, font sizes and control dimensions.
- `ui/src/styles/theme.css` keeps the `@font-face`, the Version 3.0 CSS variables and
  the CSS layer order: Mantine styles first, Tailwind base inside the `tailwind`
  layer, and Tailwind utilities last so utilities win when applied.
- `ui/src/AppProviders.tsx` mounts `MantineProvider` with the dark `appTheme`, the
  brand palette and the GameNode variant color resolver. Render overlays inside that
  provider; standalone component tests need their own `MantineProvider` wrapper.
- Prefer utility classes for layout, colors, typography, borders, radii and
  `hover`/`active`/`focus-visible` states. Keep inline `style` only for runtime
  values such as virtualizer transforms and sizes, measured sidebar/detail-pane
  widths, pointer-anchored menu coordinates and computed status colors.
- Use `Menu`, `Notification`, `Switch` and `SegmentedControl` from Mantine for
  complex controls.
- Use the shared `FloatingModal` (`ui/src/components/FloatingModal`) for large work
  surfaces that benefit from dragging and resizing. It provides header dragging,
  bottom-right resizing, viewport clamping, keyboard resize, geometry persistence
  and focus trapping.
- Use the shared `AppModal` (`ui/src/components/AppModal`) for centered confirmation
  and notice dialogs.
- Both modal primitives use one layer stack: the last modal opened renders on top,
  only the topmost surface handles Escape, and closing restores focus to its trigger
  (`useFocusRestore`).
- Use `@tabler/icons-react` components instead of loose SVG assets.
- Keep `@tanstack/react-virtual` as the library virtualization engine. Do not wrap
  virtual tiles or rows in Mantine layout components; style their containers with
  Tailwind classes instead.

### Running the Vite Development Server

```sh
pnpm --dir ui dev
```

This starts the Vite development server on `http://localhost:4777` with instant HMR.

### Mock authenticated library for UI work

Run the frontend with the isolated mock host enabled:

```sh
pnpm --dir ui install --frozen-lockfile   # first time, or after dependency changes
VITE_UI_MOCK=1 pnpm --dir ui dev
```

Restart an already running Vite server with the variable set; the mode is selected
at startup. Open `http://localhost:4777/`. It starts with a fictional signed-in account and
12 fictional games, local cover images, and a verified library. Search, the filter popover
(availability, stores, favorites/running/show hidden, match state, GameNode status,
playtime, history, last session, release decades and date added), active-filter chips,
sorting and grouping menus, the grid/list views, selection and the batch bar, details,
favorites, matching and match suggestions, the add and undo flows, saved views, hidden
games, collection scopes, the GameNode catalog, the command palette, the shortcuts dialog,
the shell notifications and sign-out/sign-in update the in-memory snapshot. Reload to
reset it. Use `http://localhost:4777/?scenario=signed-out` to start signed out, add
`&library=large` for 328 owned Steam games (18 installed, a few family-shared), add
`&library=variants` for every match state and the DRM-free detail fixtures, and add
`&library=empty`, `&library=no-installed`, `&discoveries=1` or `&scanning=1` for the
state fixtures (`?scenario=first-run` starts the signed-in first-run guide). Add `&steam=expired` (or any store scenario) plus `&locale=pt-BR`
(or `?locale=pt-BR` on its own) for Portuguese.

Game tools opens from a game's context menu, **More actions** or the PC section of the
detail pane, and the mock keeps the real channel, version, proxy, candidate, anti-cheat,
install and uninstall interactions in memory. **Aster Vale** is running, so its tools
action is disabled. **Infinite Coast** (`steam:9`) detects Easy Anti-Cheat: the danger
callout shows, Install stays disabled until the confirmation is checked, and Browse…
selects `bin/x64/picked.exe` through the same selection command as the candidate list.
**Clockwork Harbor** (`steam:3`) is a clean game with no callout.

The **Settings** modal covers the redesigned topics and fixtures. **Stores** uses the
Steam account fixture; use `&steam=notConnected`, `&steam=signingIn`, `&steam=expired`,
`&steam=unreachable`, `&steam=credentialError`, `&steam=catalogExpired` or
`&steam=catalogUnreachable` to exercise account actions and progress. The last two
stay connected while the catalog reports the problem, so the same banner and
recovery path is reachable from `catalogError`. Add `&epic=expired` to start the Epic
card in the expired state while Steam stays connected; it offers **Sign in again**
and **Remove saved sign-in**. **Plugins** lists two official plugins plus the community
plugin **Prateleira Local**, which starts stopped so **Start**/sessions and the
origin filter counts are reachable. The inventory shows trusted origin, version,
executable/session status, exact granted capability IDs and native OS access
separately; it contains no credential values. **Tools** starts with three cached
OptiScaler archives (including a Nightly and one without a published digest) and the
tool installed in **Forgotten Atlas** and **Hollow Mountain**, so the OptiScaler card
counts two games and 0.7.9 starts marked **In use**. The mock snapshot carries Steam
account and PC ports state together, and the same locale parameter covers English
and Brazilian Portuguese. Sign-in, check, cancel, focus and sign-out use the
generic provider command interface.

**Settings fixture states** — every state below is reachable from `VITE_UI_MOCK=1`
without touching the native host:

| State | How to reach it |
|---|---|
| Expired store | `?epic=expired` (or `?steam=expired`) |
| Unavailable watched folder | `?watches=unavailable` seeds `D:\Jogos` and the unavailable `E:\Itch`; `?library=variants` also includes them |
| Pending backup changes | Settings › **Backups**, change the destination, the automatic switch or the copies kept; the changed dots and unsaved bar appear until **Save changes** or **Revert** (no fixture is pre-dirty) |
| Stopped community plugin | Settings › **Plugins**; **Prateleira Local** starts stopped. **Start** creates its session and **Stop** stops it again |
| Nightly cache | Settings › **Tools** › **Download cache**; 0.8.0-pre1 is the Nightly row and 0.7.9 is In use |
| Non-Windows mode | `?platform=linux` hides the General › Startup card, the Epic card and the Windows-only Tools/ports surfaces, and makes DRM-free games and Backups report that they are unavailable |

The mock replaces the Tauri command and snapshot-event bridge, including when
Vite is displayed inside a native dev window. Unsupported commands produce a
console warning and do nothing; they never fall through to the real host.
This is a UI fixture, not an HTTP server or a real authenticated API: it does
not exercise OAuth, remote endpoint contracts, native adapters, or persistence.
The native Rust host still starts normally if you run it, so use the browser for
an isolated UI session. The switch only works in Vite development mode; normal
production builds and ordinary `pnpm --dir ui dev` keep their existing behavior.

#### Library organization, catalog and failure fixtures

The redesigned library, sidebar, filter popover, batch bar, detail variants and
catalog view are mounted, so the fixtures below are reachable through the UI.
Run the mock as above. Everything stays in memory and a reload resets it.

Fixtures: 12 fictional local games plus the hidden **Hidden Harbor** entry, the
`Backlog curto` and `Coop no sofá` collections, the **Steam 10 h+** saved view,
three stores, the GameNode statuses plus matched and unmatched rows, one running
game, store-account scenarios and 40 fictional GameNode catalog games. The
comprehensive L6 fixture, discovered candidates, first-run, no-installed and
scanning states are opt-in as described below.

**Comprehensive L1–L6 fixture** — `?library=variants` (or `&library=variants`
after another parameter) adds the rows and local registrations below to the
default fixture; the default counts stay unchanged.

| Surface | Row key | Title | What it shows |
|---|---|---|---|
| Matched, installed | `steam:1` | Aster Vale | Play, achievements and GameNode status |
| Unmatched | `steam:3` | Clockwork Harbor | No-match notice plus two suggestions |
| Ambiguous | `steam:1301` | Neon Courier 2088 | Needs-review notice plus two suggestions |
| Marked as not a game | `steam:1302` | Wallpaper Foundry | Not-a-game note; no suggestions |
| DRM-free running | `manual:brasa` | Brasa | Running session timer, matched `ongoing`, launch arguments, save folder, active protection and 12 backups |
| DRM-free unmatched | `manual:drift` | Drift | Unmatched DRM-free row with two scored suggestions |
| Family shared, not installed | `steam:7` | Glass Garden | Install primary and the Steam Family note |
| Hidden | `steam:900` | Hidden Harbor | Hidden until **Show hidden** |

The fixture also keeps both collections, the saved view and the hidden entry,
adds the **DRM-free** provider shortcut with the two manual registrations, and
shows three discovered candidates (Signal Ridge, Paper Harbor and Tiny Comet) so
the banner, sidebar count and review modal are reachable without a separate
parameter. The unmatched keys accept **Not a game** from the match dialog and
**Use** from the suggestions list.

Use it to reach each L6 detail variant: select `manual:brasa` (DRM-free running),
`manual:drift` or `steam:3` (unmatched with suggestions), `steam:1301`
(ambiguous), `steam:1302` (not a game), `steam:7` (family shared) and three rows
with Ctrl/Shift (multi-selection). Add `&locale=pt-BR` for Portuguese.

**State and failure scenarios**

| Query | What it opens |
|---|---|
| `?scenario=first-run` | Signed-in first-run with no library, no connected store and no watched folder (first-run guide) |
| `?library=empty` | Signed-in empty library while a store stays connected (no-games state with Rescan) |
| `?library=empty&scanning=1` | The empty library while its initial rescan runs (centered scanning panel and skeletons) |
| `?scanning=1` | Starts a rescan on load over the normal rows (scanning panel above usable rows) |
| `?library=no-installed` | Owned and family-shared rows with nothing installed; opens on the **Installed** scope so **NoInstalled** and **Show all** are immediate |
| `?discoveries=1` | Adds the three watched-folder candidates to any scenario without changing the library rows |
| `?steam=expired` | Steam account state `expired` (Sign in again) |
| `?steam=catalogExpired` | Steam `connected` with `catalogError: expired` (same banner and action) |
| `?steam=catalogUnreachable` | Steam `connected` with `catalogError: unreachable` (Try now) |
| `?viewWrite=fail-once` | The first saved-view save or rename answers `failed`; the retry succeeds without reloading |
| `?viewWrite=fail` | Every saved-view storage write fails |
| `?mutations=fail` | Add, batch status and add-to-collection GameNode writes fail |
| `?catalog=error`, `?catalog=offline`, `?catalog=incomplete` | Catalog failure, offline and incomplete answers |
| `?sync=first` | Signed-in account whose first-sync explanation is pending; **Keep sync on** dismisses it, **Turn sync off** disables sync and then dismisses it |
| `?toolpick=cancel`, `?toolpick=fail`, `?toolpick=outside` | Browse… in Game tools keeps the current executable on cancel, or reports a picker or outside-folder error |

**L1–L6 and catalog checklist**

1. **L1 grid + detail** — the default fixture; click the first tile. Resize the
   detail pane and clear the search.
2. **L2 list grouped + multi-select** — Ctrl+L, Group menu › Store, then select
   three rows. `?library=variants` adds ambiguous, unmatched and DRM-free rows.
3. **L3 filters** — Ctrl+Shift+F (or Filters). Check a facet, search an option,
   and inspect the counts; apply a store, decade and history combination.
4. **L4 menus, palette, save view, tooltip** — Ctrl+K; the app menu (Add a game,
   Rescan, Settings, Keyboard shortcuts, About, Sign out, Quit); the tile/row
   context menu; hover an icon with a label. Change a filter, then **Save as
   view** (use `?viewWrite=fail-once` to see the failure and retry); press `?`
   for the shortcuts dialog.
5. **L5 states, banners, notifications, shortcuts** — open the states above in
   turn: `?scenario=first-run`, `?library=empty`, `?library=no-installed`,
   `?library=empty&scanning=1`. Expired and unreachable banners come from
   `?steam=expired`, `?steam=catalogExpired` or `?steam=catalogUnreachable`;
   dismissed banners stay dismissed until a successful sync. Recover with
   Settings › Stores › Check (or sign in again after an expiry); the banner
   disappears, and a later failure shows again. Discovered comes from
   `?discoveries=1` or `?library=variants`.
   Add a game from **Explore GameNode** to see Undo/View on GameNode, hide one
   to see Show again, and fail an add with `?mutations=fail` for its danger
   notification.
6. **L6 detail variants** — the keys in the table above; the catalog view is
   **Explore GameNode** in the sidebar (40 games; type at least two characters
   and wait for the 250 ms mock answer).

Add `&locale=pt-BR` (or start with `?locale=pt-BR`) to check every surface in
Portuguese. All of these fixtures still go through the mock's real view query,
library inputs and commands; they do not bypass the UI or invent host behavior.

**Saved views**

1. Under **Collections and views** in the sidebar, select **Steam 10 h+**. The
   library applies its Steam store filter, 10 h+ playtime filter, playtime sort,
   store grouping and grid view; the header names the view and the sidebar item
   is highlighted.
2. To save another combination, change a filter, the sort, the grouping or the
   view mode (or pick a scope other than All games), then choose **Save as view**
   in the filter chips row or the **+** button beside **Collections and views**.
   Enter a name and confirm. Empty and duplicate names are blocked, and a
   notification confirms the save.
3. Open the context menu on a saved view to rename or delete it. Delete asks for
   confirmation and keeps the current filters applied; changing a filter, sort or
   grouping after applying a view drops its highlight without changing the saved
   view.

**Hidden games and Show hidden**

1. **Hidden Harbor** (`steam:900`) starts hidden. To hide another entry, open its
   context menu (right-click, Shift+F10, or the tile/row menu) and choose
   **Hide from library**. The entry leaves the rows, counts, groups and Select
   all, and the notification offers **Show again**.
2. Open **Filters › Shortcuts** and turn on **Show hidden**. Hidden entries appear
   dimmed with an eye-off indicator, are included in the counts, and their context
   menu offers **Show in library**. Turn **Show hidden** off to hide them again;
   an entry revealed with **Show in library** stays visible.

**Collections**

1. Select **Backlog curto** or **Coop no sofá** under **Collections and views**.
   The library shows the local rows whose cached GameNode entry belongs to that
   collection and the header names the collection.
2. To add a game, open its context menu and choose **Add to collection ›
   Backlog curto** (or the other collection). Its status is preserved, a
   notification confirms the change, and the collection count updates. On a game
   that is not in the GameNode library the item is disabled with **Add to
   GameNode first**.

**Catalog (40 games)**

1. Choose **Explore GameNode** in the sidebar. In the catalog search, type at
   least two characters and pause; the view sends the query after 300 ms and the
   mock answers after another 250 ms. The fixture has 40 games, and its first 12
   titles match local games so results can show **In your library** and **On this
   PC** without a network.
2. Select a tile to open the catalog detail slot, or open its context menu.
   **Add to GameNode…** opens the normal add dialog in catalog mode; after the
   add the result shows **In your library**. **Show in library** switches back to
   the matched local row, clearing the search, scope, collapsed groups and filters
   as needed and revealing a hidden row with **Show hidden** instead of unhiding
   it.
3. Combine the scenarios below with `&locale=pt-BR` to check the translated
   states.

**GameNode status and mutation failures**

1. Start with `?mutations=fail`. Add, batch status and add-to-collection writes
   then answer failure instead of changing the fixture.
2. Select two or more games, use **Change status** in the batch bar (or the
   status select in one game's detail pane), and confirm the danger notification;
   the game keeps its previous status. The same flag makes an add from the library
   or catalog fail and leaves a catalog result that was not already in the library
   without **In your library**. A failed add creates no entry, so its notification
   has no **Undo**.

**Catalog failures** — `?catalog=error` fails every search, `?catalog=offline`
reports that a connection is needed without sending a request, and
`?catalog=incomplete` marks ready results as possibly incomplete.

**Locale and sign-out** — `?locale=pt-BR` (or `&locale=pt-BR` after another
parameter) translates the shell, library, menus, notifications and catalog.
`?scenario=signed-out` starts signed out; combine it with `&locale=pt-BR` as
needed. Signing out from the app menu also returns to the signed-out shell, and
signing back in restores the fixture library.

**Dialog fixtures**

The redesigned dialogs read the same snapshots and commands as the native host.
Use the **Details panel** toggle and these fixtures:

- **Match** — `?library=variants`, select **Neon Courier 2088**, then **Review…**.
  The ambiguous row opens three candidates at 96%, 74% and 38%; selecting one
  changes the preview and **Match** commits it.
- **Add to GameNode** — select a matched game and choose **Add to GameNode**. The
  dialog has the five statuses and collection counts. **New collection** creates a
  collection; the name **Collection limit** answers with the localized
  `collection_limit_reached` error and keeps the typed name.
- **Batch add** — select three games with Ctrl/Shift, choose **Add to GameNode**,
  then confirm. Each item takes 650 ms in the mock. **Hide** closes the dialog and
  the run finishes with the usual multi-add notification; signing out while it runs
  reports the remaining games in that notification without Undo or View on GameNode.
- **Achievements** — `?library=variants`, select **Aster Vale**, then **See all**.
  The fixture has 58 achievements with 31 unlocked and one hidden locked row; the
  sort menu offers most recent, rarest, most common and name.
- **Save protection** — `?library=variants`, select **Brasa**, then **Local save
  protection**. The history has four versions: two after-session copies (42 min
  and 1 h 12 min), one manual copy and one recovery copy. **Keep** changes the
  per-game retention among 5, 10, 20 and 50; space used and the copy count update
  in the mock.
- **Game settings and custom cover** — `?library=variants`, select a DRM-free
  game, then **Edit…**. **Change cover…** returns the bundled `mock-6.svg`
  sample; **Cancel** discards the pending cover and reopening shows the previous
  one.
- **Discovered games** — `?discoveries=1`, then **Review** in the banner. The
  three rows show a suggested match, no match, and a likely non-game
  (`vc_redist.x64.exe`) that starts unchecked. **Allow rediscovery in future
  scans** starts checked, and the header checkbox drives the **Add N games**
  count.
- **Tracking conflict** — `?claim=1` opens the conflict without a watched-folder
  scan; **Cancel**, **Use existing item** and **Move tracking** are reachable.
- **Sign-in and mural** — `?scenario=signed-out` shows 12 cached covers plus 13
  neutral mural tiles; below 860 px the art pane is hidden. Add
  `&signin=pending` so a provider click stays pending; `&reopen=fail` makes
  **Open again** report the localized reopen error without ending the attempt.

The first-use sync explanation is reachable with `?sync=first`: **Keep sync on**
leaves sync enabled and dismisses it, and **Turn sync off** disables sync before
dismissing it. **Game tools** opens from a game's context menu, **More actions** or
the PC section of the detail pane, as described above. These control paths document
the mock fixtures only. They are not a record of the browser visual comparison or of
native Windows acceptance.

The mock keeps its state in memory and never writes the database. In the native
host, migration `0010_library_organization.sql` adds the five per-account tables
`saved_views`, `hidden_entries`, `account_library_entries`, `account_collections`
and `account_library_reads`; the watched-folder check time is stored as
`discovery.lastCheckedAt`. See [local-state.md](local-state.md) for their columns,
replacement rules and retention.

#### PC ports mock

`VITE_UI_MOCK=1` also presents a fictional Windows signed-in build with a PC ports
catalog, so the `EXPLORE` destination is visible in a browser running on Linux or
macOS. `?platform=linux` hides it, matching a real Linux build, and
`?scenario=signed-out` hides it because the destination requires a session.

The catalog fixture has seven fictional entries: a guided recipe with a required
game folder and a console-package option, a plain release, a pre-release-only
entry, a withdrawn entry, an installed entry with an update available, an
installer-only entry with no Windows asset, and one entry whose install always
fails so the failure details can be checked. A package-capable slot opens on its
package choice in the guided setup. Picking a folder for Hollow Mountain's package
slot reproduces the "that folder contains game packages" rejection, and asking the
mock to install a base package without its required update returns the host's typed
missing-update error. A failed install from a previous session starts unseen, which
gives the sidebar badge a count of one. The catalog starts from a cached snapshot
marked stale, so opening PC ports shows the refreshing state and then the ready
state. `?ports=unavailable` starts with no snapshot and the unavailable state.

Installing an entry is simulated in memory with `ports:progress` events
(download, verify, extract, supplied files, finalize, register, complete). Cancel
a running install to see the cancelling and cancelled states. "Hollow Mountain:
Deep Delve" always fails with extractor output; the other installable entries
complete and gain an installed provenance row. Picking a parent folder, picking
supplied files, choosing an executable, marking results seen and remembering the
grid/list view and filters all round-trip through the mock host. Reload to reset.

### Running the native app

The Tauri host embeds the built UI (`ui/dist`) through the default `custom-protocol`
Cargo feature. The desktop resolves bundled providers beside its own executable and never searches
`PATH`; the Linux development launcher builds Steam and Linux package discovery plugins, while
Windows builds Steam only:

```sh
pnpm --dir ui build
scripts/run-desktop.sh
```

Without that feature Tauri treats the build as a dev build and loads `devUrl`
(`http://localhost:4777`), which shows a blank "Operation was cancelled" window when no Vite
server is running. For native HMR, start `pnpm --dir ui dev` and run
`scripts/run-desktop.sh --no-default-features`. Pass Cargo flags before `--`
and application arguments after it; for example,
`scripts/run-desktop.sh --release -- --background`. On native Windows PowerShell use
`scripts/run-desktop.ps1`; pass flags with `-CargoArguments @('--release')`
and app arguments with `-AppArguments @('--background')`. Both launchers check
that the platform's required plugins are present beside the desktop before running it. Plain
`cargo run -p gamenode-app` builds only the host and may leave a plugin missing or stale.

### Application logs

The desktop host writes a rotating log file under the data directory, alongside the database:

| OS | Log directory |
| --- | --- |
| Linux | `~/.local/share/gamenode-desktop/logs/` |
| Windows | `%LOCALAPPDATA%\GameNode\Desktop\logs\` |

One file per UTC day (`gamenode.YYYY-MM-DD.log`); the newest ten are kept. The default filter logs
`info` for GameNode crates and `warn` for third-party crates, and writes happen on a dedicated
worker thread. Set `GAMENODE_LOG` (or `RUST_LOG`) to change it for a run, for example:

```sh
GAMENODE_LOG="debug,gamenode_api=trace" scripts/run-desktop.sh
```

Settings → About shows the current path and offers **Show recent log** (with a copy button) and
**Open log folder**. Steam diagnostics log the request endpoint, HTTP status, and a failure
category such as `timeout` or `connect`; the cookie, the Web API token, and full URLs are never
written.

### Driving and previewing from T3 Code Preview

- **Browser / T3 Code Preview**: open `http://localhost:4777` via `preview_open`
  (`target: { kind: "environment-port", port: 4777 }`) to preview and interact with
  the UI directly.
- **Automated Frontend Tests**:
  ```sh
  pnpm --dir ui test --run
  ```
- **Building Frontend Assets**:
  ```sh
  pnpm --dir ui build
  ```

### Scope and limitations

Vite browser preview verifies React components, styling, responsive sizing, focus
scopes, keyboard navigation, and localization (`en` and `pt-BR`), but it **does not** exercise:
- Native OS window decorations, DPI scale factors from a physical monitor, or the native
  Windows system tray.
- Real OS adapters (`gamenode-platform`): Windows Credential Manager, Secret
  Service, native file dialogs, Windows process/parent tracking, directory-change
  notifications, registry access, or Ludusavi helper execution.

Use the WSL-to-Windows test and packaging routes below to verify those native
platform integrations.

## Scanner fixtures and running against a local Steam

Scanner tests run over an in-memory file system (`MemoryFiles`, `FixedRoots`), so they behave the
same on every OS. The files in `crates/core/src/provider/steam/fixtures/` follow the layout Steam
writes; ids, paths and sizes are made up:

```text
<steam root>/steamapps/libraryfolders.vdf        fixtures/libraryfolders_{linux,windows,old}.vdf
<steam root>/steamapps/appmanifest_<id>.acf      fixtures/appmanifest_portal2.acf, _malformed.acf
<steam root>/steamapps/common/<installdir>/      an empty folder (a manifest without it is skipped)
```

To add a case, put the new file in `fixtures/`, include it with `include_str!` in
`steam/tests.rs`, and build the file system with the `game(...)` helper there.

To run the app against a throwaway Steam, build the layout under a temporary home folder and start
the app with that `HOME` (Linux; the Steam roots are `~/.steam/steam` and
`~/.local/share/Steam`). The scratch home also keeps your own data, settings and credentials out of
the run: you sign in again inside it, and `XDG_DATA_HOME` and `XDG_CONFIG_HOME` must not point
elsewhere.

```sh
fixture=$(mktemp -d)
steam="$fixture/.steam/steam"
mkdir -p "$steam/steamapps/common/Portal 2"
sed "s#/home/me/.local/share/Steam#$steam#; s#/mnt/games/SteamLibrary#$fixture/Second#" \
  crates/core/src/provider/steam/fixtures/libraryfolders_linux.vdf > "$steam/steamapps/libraryfolders.vdf"
cp crates/core/src/provider/steam/fixtures/appmanifest_portal2.acf "$steam/steamapps/appmanifest_620.acf"
HOME="$fixture" XDG_DATA_HOME= XDG_CONFIG_HOME= cargo run --locked -p gamenode-app
```

The library lists Portal 2. Play and Install hand `steam://` links to the operating system; without
Steam installed the app shows that the Steam client could not be found. Rescan after editing the
fixture (add or delete a manifest) to see games appear and leave. The second library folder does not
exist, which shows the skipped-unplugged-drive case. Delete `$fixture` when done.

On Windows the root comes from `HKCU\Software\Valve\Steam` (`SteamPath`); the registry test in
`gamenode-platform` writes and removes a value under a test key, and a manual run can point
`SteamPath` at a folder laid out as above.

## Build and run Windows executables from WSL

WSL interoperability can launch the actual Windows binaries; a native Windows
Rust/Visual Studio installation is not required for this development route.
Install `cargo-xwin` (`cargo install cargo-xwin --locked`), add the Rust
`x86_64-pc-windows-msvc` target, and provide LLVM/Clang, lld-link, llvm-lib,
CMake, Ninja and NASM on PATH. `cargo-xwin` downloads the Microsoft CRT/SDK.
On Ubuntu these tools are available from clang/llvm/lld, cmake, ninja-build and
nasm packages. Tools may also be extracted into a user cache instead of being
installed system-wide; CMake's shared libraries must then be discoverable.

```sh
scripts/package-windows-wsl.sh
CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=/usr/bin/env \
  cargo xwin test --workspace --target x86_64-pc-windows-msvc --locked
powershell.exe -NoProfile -ExecutionPolicy Bypass -File \
  "$(wslpath -w "$PWD/dist/windows/gamenode-desktop/install.ps1")"
```

### Windows desktop with frontend hot reload

To run the current workspace in a Windows Tauri window without replacing the
installed application, quit GameNode completely (including its system-tray
process), then run from WSL:

```sh
scripts/dev-windows-wsl.sh
```

The launcher builds the Windows desktop and its bundled plugins under
`target/windows-hmr`, verifies the plugins sit beside the host, prepares its
backup assets, starts a local Vite server, and opens the Windows app. It forces the real Tauri bridge rather than the UI
mock. It uses the same Windows Credential Manager entry and local application
data as the installed app, so an existing session can be restored and changes
made during testing affect that data and account. If sign-in is needed, the
Windows `app.gamenode://` URI handler must be registered; the normal Windows
installation registers it and forwards the callback to the running dev app.

Keep the launcher running while editing React, CSS, or other Vite frontend
files; Vite updates the window. Rust and bundled provider changes require
quitting the app and running the launcher again to rebuild it. Quit via the
app menu or system tray to stop the development session and Vite; closing the
window can leave the app in the tray. The launcher refuses to start if another
GameNode process or a server on port 4777 is running. If interrupted with
Ctrl+C, quit any development app still running in the tray before restarting.
The normal installed executable and URI registration are not replaced.

The launcher needs the WSL build tools listed above, Node.js, pnpm, Python 3.11+,
`curl`, `flock`, and Windows interoperability (`powershell.exe` and
`wslpath`). The first build can take several minutes and preparing backup
assets can download the project's pinned inputs. Windows must be able to reach
WSL's `http://localhost:4777`; if the check fails, inspect WSL localhost
forwarding and Windows firewall settings.

For the full local workflow, run `scripts/install-windows-wsl.sh` from WSL. It
builds the locked release Windows MSVC executable, installs it for the current
Windows user, and launches the installed app. It requires WSL Windows interop plus
`powershell.exe` and `wslpath` on `PATH`.

The unsigned package installs below
`%LOCALAPPDATA%\Programs\GameNode Desktop`, creates a Start Menu shortcut and
registers a quoted URI command under HKCU. An existing URI handler is exported
as `previous-uri-handler.reg` and restored by uninstall when this installation
still owns the scheme. Reinstall to the same path after quitting the application.
To uninstall, invoke the installed `uninstall.ps1` with PowerShell's
`-NoProfile -ExecutionPolicy Bypass -File` arguments. The execution-policy
setting applies to that PowerShell process; it does not change machine policy.
Application data is preserved.

## Current verification

On 2026-11-04, 104 Linux tests and 94 actual Windows tests passed. The Linux
locked check, formatting and Clippy passed; the Windows MSVC-target cross-build
passed and the installed native app opened in pt-BR. See
`windows-verification.md` for the local installation evidence and limits.
This checkout has no configured Git remote. Clean-checkout native CI builds
and uploaded CI artifacts remain pending; WSL cross-compilation and execution
are recorded separately from native Windows compilation.
