# Design

## Context

This builds on the archived `add-steam-library` change. What step 3 relies on:

- `gamenode-core` keeps all logic pure: `AuthMachine` and `LibraryMachine` take inputs and return effects, and the app's `Engine` (`crates/app/src/engine/mod.rs`) executes them on Tokio or on the single FIFO storage thread, then publishes a `Snapshot`. OS access comes in through ports in `core/src/ports.rs` (`HostFiles`, `RootResolver`, `UriOpener`, …).
- Library entries carry the scanned `install_dir`, keyed `<provider>:<id>`, per account. `LibraryMachine` knows the open account (including a cached, unverified one) and each entry's match.
- The Steam scanner (`core/src/provider/steam`) has a KeyValues parser with case-insensitive lookups, and resolves Steam roots through `RootResolver`.
- `platform/src/db.rs` has ordered migrations (`0001_initial_settings`, `0002_library`) and a key/value `settings` table. Corrupt settings values already fall back to defaults.
- `main.rs` acquires the single-instance endpoint, builds `AppWindow`, shows it and runs the Slint event loop; `on_quit` calls `quit_event_loop`. Closing the window ends the process. IPC messages are `Activate` and `AuthCallback`.
- Packaging is `packaging/windows/install.ps1` / `uninstall.ps1`, per-user, writing `.gamenode-desktop-install.json` next to the installed exe.
- Slint 1.18.1 ships a `SystemTrayIcon` element (Windows backend: `Shell_NotifyIconW`) behind its `system-tray` feature, with `Menu` children, `icon`, `tooltip`, `visible` and `clicked`. A live tray keeps the event loop alive with no window.

Server contract (existing, unchanged), read from `game-node-server/src/playtime`:

| Call | Notes |
| --- | --- |
| `POST /v1/playtime` | Signed-in. Body `{ gameId, platformId, source, totalPlaytimeSeconds, lastPlayedDate? }`. `source` is the `UserPlaytimeSource` enum (`steam`, …). Overwrites the (user, game, source, platform) row and writes history when it changes. Returns 201. |

## Goals / Non-Goals

**Goals:** One tracker in `core` that every later provider uses unchanged; a session queue that step 6 can drain without a schema change; a Steam path that never conflicts with a web Steam connection; background mode that keeps the "window is a view" rule (closing the window hides a view, the engine keeps running).

**Non-Goals:** No draining of the queue, no device id, no tray or autostart on Linux, no off switch for autostart (Later), no tracking of games that are not in the scanned library.

## Decisions

### 1. Tracker as a pure state machine in `core`

New `core::tracker` module, in the style of `LibraryMachine`:

- **Inputs:** `Library { account, entries: [(key, install_dir, provider_mode, title)] }` (sent whenever the library or account changes; `None` account stops tracking), `Listed { at, processes: [ProcessInfo] }`, `Exited { process, at }`, `Tick { now }`, `Stop { at }` (sign-out, rejected session, Quit).
- **Effects:** `Watch(process)`, `OpenSession(row)`, `Checkpoint { id, at }`, `CloseSession { id, ended_at }`, `ReadCounter { key }`, and a `running` set the engine hands to `LibraryMachine` for the view.
- **Process identity** is `(pid, created_at)`, so a reused pid never continues an old game (spec: "Process identity is reused").
- **Matching paths:** install dirs and exe paths are normalized with the existing `provider::path::normalize`, compared case-insensitively on Windows, and a match requires the exe path to start with `install_dir` followed by a separator. The deepest matching dir wins.
- **Sessions:** a game opens a session when its first process appears, with `started_at` = the earliest `created_at` among its processes; it closes when its last process exits, with `ended_at` = that exit time. `Tick` every 60 s emits checkpoints and splits sessions at 24 h (close at `started_at + 24h`, open a new one at the same instant).
- **Counter providers** produce running state and `ReadCounter` on exit, never sessions.
- **Session ids** are UUID v4 strings from the existing `platform::random` source, passed in through the engine so `core` stays deterministic in tests.

Alternative: tracking only games launched from GameNode. Rejected by the roadmap: play started from Steam or a shortcut must count.

### 2. `Processes` port, Windows adapter in `platform`

```rust
pub trait Processes: Send + Sync {
    /// Every process whose executable path could be read. `Err(Unsupported)` off Windows.
    fn list(&self) -> Result<Vec<ProcessInfo>, ProcessError>;   // { pid, created_at, exe_path }
    /// Calls `on_exit(exit_time)` once, from any thread, when the process ends.
    fn watch(&self, process: &ProcessInfo, on_exit: Box<dyn FnOnce(SystemTime) + Send>) -> Result<(), ProcessError>;
}
```

