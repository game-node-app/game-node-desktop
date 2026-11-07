# Design

## Context

See `proposal.md` for motivation and `specs/` for the behavior contract. The current constraints
that shape the implementation:

- The Steam provider scanner reads `libraryfolders.vdf` and `appmanifest_*.acf` only. The library
  machine treats `installed` as the single visibility fact: rows, provider counts, lookups, covers
  and playtime all iterate `installed_entries()`.
- `LibraryStore::save_scan` replaces the provider's installed set in one transaction. Entries are
  never deleted; an entry the scan no longer finds is marked uninstalled.
- The credential abstraction is a single redacted string (`SessionCredential`) with one keyring
  account, `session`, and one Linux fallback file, `private/session-token`.
- The app is Tauri v2 with a `main` window only; capabilities are scoped to `main`. Tauri 2.12 and
  wry 0.57 expose `WebviewWindow::cookies_for_url`, and a second webview can use its own data
  directory.
- Network adapters live in `crates/api`; blocking storage work runs on the engine's FIFO thread;
  HTTP work runs on Tokio; UI callbacks only dispatch and render.
- Steam's store and family endpoints are unofficial. The session cookie and store page layout can
  change without notice. GameNode must degrade to installed games, never break the library.

## Goals / Non-Goals

**Goals**

- Import the whole Steam account library without an API key.
- Keep the Steam credential out of SQLite, logs, snapshots and the main webview.
- Refresh the short-lived token silently while the stored session is valid.
- Represent installed, owned and family-shared games independently and keep entries stable.
- Make the installed filter fast and obvious without hiding the full library by default.
- Keep every failure recoverable and visible in Stores and, when relevant, in the library.

**Non-Goals**

- Steam API keys, multiple simultaneous Steam accounts, or store accounts other than Steam.
- Remote playtime (`playtime_forever`) and Steam Family playtime in the UI or in GameNode sync.
- Auto-adding imported games to the user's GameNode account library.
- Reading or controlling the Steam client; Play and Install stay URI-based.
- A general filter framework; the layout gains one `installedOnly` boolean with room to grow.

## Decisions

### 1. Sign in through an isolated in-app webview, not the system browser

The system browser cannot hand GameNode a Steam credential: Steam's OpenID redirect carries only
a SteamID, and the owned-games endpoints need the short-lived `webapi_token` embedded in an
authenticated store page. Reading the system browser's cookies is not possible or acceptable.

The app opens a second Tauri webview window at Steam's own login page. It has:

- a dedicated label, `steam-login`, and no capability entry (the capabilities file stays scoped to
  `main`), so the remote page has no application commands;
- a dedicated data directory under the 0700 private directory, removed after the attempt and on
  startup if a previous crash left it behind;
- a navigation allowlist for the Steam domains; non-Steam HTTPS links open in the system browser;
- a 10-minute lifetime, with Cancel and window-close both treated as cancellation;
- a poll that reads `steamLoginSecure` from the webview cookie store from Rust. No JavaScript is
  injected into Steam's page and no app IPC is exposed to it.

Alternatives: system browser + API key (excluded by the user), system browser + public profile
scraping (private libraries, exact ownership and rate limits make it unusable), and reimplementing
Steam's login form (would put passwords in GameNode). Playnite's Steam integration uses the same
isolated-webview model.

### 2. Store the long-lived cookie; derive the short-lived token on every sync

The keyring holds one string: the minimal Steam cookie header (`steamLoginSecure` plus `sessionid`
and any cookie the store needs to recognize the session). It does not hold the `webapi_token`, the
SteamID or the account name.

Every check/sync does:

1. `GET https://store.steampowered.com/` with the cookie.
2. Parse `#application_config` for `data-userinfo` (`logged_in`, `steamid`, `account_name`) and
   `data-store_user_config` (`webapi_token`).
3. Use `webapi_token` for the Steam Web API calls.
4. If Steam rotated `steamLoginSecure` in `Set-Cookie`, save the new header before finishing.

The token is valid for about a day but is fetched on every sync. This avoids token-expiry state and
means the only stored secret is the session cookie. A fallback to
`https://store.steampowered.com/pointssummary/ajaxgetasyncconfig` covers a store page whose
`application_config` cannot be parsed; the fallback lacks identity fields, so the SteamID is
decoded from the cookie and the account name is kept from the last successful parse.

Alternatives: storing the token (expires quickly, another secret), opening a hidden webview on
every sync (slow, more moving parts), or an API key (excluded).

