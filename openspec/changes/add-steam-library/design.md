# Design

## Context

This builds on `bootstrap-desktop-foundation`, which is implemented but still open. What step 2 relies on:

- `gamenode-core` holds `AuthMachine`, `Snapshot`, `Settings` and the adapter traits in `ports.rs` (`SettingsStore`, `CredentialStore`, `AuthApi`, `BrowserOpener`). It has no OS, file, network or UI access. Its docs say game and provider interfaces arrive with their roadmap steps.
- `gamenode-platform` holds the SQLite repository with ordered, transactional migrations (`MIGRATIONS` in `db.rs`, a single `0001_initial_settings.sql` key/value `settings` table), data paths, credentials, IPC and browser opening.
- `gamenode-api` holds `GameNodeClient` (reqwest + rustls, 20 s timeout, no redirects), which only speaks the auth endpoints.
- `gamenode-app` holds the `Engine`. Commands come in from Slint, HTTP runs on Tokio, and blocking storage runs on one FIFO thread. Snapshots go out through a publish callback. The UI is `ui/app.slint` with an empty-library heading.

Server contracts, read from `game-node-server` (none change):

| Call | Notes |
| --- | --- |
| `GET /v1/game/external?category=1&uids=…&limit=…&offset=…` | Signed-in users only. Takes 1–100 `uids` as a repeated key. Paginated `{ data, pagination: { hasNextPage } }`, default `limit` 20. Rows are `GameExternalGame` with `uid`, `gameId` and `game.cover.url`. One `uid` can return several rows. |
| `POST /v1/search/games` | `{ query, limit, page }`. Items have `id`, `name`, `firstReleaseDate` and `coverUrl`. |
| `GET /v1/collections/library/:userId` | The user's collections, including private ones when the caller is the owner. Has `id`, `name` and `defaultEntryStatus`. |
| `GET /v1/collections-entries/game/:id` | The caller's entry for the game, or 404. |
| `POST /v1/collections-entries` | `{ collectionIds, gameId, platformIds, status, finishedAt }`. Overwrites an existing entry's status, platforms and collections. A collection's `defaultEntryStatus` overrides `status`. |

Cover URLs are IGDB URLs such as `//images.igdb.com/igdb/image/upload/t_thumb/<id>.jpg`. The web swaps the `t_<size>` segment to get the size it needs.

## Goals / Non-Goals

**Goals:** Build a provider runtime that later stores plug into with only a manifest or a scanner. Keep all library logic (scan merge, match precedence, filter, sort, selection) in `core`, where it can be tested without a UI or an OS. Make the library usable from local data alone.

**Non-Goals:** No generic scanners (`registry_subkeys`, `files`), no direct executable launches, no user manifests, no running-state or playtime fields, no reporting of detections or picks. No new server endpoints and no editing of existing GameNode library entries.

## Decisions

### 1. Provider runtime in `core`, file access through ports

New `core` modules:

- `provider::manifest` holds serde types with `deny_unknown_fields`.
- `provider::registry` loads and validates manifests and maps native scanner names to implementations.
- `provider::record` holds the plain record `{ id, name, install_dir, fields }`.
- `provider::template` parses and fills placeholders.
- `provider::steam` holds the KeyValues parser and the Steam scanner.

Scanners read the disk only through a new `HostFiles` port (`read_to_string`, `read_dir`, `is_dir`, `canonicalize`) and resolve roots through a `RootResolver` port (home expansion and, on Windows, `HKCU` string values). `platform` implements both with `std::fs`, plus `windows-sys` registry calls behind the `Win32_System_Registry` feature. Fixture tests use an in-memory `HostFiles`, so Windows-path fixtures run on Linux CI too.

Manifests are TOML strings. The app's `build.rs` lists `providers/*.toml` and generates `include_str!` entries, and the loader takes `(file name, text)` pairs. Core therefore never reaches outside its crate for files, and its tests can pass manifests inline. Validation collects one diagnostic per bad manifest and keeps going (see the `desktop-providers` spec).