Windows: `CreateToolhelp32Snapshot` + `Process32NextW` for pids; for each pid not seen in the previous listing, `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE)`, `QueryFullProcessImageNameW` and `GetProcessTimes`. Results are cached by `(pid, created_at)` so steady-state listings only open new pids. `watch` uses `RegisterWaitForSingleObject` on the kept handle (thread pool, no thread per game) and reads the real exit time with `GetProcessTimes`, so a delayed callback still records the true end. A process that cannot be opened is skipped; one that disappears from a listing without a wait firing is ended at the listing time (spec fallback).

The engine lists every 5 seconds while tracking is active. Linux returns `ProcessError::Unsupported` once; the engine then stops listing and the snapshot says tracking is unavailable.

Alternatives: WMI process-start events (heavier, COM, needs extra permissions), ETW (admin). A thread blocked in `WaitForSingleObject` per process is simpler but can mean dozens of threads for games with helper processes.

### 3. Local queue and totals: migration `0003_playtime`

```sql
CREATE TABLE playtime_sessions (
  id TEXT PRIMARY KEY, account_id TEXT NOT NULL, entry_key TEXT NOT NULL,
  game_id INTEGER, source TEXT NOT NULL, platform_id INTEGER NOT NULL,
  started_at INTEGER NOT NULL, checkpoint_at INTEGER NOT NULL, ended_at INTEGER  -- NULL while open
) STRICT;
CREATE TABLE playtime_totals (
  account_id TEXT NOT NULL, entry_key TEXT NOT NULL,
  total_seconds INTEGER NOT NULL, last_played_at INTEGER,
  accepted_game_id INTEGER, accepted_seconds INTEGER, rejected_seconds INTEGER,
  PRIMARY KEY (account_id, entry_key)
) STRICT;
```

Times are Unix seconds UTC. `game_id` is the entry's match when the session closes; step 6 resolves still-null ids at send time. The row shape matches the roadmap's session `{ session_id, game_id, source, platform_id, started_at, ended_at }`, so step 6 adds only a sender. On startup, before tracking starts, every row with `ended_at IS NULL` gets `ended_at = checkpoint_at` (all accounts: it is a local repair, not a view). A new `PlaytimeStore` port in `core` is implemented by `platform::playtime_db` and runs on the existing storage thread.

Sync settings go in the existing `settings` table as `sync_enabled` and `sync_explained`. Missing → on / not dismissed; unparsable `sync_enabled` → off (spec: an opt-out is never lost).

### 4. Steam counter and global source preference

Implement `provider::steam::counter` with the existing KeyValues parser and `HostFiles`.
Read `UserLocalConfigStore/Software/Valve/Steam/apps/<appid>/Playtime` (minutes,
converted with checked multiplication to seconds) and `LastPlayed` (Unix seconds).
Read `userdata/*/config/localconfig.vdf` under the canonical roots used by scanning.
Choose the account folder with the most recent `LastPlayed` per app, without summing accounts.
Fixtures cover malformed, missing and delayed data.
Missing, unreadable or invalid entries leave stored totals unchanged. Logs never include file contents.
Read on startup, after scans, on game exit and again at +1 and +5 minutes.
The Steam manifest declares `playtime = { counter = "steam" }`, validated against a counter registry.
Document the format and retry timing in `docs/steam-playtime-counter.md`.

Persist a PC-wide `playtime_source` setting (`store` or `internal`); missing or
invalid values use `store`. Store is the default and uses a counter where supported.
Internal disables store-total submissions and measures sessions for every provider.
Games without a supported counter always measure sessions, regardless of the preference.
Changing this setting takes effect immediately: close existing measured sessions at the
change time, recompute provider modes and re-evaluate pending totals. New internal
sessions begin at the switch time, excluding already imported play. Keep both histories
separate; never sum them or delete one when switching. Counter mode does not create
measured sessions. Internal sessions retain the existing local-queue delivery rules.

### 5. Submitting totals

A small pure `core::playtime::Submitter` decides what to send: a total is pending when Store is selected, sync is on, the explanation is dismissed, the session is verified, the entry has a match, and `(game_id, total)` differs from `(accepted_game_id, accepted_seconds)` and from `rejected_seconds`. It is re-evaluated on start, after scans and counter reads, on match changes, when sync turns on, and when the session becomes verified. Requests go out one at a time through a new `PlaytimeApi` port (`gamenode-api`: `POST /v1/playtime`, `platformId: 6`, `source` from the manifest, `lastPlayedDate` from the counter).

- 2xx → store `accepted_*`.
- 401 → the existing session-rejection path.
- Other 4xx → store `rejected_seconds` and log; that value is not retried, a changed total is.
- Network error or 5xx → stop the pass; retry at the next trigger.

If a match changes to another game, the next submit goes to the new game; the old game's row on the server is left as it is (removing it would need a delete endpoint, and step 6 decides reporting rules).

### 6. Background mode in the app

