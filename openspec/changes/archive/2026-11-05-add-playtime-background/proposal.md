# Proposal

## Why

After step 2 the library lists, matches and launches Steam games, but GameNode only knows about play that happens while its window is open, and it records none of it. Step 3 of `docs/mvp-roadmap.md` makes GameNode a background companion: it starts at login, notices any game running from a scanned install folder, and puts Steam playtime on the user's GameNode profile. This is the first tester build.

## What Changes

- **Tracker.** One generic tracker for every provider: a game is running while any process runs from inside its scanned `install_dir`. Starts are found by listing processes periodically (Toolhelp on Windows); exits are found by waiting on the process handle. The library shows "Running" on the entry and in the detail pane, and Play is disabled with a reason while the game runs.
- **Local session queue.** Every measured session is written to SQLite when it starts, checkpointed while it runs, closed at its last checkpoint after a crash or power loss, and split at 24 hours. Sessions belong to the signed-in GameNode account; signing out closes the open session. Measured sessions wait in the queue: nothing sends them until step 6 ships `POST /v1/playtime/sessions`.
- **Steam playtime counter.** Read Steam totals from `userdata/<account>/config/localconfig.vdf`, convert minutes to seconds and submit them through `POST /v1/playtime` (source `steam`, platform 6). Fixture tests cover fields, units, delayed writes and multiple account folders.
- **Visible playtime.** Grid, list and game details show the selected source's total. Details identify the source; internal time includes the current session. Unknown store totals show an unavailable value rather than zero.
- **Global playtime source.** Settings offers Store (default) or Internal. Store uses the provider counter where available; Internal records measured sessions. Games without a supported store counter always use Internal. Switching sources preserves separate histories and never adds imported totals to internal sessions.
- **Provider manifests** gain an optional `playtime = { counter = "<name>" }`. A provider with a counter uses it while Store is selected; Internal records measured sessions.
- **Background mode.** `--background` at login starts GameNode with tracking and a tray icon and no window. Autostart is a `Run` value under HKCU, on by default, rewritten each start with the current executable path. Opening GameNode shows the window. **BREAKING (step 1 behavior):** closing the window now returns to the background instead of quitting; Quit is in the tray menu and the app menu.
- **"Sync with GameNode" setting**, on by default and explained once after the first sign-in. Off means nothing leaves the PC except the lookups needed for matching, covers and Add to GameNode; Steam totals are kept locally and sent when it is turned back on.
- Background mode, tray, autostart and the tracker are Windows-only, as the roadmap's "Linux code during the MVP" rule says. On Linux the app keeps building and running as in step 2: closing the window still quits, and tracking reports itself unavailable.

Out of scope: sending measured sessions and the sessions endpoint (step 6), reporting detected games (step 6), any store besides Steam (step 4), a setting to turn autostart or background tracking off (Later), Linux process tracking, tray and autostart (Linux milestone).

## Capabilities

### New Capabilities

- `desktop-playtime`: Detecting running games from scanned install folders, the per-account local session queue with checkpoints and crash recovery, store counters and submitting their totals to GameNode, and which of the two each provider uses.
- `desktop-sync`: The "Sync with GameNode" setting, its first-run explanation, and the rule for what may leave the PC while it is off.

### Modified Capabilities

These capabilities come from `bootstrap-desktop-foundation` and `add-steam-library`. `add-steam-library` must be archived before this change, so `openspec/specs/` holds the requirements modified here.

- `desktop-lifecycle`: "Exit behavior for the foundation" is replaced. Closing the window returns to the background; the app starts in the background at login through HKCU `Run`, shows a tray icon with Open and Quit, and a second launch opens the window (a second `--background` launch does nothing).
- `desktop-providers`: Manifests accept an optional `playtime` counter, validated against the counters the app has. The Steam provider declares the Steam counter.
- `desktop-library`: Entries and the detail pane show whether the game is running, Play is disabled while it runs, and Play no longer has to keep the window open.
- `desktop-local-state`: The database also holds the per-account session queue, counter submission state and the sync setting.
- `desktop-shell`: Settings gains the "Sync with GameNode" switch, the app menu's Quit ends the process instead of closing the window, and tray and tracking strings are translated.

## Impact

- **Code:** `crates/core` (tracker state machine, session queue model, Steam counter reader, sync rules, manifest `playtime` field), `crates/platform` (process listing and exit waits behind a new `Processes` port, HKCU `Run` autostart, migration `0003`, session and counter repositories), `crates/api` (`POST /v1/playtime`), `crates/app` (background start, tray, close-to-background, tracker and submit wiring, Settings switch, first-run explanation, running state in the library UI), `providers/steam.toml`, `i18n/`, `docs/` (Steam counter format notes, verification), `packaging/windows/uninstall.ps1` (removes the `Run` value).
- **Dependencies:** Slint's `system-tray` feature (its built-in `SystemTrayIcon`, enabled for Windows only), and more `windows-sys` features (Toolhelp, process and wait APIs). No new crates.
- **Server API (existing, unchanged):** `POST /v1/playtime` with `{ gameId, platformId, source, totalPlaytimeSeconds, lastPlayedDate }`. No new endpoints.
- **Depends on:** `add-steam-library` finished and archived.
