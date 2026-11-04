# Proposal

## Why

After step 1 the signed-in user sees an empty library. Step 2 of `docs/mvp-roadmap.md` turns it into a working one: the user's installed Steam games, matched to GameNode, with covers, Play and Install, and a way to add a game to their GameNode library. It also builds the provider runtime that steps 4 and 5 reuse for every other store, so Steam has to fit the manifest model instead of being a special case.

## What Changes

- Add a provider runtime to `gamenode-core`: TOML manifests embedded from `providers/`, validation that skips and logs a bad manifest without stopping the others, a scanner registry, placeholder filling from scanner records, and launch/install rules that only open URIs whose scheme the manifest declares.
- Add the bundled Steam provider: `providers/steam.toml` plus a native scanner for `libraryfolders.vdf` and `appmanifest_*.acf`. It finds Steam on Windows (registry `SteamPath`) and on native and Flatpak Steam on Linux, ignores Steam's own runtimes and redistributables, and has fixture tests built from anonymized real files. Play opens `steam://rungameid/{id}`. Install opens `steam://install/{id}`.
- Match detected games through `GET /v1/game/external` (category 1, batched). Games without a match get a name search through `POST /v1/search/games`, prefilled with the detected title. The user can pick a match, or change or clear one, at any time. Unmatched games stay playable.
- Download covers from GameNode's image URLs and cache them on disk, so the library and its covers open without the network.
- Replace step 1's empty library with the library UI: grid and list views, a sidebar with provider filters, search, sort, keyboard navigation, multi-select, context menus, a detail pane with Play, Install, match and Add to GameNode, and a remembered layout.
- Add to GameNode: from the detail pane, the user picks a status (Playing, Finished, Planned, Dropped, Ongoing) and optionally some of their GameNode collections. The game is added with platform PC through `POST /v1/collections-entries`. A game that is already in their GameNode library shows its current status and is never overwritten. Detected games are never added automatically.
- Rescan when the app starts, when the window opens, and on demand.
- **Changes step 1 behavior:** when a saved session cannot be verified because GameNode is unreachable, the app now opens the cached library for the last signed-in account (Play works) instead of only showing the retry screen. Matching, search and Add to GameNode wait until the session is verified. A rejected session still returns to sign-in.

Out of scope: playtime and tracking, "running" state, tray and background mode (step 3); every store besides Steam, generic scanners, and launching executables (step 4 onward); reporting detected games or picks to the server (step 6); Proton and OptiScaler. No new server endpoints.

## Capabilities

### New Capabilities

- `desktop-providers`: Provider manifests and their validation, the scanner registry and plain scan records, placeholder filling, the launch and install rules, and the bundled Steam provider with its native scanner.
- `desktop-library`: The local library of detected games: when scans run, library keys, the grid and list views, filters, search, sort, selection, context menus, the detail pane, Play and Install, cover caching, remembered layout, and the offline cached library.
- `desktop-game-matching`: Linking detected games to GameNode games, automatically by store id where a category exists and by user pick otherwise, with re-matching and per-account storage of picks.
- `desktop-account-library`: Adding a detected, matched game to the user's GameNode library with a status and collections, without overwriting an existing entry.

### Modified Capabilities

These capabilities come from `bootstrap-desktop-foundation`, which is still open. It must be archived before this change, so that `openspec/specs/` holds the requirements modified here.

- `desktop-shell`: "Foundation shell and navigation" no longer requires an empty library with no game actions. The signed-in main view becomes the library.
- `desktop-auth`: "Restore and validate the session" now opens the cached library when GameNode is unreachable during restoration, and keeps network-only actions disabled until verification succeeds.
- `desktop-local-state`: Adds the per-account library cache (detected games, matches, cover files, layout). A record of the last signed-in account is allowed in local data, but credentials still are not.

## Impact

- **Code:** `crates/core` (provider runtime, Steam scanner, matching and library state, new ports for files, roots, URI opening and GameNode game APIs), `crates/platform` (filesystem and registry root adapters, URI opener, cover cache directory, migration `0002`), `crates/api` (external lookup, game search, collections, collection entries, cover download), `crates/app` (library views, detail pane, add dialog, layout persistence, scan triggers), new `providers/steam.toml`, new strings in `i18n/`.
- **Dependencies:** a TOML parser in `core`. A Steam KeyValues parser is written in `core` rather than added as a dependency. Windows registry reads use the existing `windows-sys` dependency in `platform`, with the Registry feature added.
- **Server API (existing, unchanged):** `GET /v1/game/external?category=1&uids=…`, `POST /v1/search/games`, `GET /v1/collections/library/:userId`, `GET /v1/collections-entries/game/:id`, `POST /v1/collections-entries`, and IGDB image URLs from the responses.
- **Data:** a new SQLite migration with tables keyed by account and `<provider>:<id>`, and a cover cache folder under the per-user data directory. Uninstall keeps both, as in step 1.
