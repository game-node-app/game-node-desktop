# Design

## Context

See `proposal.md` for the motivation. This section covers only the constraints that shape the approach.

- **Shell.** The signed-in shell has one main view, the library. The sidebar (`ui/src/features/shell/Sidebar.tsx`) mixes navigation items (`All games`, providers) that set the library filter with DRM-free actions. No second top-level destination exists. An earlier change, `add-downloads-and-rqbit-engine`, planned a "Downloads" section for consent-gated BitTorrent transfers. It was removed in `208d9945` without shipping, but the name is still associated with that feature.
- **Building blocks to reuse.** Several pieces already exist:
  - `Registration` (`crates/core/src/drm_free.rs`) already carries `launchers`, `gameplay`, `arguments` and `root`. A port install can produce one directly.
  - The game-tools pipeline (`crates/platform/src/tools/`) already downloads GitHub releases over HTTPS, verifies their SHA-256, caches archives, detects 7z and zip archives and validates archive entry paths.
  - `manifest_cache.rs` already fetches a remote document with an ETag and parses it in a worker.
  - The `gamenode-asset://` protocol already serves cached artwork.
- **Upstream.** The Quiver Catalog API (`https://api.quiverlauncher.com/api/v1`) is public, unauthenticated and read-only, and it publishes an OpenAPI 1.1.0 description.
  - The `/apps` listing returns `catalogId`, `launcher` (setup files, preferred executables, files to add, asset filter, folder name), `verified`, `libraryArt`, `consoles` and `supportedOS` for each entry. It pages through results with `limit` (at most 100) and `nextCursor`.
  - `/apps/{slug}` adds `withdrawn` and `checking`, and `/apps/{slug}/releases` lists assets with a `checksum`, an `unverifiedChecksum`, `os`, `architecture` and `format`.
  - The listing has no licence field.
  - Quiver's packages are "All rights reserved", and its trademark policy allows truthful text references but not its logo.
- **Platform.** Windows comes first. Linux builds exist, but port installs on Linux are deferred.

## Goals / Non-Goals

**Goals:**

- Give the catalog a permanent home in the shell that scales to more than 1,000 entries and reuses the library's layout, tiles, toolbar and detail pane.
- Keep the upstream optional at runtime. If the API changes, blocks access or disappears, GameNode keeps working from its cache and overlay, and installed ports are unaffected.
- Make one install engine serve all three kinds of entry: release only, guided setup with supplied files, and guided setup that also extracts a console package.
- Make every install transactional: it either ends as a registered entry or leaves nothing behind, including after a crash.

**Non-Goals:**

- Mirroring or re-hosting the upstream catalog, art or releases.
- Generic installer execution (`.msi` or setup `.exe` files). Entries whose only Windows asset is an installer are listed, but Install is unavailable for them, with a reason.
- Updating ports after install, managing mods, or verifying files after install.

## Decisions

### D1. Placement: a sidebar `EXPLORE` group with a **PC ports** destination

The sidebar gets a second group header, `EXPLORE`, under `YOUR SPACE`, with one item, **PC ports** (pt-BR: "Ports para PC"). Selecting it swaps the content area, so the toolbar, grid or list and detail pane show the catalog instead of the library. `All games` and the provider items now do two things: they return to the library and they set its filter. The active destination is held in `uiStore` (`activeDestination: 'library' | 'ports'`) and is not persisted, so the application always opens on the library.

```
+--------------+---------------------------------------+-----------------+
| YOUR SPACE   | [search........] [Console v] [Guided] |  Detail pane    |
|   All games  |  [x] Verified  [Grid|List]   Refresh  |  art, game,     |
|   Steam      +---------------------------------------+  project, needs |
|   DRM-free   |  tile  tile  tile  tile  tile         |  - Bloodborne   |
|              |  tile  tile  tile  tile  tile         |    game folder  |
| EXPLORE      |  (virtualized, same tile treatment)   |  release v0.9.38|
| > PC ports 1 |                                       |  [ Install ]    |
|              |  Port catalog by quiverlauncher.com   |                 |
+--------------+---------------------------------------+-----------------+
                    guided setup opens in a FloatingModal
```