Alternatives: putting scanners in `platform` would make every scanner platform code and lose cross-platform fixture tests. A `providers` crate would add a fifth crate that the roadmap layout doesn't have.

### 2. Manifest schema for step 2

Only the fields Steam uses, plus an `exclude_ids` list that any provider can use:

```toml
id = "steam"
name = "Steam"
platforms = ["windows", "linux"]
source = "steam"
scan = { native = "steam" }
launch = { uri = "steam://rungameid/{id}" }
install = { uri = "steam://install/{id}" }
match = { external_category = 1 }
exclude_ids = ["228980", "1070560", "1391110", "1628350", "1493710", ...]  # tools, runtimes, Proton

[[roots.windows]]
registry = 'HKCU\Software\Valve\Steam'
value = "SteamPath"

[[roots.linux]]
path = "~/.steam/steam"
[[roots.linux]]
path = "~/.local/share/Steam"
[[roots.linux]]
path = "~/.var/app/com.valvesoftware.Steam/.local/share/Steam"
```

The roadmap example's `playtime = { counter = "steam" }` is left out until step 3's spike decides it. `deny_unknown_fields` would reject it today, and step 3 adds the field together with its meaning. `source` is parsed and kept now because step 3 needs it, and it costs nothing.

Template rules: `{name}` placeholders only, names `[a-z_]+`. The template's scheme must be literal, so no placeholder may appear before `://`. Values are percent-encoded with the path-segment set, and any value containing control characters is rejected. At open time the scheme of the filled URI is checked again against the template's literal scheme.

### 3. Hand-written KeyValues parser and the Steam scanner

Steam's text KeyValues (`.vdf`/`.acf`) has quoted or bare tokens, `\\`-style escapes, nested `{}`, `//` comments and optional `[$COND]` tags. A parser of about 150 lines in `core` with table-driven tests is smaller to audit than a dependency. A malformed file fails as a whole and is skipped, as the spec requires.

Scan per root:

1. Canonicalize the root and skip it if it is missing or already scanned (this catches the `~/.steam/steam` link).
2. Read `steamapps/libraryfolders.vdf`, or `config/libraryfolders.vdf` if that is missing. Support the current format (`"0" { "path" "…" }`) and the old one (`"1" "D:\\Games"`). Always include the root itself.
3. For each library, read every `steamapps/appmanifest_*.acf` and take `AppState.appid` (must be numeric), `name` and `installdir`. The install dir is `<library>/steamapps/common/<installdir>`, kept only if it is a directory. The record fields are `id`, `name`, `install_dir` and `state_flags` (informational).
4. Drop `exclude_ids`.

On Windows `SteamPath` uses forward slashes, and the paths are normalized. A library folder that is gone (an unplugged drive) is skipped. Its games leave the view and keep their matches. Fixtures are anonymized real files: Windows with two libraries, native Linux, Flatpak, the old `libraryfolders` format, a malformed manifest, and a tool app.

### 4. Library state machine in `core`

`LibraryMachine` sits alongside `AuthMachine` and works the same way: inputs in, effects out, no I/O.

- **Inputs:** `Loaded(cache)`, `ScanRequested(reason)`, `ProviderScanned(provider, Result<records>)`, `LookupDone(category, results)`, `MatchChosen`, `CoverReady`, `AccountChanged(Option<account>)`, plus view commands (`SetView`, `SetFilter`, `SetSearch`, `SetSort`, `Select{key, mode}`, `Move(direction)`).
- **Effects:** `RunScan`, `PersistScan`, `Lookup{category, uids}`, `FetchCover{game_id, url}`, `PersistMatch`, `OpenUri`, `PersistLayout`.

One scan at a time, with a single `rescan_pending` flag. The merge is per provider: a successful scan replaces that provider's installed set by key, and a failed scan keeps the previous set and marks the provider failed. Entries are never deleted, so matches and covers survive uninstalls.