### 3. A second credential slot, with the Linux private-file warning

`open_store` becomes slot-aware:

| Slot | Windows account | Linux Secret Service account | Linux fallback file |
| --- | --- | --- | --- |
| `session` | `session` | `session` | `private/session-token` |
| `steam` | `steam` | `steam` | `private/steam-token` |

The existing credential trait is reused. `SessionCredential` becomes an alias for a redacted
`Secret` string so the Steam cookie can use the same load/save/delete shape without gaining a
second Debug implementation that might leak. `open_store(paths, CredentialSlot)` selects the
keyring entry or file. The private-file store already enforces a 0700 directory, 0600 file,
no-follow semantics and no symlink; the Steam path inherits those checks.

On Linux with no Secret Service the app uses the same private-file fallback as the GameNode
session (the user's choice) but the Stores card says explicitly that the file is not encrypted and
recommends a keyring. A locked Secret Service and a failing Windows Credential Manager remain
errors; the app does not silently fall back to plaintext on either. A failed save after a
successful login leaves the account connected for this run and shows `persistFailed`.

### 4. Three independent facts per entry

```text
Entry key: <provider>:<store id>          (unchanged, e.g. steam:620)

installed     true when the installed scan found the app on this PC
owned         true when GetOwnedGames returned the app
familyShared  true when GetSharedLibraryApps returned the app
              and the app is not owned

visible       installed || owned || familyShared
installedOnly installed
```

- `catalog-only` entries have `install_dir = ""`.
- A successful phase replaces only its fact set. A failed phase leaves its previous facts.
- Owned wins over family-shared for labels; a family-shared entry that becomes owned loses the
  family flag on the next family merge.
- Sign-out and a detected account change clear owned and family flags. Entries, chosen/automatic
  matches, covers and playtime history are never deleted.
- `StoredEntry` and `Row` gain the two booleans. `ProviderCount` gains `ownedCount`,
  `familyCount` and `installedCount`; `count` remains the visible total.
- `LibraryView` gains `installedTotal` so the UI can render filter-aware counts without summing
  providers.

Why not a single `access` enum? The facts are independent: an installed game can be neither owned
nor shared (another account on the PC), a game can be owned and installed, and a shared game can
become owned. Independent booleans make the merge and the labels exact.

### 5. Catalog sync is separate from the installed scan

The scan run and the catalog sync have different networks, failure modes and cadences. Keep them
as separate effects/results:

```text
Effect::RunScan { op, provider }        -> Input::ProviderScanned { op, provider, result, now }
Effect::SyncCatalog { op, provider }    -> Input::CatalogSynced { op, provider, owned, family, now }
```

- `Effect::SyncCatalog` is emitted after a scan run for a provider with a catalog, when the
  account becomes connected, and on an explicit refresh. The engine decides whether a credential
  exists and whether the background scan should skip a recent sync.
- `CatalogSynced` carries `Option<Result<Vec<CatalogRecord>, CatalogError>>` for owned and family.
  `None` means "not attempted"; `Ok` replaces the set; `Err` leaves the set and records the
  categorized error.
- The core guards results with the `op`, so a result from before a sign-out or account change is
  ignored.
- The engine coalesces per provider: a request while one runs marks `again`, and one more runs
  afterwards. The core needs no pending queue.
- Local installed games remain available while Steam is offline or unauthenticated; the catalog
  sync never gates the installed scan.

The `SyncCatalog` effect is emitted even when Steam is disconnected; the engine returns
`NotConnected` without touching facts. This keeps the decision in one place.

### 6. Steam web client

Add `crates/api/src/steam.rs` behind the `SteamWebApi` port. It uses the existing reqwest client
configuration (rustls, system proxy, timeouts) with a realistic desktop Chrome User-Agent and a
20-second request timeout. It retries 429 and 5xx at most three times with backoff and honors a
short `Retry-After`.

**Store session**

```
GET https://store.steampowered.com/
Cookie: <stored header>
```

- Follow at most 10 redirects; a final URL under `/login` means `Expired`.
- Cap the body at 4 MiB and read it as UTF-8.
- Extract the `<div id="application_config">` tag, then its `data-userinfo` and
  `data-store_user_config` attributes. Decode HTML entities (`&quot;`, `&amp;`, numeric entities)
  before `serde_json`.
- `logged_in == false` or a missing/empty `webapi_token` means `Expired` or `Changed` as
  appropriate.
- Collect `Set-Cookie` values for `steamLoginSecure` and `sessionid` and return a rotated header if
  any changed.
- Fallback: `GET https://store.steampowered.com/pointssummary/ajaxgetasyncconfig` expects
  `{ success: 1, data: { webapi_token } }`; an empty `data` means expired.

**Owned games**

```
GET https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/
    ?access_token=<token>&steamid=<id>&format=json&language=english
    &include_appinfo=true&include_played_free_games=true
    &include_free_sub=false&skip_unvetted_apps=false
```

- Keep numeric app ids with a non-empty name; trim whitespace and strip `™`, `®`, `©`; dedupe by
  app id.
- `response` missing/empty while HTTP is 200 means Steam did not disclose the library (`Privacy`).
  `game_count == 0` with a `games` array is a successful empty catalog.
- 401/403 means `Expired`.

**Steam Family**

```
GET https://api.steampowered.com/IFamilyGroupsService/GetFamilyGroupForUser/v1/
    ?access_token=<token>&steamid=<id>
```

- `is_not_member_of_any_group == true` is `NoFamily`, a successful empty result.
- Otherwise take `family_groupid` and call:

```
GET https://api.steampowered.com/IFamilyGroupsService/GetSharedLibraryApps/v1/
    ?access_token=<token>&steamid=<id>&family_groupid=<id>&language=english
    &include_own=false&include_excluded=false&include_free=false
    &include_non_games=false&max_apps=10000
```

- Keep entries with a numeric app id, a non-empty name and `exclude_reason == 0`; skip excluded
  apps and log only the count. Dedupe by app id. `include_own=false` keeps owned games out, and
  the merge still treats `owned` as winning.
- The family request is optional: a failure records `FamilyError` and leaves the previous family
  facts; it never fails the owned catalog.

**Error taxonomy** (no secret in any variant or log):

```text
Unreachable | RateLimited | Server | Expired | Privacy | Changed | InvalidCookies
```

Logs name the endpoint path without the query string, the HTTP status, and the category. They
never log the cookie, the token, the HTML body or the full URL.

### 7. Account identity and change detection

A new `store_accounts` table keeps non-secret identity:

```sql
CREATE TABLE store_accounts (
    provider          TEXT PRIMARY KEY NOT NULL,
    account_id        TEXT NOT NULL,
    account_name      TEXT,
    last_verified_at  INTEGER
) STRICT;
```

- On startup, load the row with the credential so the card can show the identity while the check
  runs.
- After a successful session check, if `account_id` differs from the stored row, send
  `Input::CatalogCleared { provider }` before the new catalog result. The engine also updates the
  row. If the new catalog then fails, the previous account's games are not visible.
- The first sign-in has no previous row, so it simply writes the row and applies the catalog.
- Sign-out clears the row and sends `CatalogCleared`.
- The table contains no cookie or token; it is safe under `desktop-local-state`.

### 8. Library merge and persistence

`merge_catalog` mirrors `save_catalog`:

```text
owned  = Some(list):
  for every provider entry: owned = false
  upsert list:             owned = true, name = coalesce(local installed name, catalog name)
  record last_catalog_at and clear catalog_error

family = Some(list):
  for every provider entry: family_shared = false
  upsert list:             family_shared = !owned, name if empty
  record last_family_at and clear family_error

owned/family = Err:
  leave that fact set; record the error category
```

- An upsert never changes `installed`, `install_dir` or `first_seen`. The installed name wins for
  installed entries; otherwise the catalog name replaces an empty name.
- `save_scan` keeps its current behavior and does not touch `owned`/`family_shared`. The catalog
  upsert does not touch `installed`.
- `provider_scans` gains `last_catalog_at`, `catalog_error`, `last_family_at`, `family_error`.
  Errors are short categories (`unreachable`, `rateLimited`, `server`, `expired`, `privacy`,
  `changed`, `familyUnavailable`), not raw messages.
- `clear_catalog` sets both facts false for the provider, clears the four catalog columns and
  leaves entries/matches/covers.
- `load_cache` reads the new columns and degrades an unreadable boolean to `false`.

### 9. Filter, counts and empty states

- `LibraryLayout` gains `installed_only: bool` (serde default false) and `Input::SetInstalledOnly`.
- `visible_entries()` replaces `installed_entries()` for rows, counts, lookups, covers and playtime.
  `installed_entries()` stays for tracking/install-specific logic.
- `filter(rows, sidebar, installed_only, search)` applies provider AND installed AND search.
- Counts:
  - `All games` and each provider show `installedTotal` / `installedCount` while the filter is on,
    and `total` / `count` while it is off.
  - The provider row shows a muted `X installed` subline when the two differ and the filter is off.
- Empty states:
  - `NoGames { stores }` when no facts exist at all.
  - `NoInstalled { provider }` when the filter is on, `installedTotal == 0`, and `total > 0`; the
    UI offers "Show all games".
  - `NothingMatches` for search-only emptiness, with the existing clear-search action.
- Lookups: all visible entries are eligible, so owned-only games get matched. The existing
  24-hour freshness cache prevents re-asking.
- Covers: all visible matched entries are eligible. `plan_covers` queues installed ids first,
  then owned/family ids, preserving the 4-at-a-time queue and failure retry.
- Playtime: `update_playtime_view` iterates visible entries. Owned-only entries show the selected
  source with no total (unknown), never a fabricated zero. The existing local Steam counter
  continues to supply installed totals.
- Running/tracking stays installed-only.

### 10. UI states and placement

**Stores panel**

- One rich Steam card above the compact store list; DRM-free stays as its own section.
- The card shows a status pill with icon + text, account identity, `owned · shared · installed`
  counts when the library is open, last check, storage method, and the actions for the state.
- Action matrix:

| State | Primary | Secondary |
| --- | --- | --- |
| Not connected | Sign in with Steam | — |
| Checking | progress, no action | — |
| Signing in | Bring window to front | Cancel |
| Connected | Sync now | Sign out |
| Expired | Sign in again | Sign out |
| Unreachable | Try again | Sign out |
| Credential error | Retry | Sign in again / Remove saved sign-in |
| Persist failed (connected) | Sync now | Sign out; warning that restart will not restore |

- `privateFile` storage shows the plain-text warning in every state, including before sign-in.
- A family failure shows an inline, non-blocking warning in the card.
- The Stores topic gets an attention indicator for `expired`, `credentialError` and
  `persistFailed`; it is a text/icon indicator, not a color-only dot.
- Toasts: connected with imported count, signed out (installed stay), sync failure? Persistent
  failures stay in the card; toasts are for completed actions only.

**Library**

- Toolbar filter chip: a monitor icon + "Installed" + count, after search. It is a toggle, not a
  sort or a provider. Active styling uses the existing brand tokens and `aria-pressed`.
- Grid/list badges: installed entries show a monitor icon when the filter is off; family-shared
  entries show a people icon and an accessible label. Owned+shared entries show only the owned
  state.
- Detail pane: empty `install_dir` becomes "Not installed on this PC"; a family-shared entry shows
  a "Steam Family" label; Play/Install stay available for owned-only entries.
- Banners: `expired` shows "Steam sign-in expired. Showing your saved Steam library." with a
  "Fix in Settings" action; `unreachable` shows a connection banner with an action to Stores. Both
  are non-blocking and disappear on recovery.
- No layout reset on catalog updates: the view keeps filter, search, sort and the selection of
  entries that still exist; new entries join under the active sort.

### 11. Scan, sync and startup timeline

```text
app start
  |-- load library cache (installed + owned + family)      -> visible immediately
  |-- load Steam credential + store_accounts row            -> status Checking or NotConnected
  |-- installed scans (all providers)                       -> ids independent of Steam
  |-- if credential: validate store page, refresh catalog   -> catalog updates, status Connected
  |
window opened / manual rescan
  |-- installed scans
  |-- catalog sync (forced)
  |
background periodic scan
  |-- installed scans
  |-- catalog sync only if the last successful one is older than the throttle
  |
Stores opened
  |-- if last check older than 5 minutes: validate session and refresh catalog
  |-- otherwise show cached status
```

- Catalog throttle: 6 hours for background scans; startup, manual rescan and Stores checks bypass
  it.
- A catalog sync runs only while the library has an account; connecting Steam while signed out of
  GameNode stores the credential and sets `Connected`, and the first sync happens when the library
  opens.
- One session check/catalog sync at a time; repeated requests coalesce and share the result.

### 12. Security

- The cookie is a bearer credential. It is stored only in Credential Manager / Secret Service, or
  the 0600 Linux fallback with the warning. It never enters SQLite, settings JSON, snapshots,
  crash text, logs or the main webview.
- The sign-in window is remote content with no Tauri capabilities and no IPC. Cookies are read by
  Rust. Its profile is ephemeral and deleted after the attempt.
- HTTPS only, redirect host checks, body-size cap, no shell, no HTML rendering in the main window.
- The Steam short-lived token is passed as a query parameter to `api.steampowered.com`; URLs with
  query strings are never logged.
- Error types and `Debug` for credentials are redacted. Tests assert that no secret appears in a
  snapshot, an error string or the database.
- Sign-out deletes the credential; a failed deletion is reported and the account remains connected
  for the run. A late login-window result is ignored by an attempt generation counter.

## Risks / Trade-offs

- **Steam's store page or family endpoints change** → Parse only a narrow contract, keep a JSON
  fallback for the token, categorize failures as `Changed`, and never block installed games. A
  future change is a contained API-client update.
- **Steam blocks embedded webviews or shows a captcha** → The window uses the platform's real
  webview engine and a realistic profile. If a specific Linux engine is refused, the account can
  still be connected on Windows; the failure is visible and retryable, not a silent loop.
- **The session cookie is large or the keyring write fails** → Store only the minimal cookie
  header; a failed write keeps the session in memory for the run and reports `persistFailed`.
- **First sync is heavy for a 1,000-game library** → The installed entries and names appear
  immediately; lookups are batched at 50; covers use the existing four-at-a-time queue and
  installed covers are queued first. The UI stays virtualized and usable throughout.
- **Family endpoint is noisy or region-limited** → Request only included, non-free, non-excluded
  games with `include_non_games=false`; skip non-zero `exclude_reason`; a family failure never
  removes owned games or fails the scan.
- **Account switch leaks the previous catalog** → Persist only the non-secret SteamID; on a
  mismatch clear both fact sets before applying anything, and ignore results from the old attempt
  by `op`.
- **Offline startup collapses the library** → Owned/family facts are cached; `Unreachable` keeps
  them and the library banner explains the state.
- **Private-file fallback is plaintext** → The user explicitly chose the same fallback as the
  GameNode session; the Stores card names the risk and recommends a keyring. Windows never
  silently falls back.
- **Playtime for owned-only games is unknown** → This is deliberate; remote playtime is a later
  change. The UI shows unknown, not zero.

## Migration Plan

1. Add migration `0006_store_accounts_and_catalog.sql`:
   - `library_entries.owned INTEGER NOT NULL DEFAULT 0 CHECK (owned IN (0,1))`
   - `library_entries.family_shared INTEGER NOT NULL DEFAULT 0 CHECK (family_shared IN (0,1))`
   - `provider_scans.last_catalog_at`, `catalog_error`, `last_family_at`, `family_error`
   - `store_accounts` as above
   - bump `CURRENT_SCHEMA_VERSION` to 6
2. Existing entries keep `installed`; `owned`/`family_shared` start false, so the existing library
   looks identical until the first Steam sync.
3. Matches, chosen matches, covers, playtime and settings are untouched. A failed migration rolls
   back and startup fails safely as today.
4. The Steam credential slot is new; an existing installation has no Steam credential and starts
   `NotConnected`. The GameNode session slot and file are untouched.
5. Rollback: an older binary refuses schema 6 without modifying it, as the existing version guard
   requires. Removing the feature from a build leaves the extra columns/tables unused and the
   installed library intact.

## Testing Strategy

- **Core:** merge truth table (owned/installed/family combinations), failed phases, account
  clear, visible rows, filter composition, counts, empty states, lookup/cover eligibility and
  installed-first cover order, playtime view for owned-only entries, layout round-trip.
- **Platform:** migration from v5 with entries/matches/covers, new columns and table, both
  credential slots, private-file permissions/symlink refusal, separate delete, no secrets in the
  DB.
- **API:** httpmock fixtures for the logged-in/logged-out store page, entity decoding, rotated
  cookie, owned JSON (normal/empty/private), family group/no-group/shared/excluded, 401/429/5xx
  retries, malformed HTML/JSON, body cap, and assertion that no log/error contains the token.
- **Engine:** fake login window and fake Steam client; sign-in success/cancel/timeout, save
  failure, startup restore, rotated cookie, expired, unreachable, catalog coalescing, account
  switch clearing, sign-out, late callback, no UI blocking.
- **UI:** Stores card for every state, storage warning, attention indicator, filter chip and
  persisted state, counts, badges, detail pane for owned-only/family entries, banners, empty
  states, keyboard/focus, English and pt-BR strings.
- **Manual acceptance:** Windows Credential Manager and WebView2; Linux Secret Service and the
  private-file warning; real Steam Guard login; 100+ owned games; family group with shared games;
  restart restore; offline startup; expired cookie; sign-out and re-sign-in; two Steam accounts.