Alternatives considered:

- **"Downloads" tab.** Rejected. An install is mostly guided setup with the user's own files, not a transfer queue. The name also belongs to the shelved BitTorrent design, which had its own consent model. Mixing the two would make it unclear what the user has agreed to.
- **"Explore" as the item itself.** Rejected as the item label because it is vague. The GameNode web product also has discovery and social features, and design principle 1 says desktop must not imply them. Using `EXPLORE` as the *group* header keeps room for future shelves, such as emulators, while the item says exactly what it is.
- **A `FloatingModal` opened from Add a game.** Rejected for browsing. More than 1,000 entries with filters and a detail pane need the full content area, and a modal would hide running installs once it closed. The modal is still the right surface for the guided setup itself, which DESIGN §6.1 lists as a large work surface. One modal opens per install, and it can be closed while the job keeps running.
- **A Settings panel.** Rejected. Settings is for configuration, not for acquiring things.

Install activity appears in three places: a badge on the PC ports item, a progress row in the entry's detail pane, and a toast when an install finishes. No global downloads panel is added.

### D2. Our own upstream client, built from the public OpenAPI description

`crates/platform/src/ports/quiver.rs` implements a small client on the existing blocking `reqwest` stack, running in worker threads. It uses only the GET endpoints it needs:

- the `/apps` listing, paged at `limit=100`
- `/apps/{slug}` and `/apps/{slug}/releases`, at install time and when a detail pane opens without cached releases

The client sends `User-Agent: GameNode-Desktop/<version> (+https://github.com/game-node-app/game-node-desktop)`, honours `Retry-After` on 429 responses and backs off exponentially, fetches pages one after another, and never calls any POST endpoint.

Upstream data is deserialized into our own structs. Every field the client reads is `#[serde(default)]`, and unknown fields are ignored. An entry missing `catalogId`, `slug` or `name` is skipped and counted. If more than 20 % of the entries are skipped, or the listing ends early, the refresh fails and the previous snapshot is kept. This is how the client detects a breaking upstream change.

- **Alternative: use `@quiverlauncher/api` in the UI.** Rejected. It is licensed "All rights reserved", it would move network access into the WebView, and it bypasses the host-only network rule.
- **Alternative: the archived community catalog repository.** Rejected as the main source because it is frozen and unlicensed. It is not used.

### D3. Lazy releases: the snapshot holds the listing only

A refresh makes about 13 requests, one per listing page. Releases, assets and withdrawals are fetched only when a detail pane opens or an install starts. They are cached for 15 minutes in memory and, for installs, kept in the job record. This keeps background traffic small and the snapshot compact. The listing's `verified` block is enough to show the "verified" and "pre-release" badges and the version that would be installed.

### D4. The overlay is a TOML file in this repository, bundled and refreshed

`ports/overlay.toml` follows the style of `providers/*.toml`. It is embedded with `include_str!` at build time, and the host fetches a newer copy from `https://raw.githubusercontent.com/game-node-app/game-node-desktop/main/ports/overlay.toml` with ETag caching (the `manifest_cache.rs` pattern). The overlay has a `schema_version`. A copy with an unknown major version, or one that fails validation, is ignored, and the newest valid copy stays in use.

