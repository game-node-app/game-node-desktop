# Tasks

Prerequisite satisfied: `add-steam-library` is archived. Its artifacts are left unchanged.

## 1. Steam counter format

- [x] 1.1 Document the Steam counter key path, minutes-to-seconds conversion, Unix last-played time, account selection and re-read schedule (immediate, +1 minute, +5 minutes) in `docs/steam-playtime-counter.md`. Verify these rules with synthetic fixtures.

## 2. Dependencies and scaffolding

- [x] 2.1 Add the `Win32_System_Diagnostics_ToolHelp`, `Win32_System_Threading`, `Win32_System_ProcessStatus` and `Win32_Foundation` features to `gamenode-platform`'s `windows-sys`, and Slint's `system-tray` feature to `gamenode-app` in its Windows-only target dependencies. Verify that `cargo check --workspace --locked` passes on Linux and in the Windows cross-build, that `cargo tree -p gamenode-app` on Linux pulls in no tray backend, and that `Cargo.lock` is committed.
- [x] 2.2 Create the `core::tracker`, `core::playtime` (session model, submitter) and `core::sync` modules with module docs. Verify that `cargo clippy --workspace --all-targets --locked -- -D warnings` passes and that `cargo tree -p gamenode-core` still shows no OS, UI or network dependency.

## 3. Process port and Windows adapter

- [x] 3.1 Add the `Processes` port (`list`, `watch`, `ProcessInfo { pid, created_at, exe_path }`, `ProcessError::{Unsupported, Failed}`) to `core::ports`, plus a scripted test double that can start and end processes and reuse pids. Verify that unit tests drive the double through a start, an exit and a pid reuse.
- [x] 3.2 Implement `Processes` for Windows in `gamenode-platform`:
  - list pids with Toolhelp;
  - for new pids, call `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE)`, `QueryFullProcessImageNameW` and `GetProcessTimes`;
  - cache results by `(pid, created_at)` and skip processes that cannot be opened;
  - watch exits with `RegisterWaitForSingleObject` and report the exit time from `GetProcessTimes`.

  Verify with a Windows test that starts a child process from a temp folder, sees it in `list` with its full path, and gets one exit callback with an exit time after its creation time.
- [x] 3.3 Implement `Processes` for Linux so it returns `Unsupported`. Verify with a Linux unit test.

## 4. Tracker state machine

- [x] 4.1 Implement folder matching:
  - normalized paths;
  - case-insensitive on Windows and case-sensitive elsewhere;
  - a separator boundary, so `C:\Games\Foo` does not match `C:\Games\FooBar\x.exe`;
  - the deepest folder wins.

  Verify with table-driven tests for each rule, including nested install folders.
- [x] 4.2 Implement the tracker inputs (`Library`, `Listed`, `Exited`, `Tick`, `Stop`), the effects (`Watch`, `OpenSession`, `Checkpoint`, `CloseSession`, `ReadCounter`) and the running set:
  - several processes keep one game running until the last exits;
  - a session starts at the earliest process creation time and ends at the last exit time;
  - a process that disappears from a listing is ended at that listing;
  - identity is `(pid, created_at)`.

  Verify with tests for a helper process that outlives the main exe, an exit observed late, a pid reused by another program, and a game that was already running when tracking started.
- [x] 4.3 Add account ownership, checkpoints and the 24-hour split:
  - `Library { account: None }` and `Stop` close open sessions;
  - a different account never continues another account's session;
  - `Tick` emits a checkpoint at least every 60 s;
  - sessions split at exactly 24 h with no gap.

  Verify with tests for sign-out mid-session, an account switch, a 50-hour session (three sessions, contiguous) and an unverified cached account that still tracks.
- [x] 4.4 Add counter providers: the running state is reported but no sessions are opened, and an exit emits `ReadCounter` for the entry. Verify with a test that a counter-provider game produces a running state, no `OpenSession`, and one `ReadCounter` on exit.

## 5. Local storage

- [x] 5.1 Add migration `0003_playtime.sql` with `playtime_sessions` and `playtime_totals` as in design decision 3, registered in `MIGRATIONS`. Verify with a step 2 fixture upgrade that keeps settings, library entries and matches, and with the existing newer-schema refusal test.
- [x] 5.2 Add the `PlaytimeStore` port and its `platform` implementation:
  - open, checkpoint and close sessions;
  - close every open row at `checkpoint_at` on startup;
  - drop and log sessions with `ended_at <= started_at`;
  - read and write totals with their accepted and rejected values, per account.

  Verify with temp-database tests for a simulated crash (open row closed at its last checkpoint), a restart that keeps ids and times, and two accounts whose sessions and totals stay apart.
