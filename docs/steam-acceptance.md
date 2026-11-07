# Steam web login and owned library — verification and acceptance

Change: `openspec/changes/add-steam-web-login-and-owned-library`.

This file records what the implementation was verified against in the development container and
what still needs a real Steam account, Windows, or a Linux desktop session. The four manual
acceptance tasks in `tasks.md` (7.3–7.6) remain open until those checks are run; none of the
results below should be read as a substitute for them.

## Automated verification

| Area | Command | Result |
| --- | --- | --- |
| Core | `cargo test --locked -p gamenode-core` | 339 unit + 7 integration tests passed |
| Platform | `cargo test --locked -p gamenode-platform` | 134 unit + 4 integration tests passed |
| Steam API client | `cargo test --locked -p gamenode-api` | 63 tests passed (httpmock) |
| App | `cargo test --locked -p gamenode-app` | 132 lib + 5 bin tests passed |
| Whole workspace | `cargo test --locked --workspace` | passed |
| Rust lints | `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| Formatting | `cargo fmt --all -- --check` | clean |
| UI | `pnpm --dir ui test --run` | 24 files, 223 tests passed |
| UI production build | `pnpm --dir ui build` | passed (existing chunk-size warning only) |
| OpenSpec | `openspec validate add-steam-web-login-and-owned-library --strict` | valid |

The previously flaky engine test `a_failed_credential_save_leaves_the_account_connected_for_this_run`
was traced to a concurrent startup check moving the live status to `Checking` before the sign-in
outcome was read; the engine skipped the first catalog import. The engine now trusts the sign-in
result and queues the import across library loading. The test passed 150 consecutive runs after the
fix.

## Automated coverage highlights

- Core: installed/owned/family truth table, owned-wins, failed phases, account clearing, visible
  rows/counts, installed filter and `NoInstalled`, owned-only lookup/cover/playtime behavior, old
  layout data, catalog error categories and op guards.
- Platform: v5→v6 migration with entries/matches/covers/playtime preserved, catalog replacement and
  failure behavior, `store_accounts`, unreadable boolean degradation, no secrets in SQLite, separate
  credential slots, Linux private-file permissions/symlink refusal.
- API: logged-in/logged-out store pages, `#application_config` parsing and entity decoding, rotated
  cookies, fallback token endpoint, owned library variants, Steam Family group/no-group/excluded,
  bounded 429/5xx retries, 4 MiB body cap, secret-free errors/logs.
- App: startup restore, sign-in success/cancel/timeout, rotated cookie, expired, unreachable,
  failed credential save, account switch (including failed new catalog), sign-out, superseded
  results, catalog coalescing, snapshot `storeAccounts` for every state with no secret.
- UI: Steam card for every state/action, private-file warning before sign-in and while connected,
  Stores attention indicator, connected/signed-out toasts (import count waits for the catalog sync),
  stale-session check when Stores opens, Installed filter, sidebar counts, grid/list/detail
  indicators, expired/unreachable banners opening Settings > Stores, all empty states, large mock
  library.

## Visual smoke check

With `VITE_UI_MOCK=1`, `?library=large&steam=expired` was checked in the preview browser:

- 328 Steam entries, 18 installed, sidebar “All games 328” and “Steam 328 / 18 installed”.
- Installed chip toggled the view to 18 entries and the counts followed the filter.
- Family-shared card showed the family icon; its detail pane showed “Steam Family” and “Not
  installed on this PC”.
- The expired banner opened Settings > Stores on the Steam card (320 owned / 8 shared / 18
  installed, Secret Service storage, attention indicator).

## Manual acceptance still required

| Task | Status | Why it cannot run in this container |
| --- | --- | --- |
| 7.3 Windows | **not run** | No Windows host, WebView2, Credential Manager or real Steam Guard account. The Windows-specific code compiles, but the real sign-in/import/restart/Play/Install pass needs a Windows machine. |
| 7.4 Linux | **partially verified / not run** | WebKitGTK login, Steam Guard and a real Secret Service session need a desktop session and the user's Steam account. Automated tests cover the private-file fallback permissions, symlink refusal, offline startup, expired/unreachable recovery and the Installed filter; a real login was not attempted. |
| 7.5 Steam Family | **not run** | Needs a real Steam Family group. httpmock covers group/no-group/shared/excluded and owned-wins; leaving a group and real shared-game revocation were not exercised. |
| 7.6 300+ first sync | **partially verified / not run** | The UI mock exercised 320 owned/18 installed with virtualization, the Installed filter and detail pane. A real 300+ account sync, installed-first cover downloads and restart-before-network behavior need the real account. |

Suggested manual pass (when available):

1. Windows: sign in with Steam Guard, import a real library, verify the three counts, toggle the
   Installed filter, Play/Install an uninstalled game, restart, sign out and sign in again.
2. Linux: repeat the sign-in with WebKitGTK; verify Secret Service storage, then the 0600
   private-file fallback and its warning when no Secret Service exists; restart offline and recover
   an expired session.
3. Family: verify shared games, owned-wins labeling and that leaving the group removes only the
   family fact while installed entries stay.
4. Large library: time the first sync, confirm the UI stays interactive, covers queue installed
   games first, and a restart shows the cached catalog before the network finishes.

Record the four results in the change notes before archiving.