```toml
schema_version = 1

[[extractor]]
id = "ps4-pkg"
name = "ps4-pkg-extractor"
repository = "paulomanrique/ps4-pkg-extractor"
tag = "v0.0.1"
asset = "pkgextract-x86_64-pc-windows-msvc.zip"
sha256 = "0c736ee02779fb5cf4403a91da23f94fdaa72debf995be0c400ac9c36acee63a"
license = "Unlicense"
executable = "pkgextract.exe"
args = ["{package}", "-o", "{out}", "--overwrite"]   # one package per run; GameNode merges

[entry."qcat_..."]           # Lantern Engine (bloodborne-bloodborne-pc)
gamenode_game_id = 0         # filled in when the overlay is authored
launcher = "BloodbornePC/Lantern Engine.exe"
gameplay = ["BloodbornePC/engine/out/bb-probe.exe"]
license = "GPL-3.0-or-later"

[[entry."qcat_...".setup_file]]
id = "game"
label = "Bloodborne game folder"
kind = "folder"
contains = ["eboot.bin", "sce_sys/param.sfo"]
destination = "CUSA03173"
from_package = { extractor = "ps4-pkg", needs_update = "01.09" }   # no title_id: several retail ids work
```

Merge rules:

- Overlay scalar fields replace upstream values.
- If the overlay defines `setup_file`, it replaces the whole upstream `setupFiles` list. Partial merging of recipe slots is too easy to get wrong.
- `hidden = true` removes the entry from the list.
- Executable roles: `launcher` becomes `Registration.executable` and `launchers`, and `gameplay` becomes `Registration.gameplay`.

**Alternative: a separate repository or a GameNode API endpoint.** Rejected for now. The repository already ships `providers/` data, contributors can review overlay changes in pull requests, and raw GitHub needs no new service.

### D5. Install engine: a staged, journaled state machine

The core defines `PortInstallPlan`, which is pure and testable: the asset, the target folder, the setup slots and how each one is satisfied, files to add, the registration template, and the space it needs. It also defines the job phases:

```
Queued -> Resolving -> Downloading -> Verifying -> Extracting
       -> SuppliedFiles(slot i: Hashing | Extracting pkg | Copying)
       -> Finalizing (rename staging -> target, write external-root copies)
       -> Registering -> Completed
  any  -> Cancelling -> Cancelled      any -> Failed(reason)
  on start, unfinished -> Interrupted (after cleanup)
```