Filter, search (case-insensitive substring on detected and matched titles), sort (stable, with title as the tie-break) and selection (set plus anchor, so Shift-range and Ctrl-toggle work) are pure functions over the entry list and are tested directly. The UI sends commands and never filters on its own. This follows the roadmap rule that the window is a view.

The `Snapshot` gains a `library: Option<LibraryView>`, which holds visible rows, provider counts, scan status, selection, detail and network availability. To keep re-publishing cheap, rows are `Arc`-shared and the view carries a generation number, so the UI only rebuilds models when it changes.

### 5. Match precedence and storage

The effective match for an entry is the account's chosen match (a game, or "no game") if there is one. Otherwise it is the automatic match for `(category, store id)` if exactly one game is known. Otherwise the entry is unmatched, and any ambiguous candidates are kept for the dialog.

Automatic results are server facts, so they are cached per PC, keyed by `(category, uid)`. Chosen matches are cached per account, as the spec requires. Only successful lookups write automatic results: a found id is set, and an id the server doesn't return is set to "none". A failed request writes nothing.

Lookups batch 50 uids per request with `limit=100`, paging while `hasNextPage` is true, because one uid can return several rows. Rows are deduplicated by `gameId` per uid. Lookups run after every scan that adds ids, and for ids last checked more than 24 hours ago.

### 6. Schema `0002_library.sql`

- `library_entries(key PK, provider, store_id, name, install_dir, fields_json, installed, first_seen, last_seen)` holds PC-level detections.
- `provider_scans(provider PK, last_success_at, last_error)`.
- `auto_matches(category, uid, game_ids_json, checked_at, PK(category, uid))`.
- `chosen_matches(account_id, entry_key, game_id NULL, chosen_at, PK(account_id, entry_key))`, where `NULL` means "no GameNode game".
- `games(game_id PK, name, release_year, cover_url, cover_file, cover_failed_at)` caches GameNode game metadata for display.
- `settings` gains `library.layout` (JSON: view, sidebar selection, sort, pane sizes, detail open) and `account.last_id`/`account.last_name`. These hold no credentials. Sign-out deletes the account keys.

A separate `games` table lets the detail pane and covers work offline without repeating every lookup.

### 7. Covers

Allowed covers are `https://images.igdb.com/igdb/image/upload/t_<size>/<id>.jpg`, after normalizing `//` to `https:` and checking the host and path pattern. The size is rewritten to `t_cover_big`. Downloads go through `gamenode-api`'s client, with at most 4 at a time and a 5 MB response cap. Each cover is written atomically to `<data>/covers/<game_id>.jpg` and loaded with `slint::Image::load_from_path`, and the Slint JPEG decoder feature is enabled explicitly. A failed download records `cover_failed_at` and is retried on the next scan. Covers are keyed by game id, so changing a match simply points at another file. Unreferenced files are left alone in this step.

### 8. API additions and the Add to GameNode flow

A new `GameApi` port, implemented by `GameNodeClient` with the stored bearer credential:

- `lookup_external(category, uids)`
- `search_games(query)`
- `list_collections(user_id)`
- `library_entry(game_id) -> Option<status>` (404 means `None`)
- `create_entry(game_id, status, collection_ids)` (platform `[6]`, `finishedAt: null`)
- `fetch_cover(url)`

A 401 from any of these goes to the existing auth path, as a rejected session.

Add flow: when a matched entry is selected, membership is read. The dialog loads collections and shows a selected collection's `defaultEntryStatus` as the status that will apply. On confirm, the app reads membership again for each game, then creates the entry only if it is absent. Multi-add runs this per game, one at a time, and returns a summary.

Check-then-create is not atomic on the server. A web add in the gap between the check and the create could be overwritten. The window is under a second and the server has no create-only endpoint for clients, so this is accepted rather than adding server work to step 2.

### 9. Offline cached library through auth integration