- **Arguments:** `--background` joins the existing internal flags. Background + an existing instance → exit 0 without forwarding. A normal launch still forwards `Activate`.
- **Window:** `AppWindow` is still built at startup, but only shown when not in background mode or on `Activate` / tray click. On Windows, `window().on_close_requested` saves geometry and layout and returns `HideWindow`; on Linux it keeps step 1's quit. Showing the window triggers the existing "window opened" scan and, when needed, the sync explanation.
- **Event loop:** `slint::run_event_loop_until_quit()`, so a hidden window does not end the process; the tray's keepalive covers the windowless state. Quit (app menu, tray) sends `Stop` to the tracker, waits for the storage thread to flush (bounded at 2 s), then calls `quit_event_loop`.
- **Tray:** a separate exported component `Tray inherits SystemTrayIcon` in `ui/tray.slint` with `Menu { MenuItem Open; MenuItem Quit }`, the GameNode icon and a `tooltip` bound from the snapshot ("GameNode — Playing <title>"). Slint's `system-tray` feature is enabled only in the Windows target dependency table, so Linux builds do not pull in `ksni`.
- **Renderer fallback:** the GPU check happens on first show. The software-restart path already drops internal flags; it also drops `--background`, because the restart happens when the user opened the window.
- **Periodic scan:** the engine requests a scan every 30 minutes, through the existing coalescing, so newly installed games become trackable without opening the window.

### 7. Autostart through HKCU `Run`

`platform::autostart` (Windows) writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `GameNode Desktop` = `"<current exe>" --background`, on every start. It never touches `...\Explorer\StartupApproved\Run`, where Windows records the user disabling an entry, so that choice survives rewrites. It only registers when the installer's `.gamenode-desktop-install.json` sits next to the executable, so development builds in `target/` never add themselves to login. `uninstall.ps1` deletes the value. Linux: not built (roadmap rule).

Alternative: a Startup-folder shortcut or Task Scheduler. `Run` is what the roadmap names, needs no COM, and appears in Windows' Startup apps list.

### 8. Snapshot and UI

`Snapshot` gains `tracking: TrackingView { available, running: [key], title }`, `sync: SyncPreferences { enabled, explained }`, `playtime_source` and a non-secret playtime error flag. The UI derives whether the explanation is due from `explained` and the signed-in state. `LibraryView` rows and `DetailView` gain `running`, and `actions::Reason` gains `Running` (Play disabled). Settings gets the sync switch and Store/Internal playtime source choice; the explanation is an overlay reusing the step 1 overlay pattern, with "Keep sync on" and "Open settings" actions. All new strings go through `@tr()` into both `.po` catalogs.

## Risks / Trade-offs

- [Games started elevated, or protected processes, cannot be opened with limited rights] → They are skipped and logged at debug level; documented as a known limit. Steam games rarely run elevated.
- [A game whose exe lives outside its install folder (some launchers, shared engines) is never seen as running] → Accepted for the MVP; the tracker reports nothing rather than guessing. Recorded for step 4's providers.
- [Steam's launcher or crash reporter processes inside the install folder extend a session] → They are part of the game folder by definition; the 24 h split and checkpoints bound the damage.
- [Steam may only write `localconfig.vdf` when Steam quits] → Covered by re-reading on start and on every scan; the re-read schedule handles delayed writes.
- [Closing the window no longer quits, which surprises step 1 testers] → The sync explanation also tells the user that GameNode keeps running in the tray; Quit stays in the app menu.
- [Autostart written on every start may fight a user who deletes the `Run` value] → Until the Later setting exists, Windows' Startup apps toggle is the supported way to disable it; this is documented in the explanation and in `docs/`.
- [Clock changes during a session] → Times come from the OS process times and the system clock; a session with `ended_at <= started_at` is dropped and logged rather than queued.

## Migration Plan

Migration `0003_playtime` adds tables and leaves step 2 data untouched. A step 2 build refuses the newer database, as the existing schema check already does; a rollback means reinstalling step 3 or deleting the database. Installing step 3 over step 2 with `install.ps1` is enough; autostart is registered on the first start. Uninstalling removes the `Run` value and keeps data as before.

## Open Questions

- The re-read schedule is immediate, +1 minute and +5 minutes. Only `Playtime` and `LastPlayed` are used.

## Visible playtime totals

`LibraryMachine` receives account-scoped playtime view data from the engine. Each
row carries an optional total in seconds and its effective Store/Internal source.
The engine loads totals and sessions on its FIFO storage thread when the account
opens, after counter writes and after session effects. Snapshots derive live
internal durations from cached open sessions and the current time; rendering never
reads SQLite. Switching sources chooses a separate history immediately. Account
changes clear cached display values before loading the new account. Store totals
remain unknown until read successfully, while loaded empty internal history is zero.
Show formatted hours/minutes (seconds for durations below one minute) in grid, list
and details; details identify the store or internal source. Translate all labels.