- **Staging.** All work for `<parent>\<folder>` happens in `<parent>\.gamenode-staging-<job_id>\`, on the same volume as the target. Finalizing is one directory rename. This makes cleanup trivial, means nothing is ever half visible at the target path, and lets the free-space check cover a single volume.
- **Journal.** Before the job writes any path, the path is recorded in `port_install_jobs.created_paths`. Paths under the target folder, the staging folder and external roots (`%APPDATA%`, `%LOCALAPPDATA%`) are all recorded. External-root copies are written under a temporary name and renamed when they finish.
- **Registration as the commit point.** The DRM-free registration is inserted in the same SQLite transaction that marks the job `Completed` and records the registration id. Recovery uses this:
  - a registration exists: the job is completed
  - no registration exists: delete the recorded paths and mark the job `Interrupted`
- **Execution.** One job runs at a time, in a dedicated worker thread owned by the app's `PortsService`. Other jobs queue behind it, because hashing and copying large dumps in parallel only causes disk contention. Cancellation is a shared flag checked between chunks of at most 8 MiB, plus a kill for a running extractor process.
- **Progress.** Progress is sent on a separate `ports:progress` Tauri event, coalesced to at most 4 updates a second. `engine:snapshot` carries only phase changes and results, so the library snapshot is not rebuilt for every progress tick. The UI reads progress with a small react-query subscription keyed by job id.

### D6. Reuse and generalize the game-tools archive code

The following move from `crates/platform/src/tools/catalog.rs` into `crates/platform/src/archive.rs`, shared by tools and ports:

- archive type detection
- `validate_archive_entry_path`
- the extracted-size cap
- the zip and 7z extraction loops

Supported Windows asset formats are `zip`, `7z` and a bare `.exe`, which is copied into the folder as is. The extracted-size limit for ports is configured separately, at 16 GiB, because some ports bundle large assets. Downloads are cached under `<data_dir>/ports/cache/<catalogId>/<version>/<filename>`.

### D7. Validating supplied files

- **Files.** A file is hashed once, with SHA-1 and SHA-256 computed in the same pass, in a worker that reports progress. It is accepted if either digest matches any accepted value, or if the slot lists no accepted values.
- **Folders.** Every name in `contains` must exist. If `accepted` hashes are given, `hash_file`, or the first `contains` entry when `hash_file` is absent, is the file hashed. The upstream schema does not say which file in a folder its hashes apply to, so the overlay can make that explicit.
- **Who decides the game version.** Hashes are enforced only when a recipe lists them, which upstream recipes may do. A port that inspects the game itself decides which editions and versions it runs, so a GameNode overlay recipe for it lists only structural markers. Lantern Engine is such a port: its launcher accepts several retail title ids and its engine checks the game image at every start.
- **Placement.**
  - `destination` + `root` → copy (never move), then re-hash the copy for files or the hash file for folders.
  - `flag` / `positional` / `env` → no copy. The flag or positional argument goes into `Registration.arguments`. `env` is stored in the port provenance and applied at launch through the existing launcher path, which gains an optional environment map.

### D8. Running extraction tools

The extractor named in the overlay is downloaded with the same cached, SHA-256-verified release fetch used for game tools, keyed `ports-extractor/<id>/<tag>`. It is extracted into `<data_dir>/ports/tools/<id>/<tag>/` and run with `std::process::Command`:

- no shell
- `CREATE_NO_WINDOW`
- a Windows job object, so cancelling or quitting kills its process tree
- the working directory set to the staging folder

The argument list comes only from the overlay's template. Each placeholder is replaced by a canonicalized path that the user chose or that GameNode created, and is passed as a separate argument.

The pinned PS4 tool takes one package per run and has no update or merge option; its template therefore has no `{update}`. When a slot has an update package and the template does not take `{update}`, the runner performs two runs:

1. the base package into the slot's output folder inside staging;
2. the update package into a temporary folder beside it, inside the staging folder and on the same volume, never inside the slot's output.

It then overlays the update tree onto the base: every update file replaces or joins the file at the same relative path, new folders are created, and nothing in the base is deleted. Files are renamed into place, so a 30 GB base is never copied. Both trees are untrusted, so symbolic links, reparse points and paths resolving outside the game folder are rejected, and cancellation is checked between entries. The temporary folder is removed on success and on failure, and a failed merge never proceeds to Finalizing.

Validation runs once, on the merged folder, exactly like a supplied folder (D7); the intermediate trees are not validated because the update tree alone may not carry the slot's markers. A template that contains `{update}` keeps the single-run behaviour, for a tool that merges the update itself. `check_choice` still builds the digests a later copy re-verifies.

The tool's stdout and stderr are kept, capped at 64 KiB, for the failure details view. Only packages the user's own dump tools produce, which are unencrypted or fake-signed, can work. GameNode ships no keys, and if the tool reports an encrypted retail package, the job fails with a message that says so.

### D9. Port art goes through the asset protocol

Upstream `libraryArt` URLs point to remote CDNs, and the WebView loads images only through `gamenode-asset://`. A new route, `gamenode-asset://localhost/port-art/<catalogId>/<capsule|hero>`, serves cached art. On a cache miss, the protocol handler fetches the URL recorded for that catalog id in the snapshot. It never fetches an arbitrary URL from the request. The fetch runs in a worker and the result is written to `<data_dir>/ports/art/`. Tiles use `<img loading="lazy" decoding="async">` with the titled placeholder fallback.

### D10. Provenance lives beside the registration, not inside it

`Registration` is unchanged. Port provenance is kept in `port_installations`: registration id, catalog id, slug, port name, installed version, install folder, environment map and job id. `PortsView` maps registration keys to provenance, so the library detail pane can show "Installed by PC ports · v0.9.38" and PC ports can show "Installed".