When the `AuthMachine` reaches `SignedIn(user)`, the engine stores `account.last_*` and sends `AccountChanged(Some)`. At startup the engine reads `account.last_*` along with settings. If a credential exists, the library loads that account's cache immediately while auth is `Verifying` or `RestoreFailed`, with `network_available = false`. When auth succeeds:

- The same account enables network actions.
- A different account reloads the library for that account.
- A rejection clears the library and the last-account keys.

If there is no stored credential, the start is signed out and no library is shown. This keeps the step 1 rule that the library needs an account, without a network round trip before it appears.

### 10. Launch and URI opening

A new `UriOpener` port (in `platform`) opens a validated `Url` as one argument: `ShellExecuteW` through the existing `open` crate on Windows, `xdg-open <uri>` on Linux. `BrowserOpener` stays separate, so its use for http(s) sign-in URLs isn't widened. Core builds the URI from the template and passes it to the opener only after the scheme check. There is no exe path in this step.

### 11. Slint library UI

- Layout: top bar, then a horizontal split of sidebar, content and detail pane. The splitters are drag handles, since Slint has no splitter widget, and their sizes are saved in the layout.
- Grid: a `ListView` of row items, each holding N covers. Rust computes N from the content width reported by a `changed width` callback and rebuilds the row model only when N or the visible rows change. Slint's `ListView` virtualizes rows, which keeps big libraries usable without a custom grid.
- List: a `ListView` with columns for title, provider and match status.
- Keys: arrows move, Home/End jump, Enter opens details, Ctrl+Enter plays, Ctrl/Shift extend the selection, Ctrl+F focuses search, F5 rescans, and Menu or Shift+F10 opens the context menu. `ContextMenuArea` provides the menus. All key handling is forwarded to core as `Move`, `Select` and similar commands.
- Dialogs (Match, Add to GameNode) are in-window overlays with focus trapping and Escape to cancel, reusing step 1's overlay pattern.
- New strings go into the existing `.po` catalogs.

### 12. Scan scheduling

Scans are triggered at startup after the library cache loads, on window show (including activation from a second launch), and by Rescan. Scanning runs in `spawn_blocking`. Persistence goes through the existing FIFO storage worker, so DB writes stay ordered with settings and match writes. Lookups and cover downloads run on Tokio after persistence and only while the session is verified.

## Risks / Trade-offs

- [Steam's file formats change or vary by version] → Accept both `libraryfolders` formats, fail per file, and grow fixtures from real installs on each OS during acceptance.
- [The tool exclusion list goes stale as Valve ships new runtimes or Proton builds] → It is data in `providers/steam.toml`. Acceptance on a Linux PC with Proton installed checks that none show up.
- [Some Steam ids are unmapped or ambiguous on GameNode] → The entry stays playable, the dialog shows candidates and search, and the pick sticks.
- [The check-then-create race when adding to GameNode] → Re-check right before creating. A server create-only option can follow if it matters (step 6 touches the server anyway).
- [Large libraries in Slint] → Row-virtualized `ListView`, generation-gated model rebuilds, and an `Arc`-shared snapshot.
- [IGDB image host or URL format changes] → The allow-list is in one function. A non-matching URL falls back to the placeholder instead of failing.
- [Showing a cached library before verification for a session that turns out revoked] → Only Play and Install, which are local actions, work until verification. A rejection hides the library immediately.

## Migration Plan

Archive `bootstrap-desktop-foundation` first, so that the modified requirements exist in `openspec/specs/`. Migration `0002` adds the tables and settings keys to a step 1 database and keeps its settings. That is tested with a v1 fixture. A step 1 binary refuses the v2 database under its newer-schema rule and leaves it untouched, so downgrading needs a reinstall of this version, not data deletion. The cover folder is created on first use. Uninstall keeps it with the rest of the user data, as in step 1.

## Open Questions

- The exact Steam tool and runtime app id list. It is collected during implementation from Valve's published runtime and Proton ids and checked on a real Linux install. It does not change the spec or the tasks.
- Whether `t_cover_big_2x` looks noticeably better on HiDPI displays. This is a one-line size choice made during UI review.