- [x] 5.3 Store `sync_enabled` and `sync_explained` in `settings`. A missing value means on and not dismissed; an unparsable `sync_enabled` means off and the explanation is due again. Verify with tests for each of the three cases.
- [x] 5.4 Update `docs/local-state.md` with the new tables, what they never contain (process lists, command lines), and crash recovery. Verify that the doc matches the migration's column names.

## 6. Steam counter

- [x] 6.1 Add the optional manifest field `playtime = { counter = "<name>" }` and a counter registry. An unknown counter fails that manifest only. Verify with a validation test next to the existing table-driven cases, and that a valid manifest still loads beside it.
- [x] 6.2 Implement `provider::steam::counter`:
  - read `userdata/*/config/localconfig.vdf` under the scan's canonical roots through `HostFiles`, using the existing KeyValues parser;
  - convert Playtime minutes to seconds and LastPlayed Unix seconds;
  - take the account with the latest `LastPlayed` per app;
  - skip a malformed account file;
  - log without file contents.

  Verify with synthetic fixture tests, including 754 minutes becoming 45240 s, two accounts, and one corrupt file.
- [x] 6.3 Add `playtime = { counter = "steam" }` to `providers/steam.toml` and document the field in `docs/providers.md`. Verify that the loader accepts the manifest and that the documented example parses in the existing doc test.


## 7. Submitting totals and sync

- [x] 7.1 Add the `PlaytimeApi` port and its `gamenode-api` implementation of `POST /v1/playtime` (`gameId`, `platformId: 6`, `source`, `totalPlaytimeSeconds`, `lastPlayedDate`). Verify with a mock-server test of the request body and of the 201, 401, 4xx and 5xx paths.
- [x] 7.2 Implement the pure `Submitter`:
  - the gate is sync on, the explanation dismissed, a verified session and a matched entry;
  - an entry is pending while `(game_id, total)` differs from what was accepted and from `rejected_seconds`;
  - requests go one at a time;
  - 2xx marks the total accepted, other 4xx marks it rejected, and a network error or 5xx stops the pass;
  - a match change sends the next submit to the new game.

  Verify with tests for each gate condition, an unchanged total that is not resent, a rejected total retried only after it changes, and sync turned back on after play.
- [x] 7.3 Implement the sync rule: while sync is off, no playtime request is made, while search, matching, covers and Add still work. Verify with an engine-level test that scans, matches and plays a game with sync off and observes no `PlaytimeApi` call but a stored total.

## 8. Engine wiring

- [x] 8.1 Wire the tracker into the `Engine`:
  - list processes every 5 s while an account's library is open, and stop after `Unsupported`;
  - deliver exits;
  - tick every 60 s;
  - feed library and account changes;
  - run session effects on the storage thread;
  - publish `TrackingView { available, running }`.

  Verify with an engine test using the process and store doubles that a game start shows as running and an exit closes the stored session.
- [x] 8.2 Wire counter reads: on start, after each scan, and on exit plus 1- and 5-minute delayed reads. Each read is followed by a submit pass, and submit passes also run on sync-on, on session verification and on match changes. Verify with an engine test whose counter double changes after exit and whose submit is observed once.
- [x] 8.3 Pass the running set to `LibraryMachine`, add `running` to rows and to `DetailView`, and add `Reason::Running`, which disables Play. Verify with library tests that Play is unavailable while running and returns when the game exits.
- [x] 8.4 Run a scan every 30 minutes through the existing coalescing, plus a scan whenever the window is shown. Verify with an engine test using a fake clock.
- [x] 8.5 Stop the tracker on sign-out, on session rejection and on Quit, and flush the storage thread (bounded) before the process exits. Verify with an engine test that Quit during a session leaves a closed row.

## 9. Background mode, tray and autostart