Removing a registration does not delete its provenance row. That lets PC ports show "not installed" correctly, and the files stay on disk. If the same executable is added again later, the existing provenance is linked to it.

### D11. Matching

When an install completes, the overlay's `gamenode_game_id` is passed to the existing local-evidence matcher as evidence of the strongest kind. It ranks with a uniquely mapped external id and still loses to a choice the user made. Without an overlay id, the upstream `games[].title` is passed as title evidence, and the usual conservative rules decide whether to match.

### D12. Lantern Engine game path: a sibling folder it finds by itself

Lantern Engine stores the path as `game_dir` in `%APPDATA%\Lantern Engine\settings.json`, chosen with a picker on its Play page. When `game_dir` is empty and the package folder is valid, it looks for `CUSA03173` in the package folder's parent.

The release archive nests everything under `BloodbornePC\`, so the overlay copies the game to `CUSA03173` in the install folder, beside `BloodbornePC`, and Lantern Engine finds it without a `config_write` step. The folder name does not limit the edition, because Lantern Engine reads the title from `param.sfo`. If it does not find the folder, the user can pick it on the Play page.

## Risks / Trade-offs

- **[The upstream API changes shape, adds authentication or blocks GameNode]** → The cached snapshot and overlay keep the catalog visible, the detail pane shows the cache age, and the skip threshold in D2 turns a silent breakage into a reported refresh failure. Installed ports never depend on the API. The product owner has accepted this risk, and Quiver will not be contacted.
- **[Legal or trademark concerns about using Quiver's data]** → Credit is text only, with no logo. We write our own client and copy no Quiver code. GameNode only reads public, unauthenticated endpoints, sends no write requests and ships no copy of their data.
- **[Copying large dumps uses a lot of disk space and time]** → The free-space check runs before the job starts. Copies are staged on the target volume, can be cancelled every 8 MiB, and are resumable only by retrying (no partial resume). Choosing a flag-based or positional recipe avoids the copy.
- **[An extraction tool is abandoned or its release is replaced]** → The tool is pinned by tag and SHA-256 in the overlay. If the hash does not match, the job fails and nothing runs. A new pin ships through an overlay update, without an app release.
- **[Overlay recipes rot as ports change their requirements]** → The upstream recipe is used wherever the overlay does not override it. Overlay entries are few and reviewed in pull requests.
- **[Antivirus software quarantines a port's executable after install]** → If the preferred executable is missing after finalizing, the "pick executable" path in the specs handles it. A scan-flagged warning before download sets expectations.
- **[Self-updating ports change their executable name]** → The registration uses the normal DRM-free repair flow, and the GameNode-level identity (D10) survives.
- **[Someone uses GameNode to install a port of a game they do not own]** → GameNode never supplies game content. Guided setup requires the user's own files and validates them against the recipe's markers and any hashes it lists. This is the same position emulator front-ends take.

## Migration Plan

1. Migration `0008_ports.sql` adds `port_catalog_snapshot` (a single row: JSON payload, fetched_at, entry_count), `port_overlay` (version, etag, payload), `port_install_jobs`, `port_installations` and `port_preferences`. It is additive, so existing tables are untouched. An older build refuses to open the newer schema, following the existing migration guard.
2. The feature is visible on Windows signed-in builds as soon as it ships. There is no feature flag, because nothing runs until the user opens PC ports. The first catalog fetch happens on the first visit, and daily refreshes start only after that.
3. Rollback: shipping a build without the feature leaves the new tables unused. Port-installed entries remain ordinary DRM-free registrations and keep working.

## Open Questions

- Which PS4 PKG extractor to pin: task 1.2 pinned `paulomanrique/ps4-pkg-extractor` `v0.0.1`. It has a CLI, runs on Windows, uses the Unlicense and takes one package per run, so GameNode merges the update itself (D8). The choice changes only overlay data.
- The exact pt-BR wording for "PC ports" is to be confirmed in copy review. The working label is "Ports para PC".