- [x] 9.1 Parse `--background`. Together with an existing instance it exits 0 without forwarding, and the software-renderer restart drops it. Verify with argument-parsing tests and a single-instance test where a second `--background` start sends nothing.
- [ ] 9.2 Show the window only for a normal start, `Activate` or a tray click, and switch to `run_event_loop_until_quit`. On Windows, `on_close_requested` saves geometry and layout and returns `HideWindow`; Linux keeps quit-on-close. App-menu Quit runs task 8.5 and then quits. Verify on Windows that closing hides the window while the process keeps running, and that Quit ends it; verify on Linux that closing still quits.
- [ ] 9.3 Add `ui/tray.slint` (`Tray inherits SystemTrayIcon`, with the Open and Quit menu items, the GameNode icon, and a tooltip bound to the running game) and create it on Windows only. Verify on Windows that the tray appears in background mode, that clicking it or choosing Open shows the window, that Quit exits, and that the tooltip names a running game.
- [ ] 9.4 Implement `platform::autostart` for Windows. On each start it writes HKCU `Run` value `GameNode Desktop` = `"<exe>" --background`, only when `.gamenode-desktop-install.json` sits next to the exe, and it never touches `StartupApproved`. Verify with a Windows test against a test key, a test that a `target/` build writes nothing, and a manual check that disabling GameNode in Startup apps survives a restart.
- [ ] 9.5 Remove the `Run` value in `packaging/windows/uninstall.ps1` and document it in `packaging/windows/README.md`. Verify that install, start and uninstall on Windows leave no `GameNode Desktop` value under `Run`.

## 10. Settings, explanation and strings

- [x] 10.1 Add the "Sync with GameNode" switch to Settings, applied immediately and persisted. Verify on both platforms that toggling it survives a restart and is reachable by keyboard with visible focus.
- [ ] 10.2 Add the first-run sync explanation overlay with "Keep sync on" and "Open settings". It also says that closing keeps GameNode in the tray. It appears after the first sign-in or at the next window show until dismissed, traps focus, and closes with Escape. Verify on Windows that a background-first start sends nothing until the overlay is dismissed in the window.
- [x] 10.3 Show the running state in the library grid, the list and the detail pane, and the "Running" reason on the disabled Play button. Nothing is shown when tracking is unavailable. Verify with a scripted running Steam entry and on Linux with the unsupported process adapter that no running indicator appears.
- [x] 10.4 Add every new string (tray menu and tooltip, Running, the sync switch and explanation, playtime errors) to the `en` and `pt_BR` catalogs. Verify that the existing catalog-completeness check passes and that the tray is in Portuguese under a `pt_BR` locale.

## 11. Verification

- [ ] 11.1 Extend `docs/windows-verification.md` with step 3 checks, then run Windows acceptance with synthetic Steam counters and a child process, and record the results:
  - install, reboot, and GameNode is in the tray without being opened;
  - a scanned Steam fixture entry started as an external child process shows as running in the library and the tray tooltip;
  - after the game exits, its playtime is submitted to a mock GameNode endpoint, and with Internal selected, a closed session is in the queue;
  - closing the window keeps tracking, and Quit stops it;
  - power off mid-game, start again, and the session is closed at its last checkpoint;
  - turn sync off, play, and nothing is sent; turn it on, and the total arrives.
- [x] 11.2 Extend `docs/linux-verification.md`. The app builds and runs as in step 2, closing quits, tracking reports itself unavailable, and nothing registers autostart. Mark it as needing a native Linux machine, since WSL cannot run it. Verify that the checklist names each item, with the run left to the Linux milestone.

## Workflow follow-up

- Archive this change after implementation and review.

## 12. Global playtime source

- [x] 12.1 Add a PC-wide persisted `playtime_source` preference, Store by default and Internal as the other choice. Missing or invalid values select Store. Verify persistence and fallback with database tests.
- [x] 12.2 Apply the preference to tracker and submitter behavior: Store uses supported provider counters, Internal measures sessions, and games without a counter always measure internally. Switching closes existing sessions and starts new internal sessions at the switch time; preserve separate histories without merging. Verify both choices, no-counter games and switching mid-game with core and engine tests.
- [x] 12.3 Add the global Store/Internal choice to Settings with immediate application, restart persistence, keyboard access and English/Portuguese translations. Explain that games without a supported store counter use Internal. Verify UI binding and settings tests.

## 13. Visible playtime

- [ ] 13.1 Add pure account-scoped display aggregation that selects store totals or internal sessions without merging, includes live session duration and distinguishes unknown store data from zero. Verify source switching, account isolation and live elapsed time.
- [ ] 13.2 Load display history on the storage worker and propagate refreshed playtime into library rows and details after counter/session/account changes. Verify imported totals, internal running time and source switching in engine tests.
- [ ] 13.3 Show translated playtime in grid, list and details, identify the source in details and verify formatted known/unknown values and UI bindings on Linux and Windows. Build an updated Windows package.
