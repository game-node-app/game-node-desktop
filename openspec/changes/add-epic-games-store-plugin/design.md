# Design

## Context

See `proposal.md` for the motivation and the delta specs for the behavior contract.

These facts come from the working tree inspected on 2026-11-10, on top of `5e9a8824`:

- The Engine is already provider-generic. `engine/providers.rs` drives sign-in, check, sync,
  discovery and counters through `ProviderRuntime` by provider id.
  `plugin_host/provider.rs` handles grants, epochs and catalog staging generically, and the family
  category is optional (`family: None` already flows).
- **Steam coupling outside the plugin**:
  - `crates/core/src/steam.rs` (`SteamWebApi`, `SteamSession`, `SteamError`) and
    `crates/api/src/steam.rs` (`SteamClient`, still `pub`) are production-compiled but only used by
    test adapters.
  - `crates/core/src/provider/steam/` (scanner, VDF parser, `counter.rs`, fixtures) is orphaned:
    `provider/mod.rs` no longer declares it.
  - `engine/steam_account.rs` and `engine/steam_login.rs` are `#[cfg(test)]`. `Engine` still has a
    test-only `steam: Option<SteamPorts>` field.
  - `plugin_host.rs` holds a `PLUGINS` constant with Steam and PackageKit descriptors. It includes
    `providers/steam.toml` and `plugin-steam/assets/login.toml`, and pins
    `CredentialSlot::Steam`.
  - `platform/credentials` has `CredentialSlot::{Session, Steam}`. `platform/paths` has
    `steam_credential_file` and `steam_login_profile_*`.
  - `commands.rs` and `main.rs` expose `steam_sign_in`, `steam_focus_sign_in`,
    `steam_cancel_sign_in`, `steam_sign_out` and `steam_check`, which just call
    `engine.provider_*("steam")`.
  - In the UI, `SteamAccountCard.tsx`, `StoresPanel.tsx` and the i18n strings are Steam-worded.
  - The core `CatalogError::InvalidCookies` and the proto `CATALOG_FAILURE_INVALID_COOKIES` are
    cookie-worded.
  - `core/tools/anticheat.rs` parses a `steam:` prefix against a hardcoded app-id list.
- **Not store integrations; they stay**:
  - `drm_free/identity.rs` reads the Ludusavi manifest's `steam`/`gog` ids for DRM-free matching.
  - Test fixtures use `steam:620` keys.
  - Template and URI docs mention `steam://` as examples.
- `crates/app/build.rs` already embeds every `providers/*.toml`.
  `scripts/check-plugin-boundaries.py` enforces crate-dependency boundaries but not source
  content.
- **Protocol constraints**:
  - `LoginPolicy` requires `cookie_url` and `cookie_names`.
  - `CounterObservation.last_played` is a plain `int64`, which the Engine stores as
    `Some(value)`, so 0 becomes 1970.
  - `CatalogGame` has only `id` and `name`.
- `library_entries.fields_json` already persists per-entry fields. Lookups key auto-matches by
  `(external_category, store_id)`.
- **`game-node-server` (checked 2026-11-10):**
  - `EGameExternalGameCategory.EpicGamesNamespace = 1001`, in
    `src/game/game-repository/game-repository.constants.ts`. Values of 1000 and above are
    GameNode-only. IGDB's `EpicGamesStore = 26` exists too, but uses store slugs and is not the
    namespace category.
  - Its uids are 32-hex catalog namespaces, for example GTA V
    `0584d2013f0149a791e7b9bad0eec102`. It does not use the launcher `appName`.
  - `GET /v1/game/external` validates `category` with `@IsEnum`, so 1001 is accepted for lookups
    by any signed-in user.
  - The server has its own Epic connection (`src/sync/epic`). It uses the same library-service
    `items` (cursor paging, excluding namespace `ue`), `playtime/account/{id}/all` and catalog
    `bulk/items` endpoints, and the launcher OAuth token endpoint with `token_type=eg1`.
  - Its playtime watcher maps `artifactId` to `appName` to namespace to game, sums the seconds
    per game, and saves them under `UserPlaytimeSource.EPICGAMES = "epicgames"`, platform 6.
- The desktop submitter (`core/playtime/submitter.rs`) submits one total per entry key, using the
  manifest `source`. Two entries matched to one game would overwrite each other's total on the
  server.
- `add-achievement-tracking` (uncommitted working tree) and `integrate-steam-plugin-into-main`
  (code merged, native acceptance pending) are in flight and touch the same files.

## Goals / Non-Goals

**Goals:**

- Host crates contain no store interpretation and no per-store branches. A store is a plugin plus a
  manifest plus a descriptor.
- Keep every Steam behavior, credential location, entry key and data row unchanged.
- Deliver Epic on Windows with installed discovery, actions, account, owned catalog and playtime.

**Non-Goals:**

- Third-party or community plugins, runtime-installed descriptors, a signature or marketplace
  model, or WASM. Descriptors stay compiled-in and trusted.
- Epic on Linux (Heroic/Legendary), Epic achievements, Epic cloud saves, launching without the
  Epic launcher, and Epic DLC management.
- Converting DRM-free/manual, PC ports or game tools into plugins.
- Changing measured-session tracking. Epic installs are tracked by the existing install-folder
  tracker like any entry with an `install_dir`.

## Decisions

### 1. Descriptors as bundled TOML under `plugins/`

Add `plugins/<plugin-id-short>.toml` (`steam.toml`, `packagekit.toml`, `epic.toml`). Each file has
the following fields:

- `provider_id`, `plugin_id`, `name`, `platforms`, `capabilities`, `executable`.
- `provider_manifest`: a provider id that resolves to the embedded `providers/<id>.toml`.
- Optional `login_policy`: a repo-relative path embedded by `build.rs`.
- Optional `credential_alias = { keyring_account, private_file }`.
- `native_access` and `legacy_login_profiles`.

`build.rs` embeds `plugins/*.toml` and their referenced login policies the same way it embeds
providers. `plugin_host` parses them once at startup into owned `PluginDescriptor`s. It keeps the
current validation (manifest id match, platform subset, login ⇔ `auth.webview`) and adds new
checks:

- The cookie grant requires cookie names.
- Capabilities must be known.
- Provider ids must be unique.
- Alias names must be plain file and keyring names.

Invalid descriptors are skipped with a diagnostic. A test asserts that every file is embedded and
valid on its platforms.

`load_bundled_for_tests` builds its `PluginBackend`s from the descriptors instead of literals.

*Alternative:* keep the Rust table and add an Epic entry. That is the per-store code the user wants
removed, and every new store would touch host code.

*Alternative:* load descriptors from disk at runtime. That widens the trust boundary; the
proposal's trust model is compiled-in data.

### 2. Generic credential slots with a data-driven alias

Replace `CredentialSlot::Steam` with `CredentialSlot::Provider { keyring_account, private_file }`,
which is built from the descriptor:

- **Default:** keyring account `provider.<provider_id>` and Linux file
  `provider-<provider_id>.secret` in the private directory.
- **Steam's descriptor alias:** `{ keyring_account = "steam", private_file = "steam-token" }`. The
  physical Steam credential is unchanged and needs no migration.

`CredentialSlot::Session` remains. Provider ids are already restricted to `[a-z0-9-]`, so derived
names are safe. The descriptor validator also rejects aliases equal to the GameNode session names.

The plugin-facing logical key stays `session` (Steam's current key). Epic uses the same key, and
the host keeps rejecting other keys.

Remove `paths::steam_credential_file` and `STEAM_CREDENTIAL_FILE` in favor of the generic
resolver. Remove the `steam_login_profile_*` helpers in favor of `login_profile_dir(provider)` at
`private/login/<provider>`. `legacy_login_profiles` (Steam: `steam-login`) are still removed at
startup.

### 3. Cookie-free login policy and secret-bearing probe

Make `LoginPolicy.cookie_url` optional and allow `cookie_names` to be empty. The rule is
`cookie_url.is_some() == !cookie_names.is_empty()`. The host's `Read` operation skips cookie
collection when no cookies are declared. Descriptor validation ties `auth.webview.cookies.read` to
a non-empty cookie list.

The probe result is already returned as `page_result_json`. It is not logged today; add a test
that asserts it never reaches tracing or snapshots.

This is additive in the TOML schema, and the wire format is unchanged.

The Epic policy (`crates/plugin-epic/assets/login.toml`) is:

- **`start_url`:**
  `https://www.epicgames.com/id/login?redirectUrl=<encoded https://www.epicgames.com/id/api/redirect?clientId=<launcher client>&responseType=code>`.
- **`probe_origin`:** `https://www.epicgames.com`.
- **`allowed_hosts`:** `epicgames.com`, `epicgames.dev`, `unrealengine.com`, `hcaptcha.com`.
  Confirm the final list during Windows acceptance.
- **Probe script:** returns `{ "authorization_code": … }` only when `location.pathname` is
  `/id/api/redirect` and the body parses as JSON with a non-empty `authorizationCode`. Otherwise it
  returns `null`.

The plugin exchanges the code at Epic's OAuth token endpoint and drops it immediately.

*Alternative:* have the plugin open a loopback HTTP server for an OAuth redirect. Epic's launcher
client has no loopback redirect, and an open port is a larger surface than the existing broker.

### 4. Generic account commands and UI

- **Tauri commands:** replace the five `steam_*` commands with `provider_sign_in`,
  `provider_focus_sign_in`, `provider_cancel_sign_in`, `provider_sign_out` and `provider_check`,
  each taking `provider: String`. Each command validates that the id is a loaded catalog provider
  before calling the existing Engine methods; an unknown id is a no-op error. This is internal IPC
  only, and the UI and mock host change in the same step.
- **UI:**
  - `SteamAccountCard` becomes `StoreAccountCard`, keyed by `account.provider`, with copy
    interpolating `provider_name`.
  - `StoresPanel` maps over snapshot accounts.
  - Family rows render only when the provider has reported family data (`last_family_at` or
    `family_error`).
- **Failure categories:** rename `CatalogError::InvalidCookies` to `InvalidCredential` and the
  proto value to `CATALOG_FAILURE_INVALID_CREDENTIAL`, keeping number 7. Proto field numbers and
  wire values are unchanged. The serialized snapshot string changes, so the UI maps both the old
  and new string while the change rolls out.
- **Copy:** messages such as "Steam changed its response" become "{provider} changed its
  response" in `en` and `pt-BR`.

### 5. Remove dead Steam code outside the plugin

Delete the following:

- `core/src/steam.rs`, `core/src/provider/steam/**`, `api/src/steam.rs` and `api/src/steam/**`,
  and the `SteamClient` re-export.
- `engine/steam_account.rs`, `engine/steam_login.rs` and `engine/tests/steam.rs`.
- The `steam` field and `SteamPorts` from `Engine`.

Before deleting, list the scenarios that the Steam engine tests cover:

- Sign-in close or cancel.
- Late result after sign-out.
- Account switch clears the catalog.
- Rotated credential.
- Expired versus unreachable.
- Deletion failure.
- Family failing independently.

Keep each scenario either in `engine/tests/provider_runtime.rs` (fake `ProviderRuntime`) or in
`plugin-steam/src/runtime/tests.rs`. Record that mapping in the PR description. Steam HTTP
parsing tests already live in `plugin-steam/src/http/tests.rs`. Port any `api/src/steam/tests.rs`
case that has no equivalent there.

Prune `reqwest` features in `gamenode-api` that only the Steam client needed, such as brotli and
zstd, if nothing else uses them.

### 6. Store-boundary source check

Extend `scripts/check-plugin-boundaries.py`:

- Add `gamenode-plugin-epic` to the public and forbidden-host crate lists.
- Add a source scan over the production code of `crates/{core,api,platform,app}/src` and
  `ui/src`, excluding `#[cfg(test)]` modules, `tests/` directories, `*.test.ts(x)`, `ui/src/mock`
  and fixtures.

The scan fails on these patterns:

- Store hostnames: `steampowered`, `steamcommunity`, `steamstatic`, `epicgames`.
- Store module paths: `::steam::`, `::epic::`.
- Provider-id literal comparisons: `== "steam"`, `"epic"` and similar, for every store descriptor
  id.
- `steam_` and `epic_` identifiers.

It reads a small reviewed allowlist (`scripts/plugin-boundary-allowlist.txt`) for the Ludusavi
`steam`/`gog` identity fields and doc comments that only cite URI examples. A Python unit test
covers both a violation and an allowed case.

*Alternative:* a Rust-level lint. That has no stable mechanism; the existing Python checker
already runs in CI.

### 7. Field-based matching and catalog fields

- **Manifest:** `MatchSpec` gains an optional `uid_field: String`.
- **`plan_lookups`:** the uid is `entry.fields[uid_field]` when the field is declared, otherwise
  `store_id`. Entries without the field are skipped.
- **Lookup results:** auto-match results are cached under `(category, uid)` as today. Applying
  them maps back to every entry of that provider whose uid matches, so one namespace can cover
  several app names. The `auto_matches` storage shape is unchanged because it already keys by
  uid.
- **`CatalogGame`:** gains `map<string,string> fields = 3`. This is additive, and the host bounds
  it to 16 keys with lengths of at most 64 and 256. The catalog merge writes those fields to
  `fields_json`. The rules are:
  - Discovery fields win when an entry is installed.
  - Catalog fields fill only missing keys, so a catalog sync never erases installed fields.
- **Epic:** discovery and catalog both report `namespace` and `catalog_item_id`. The Epic manifest
  declares `match = { external_category = <EpicGamesNamespace id>, uid_field = "namespace" }`.

The category is `1001` (`EpicGamesNamespace`), confirmed in `game-node-server`. Using IGDB's 26
would silently produce no matches, because those uids are slugs. A manifest test pins 1001 with a
comment pointing at the server enum.

*Alternative:* use `namespace` as the entry key. Launch, install and playtime all key on
`appName`, and several apps can share a namespace. The entry key must stay `epic:<appName>`.

### 8. Unknown last-played in counters

Change `CounterObservation.last_played` to `optional int64` (same field number 3; proto3 presence
is wire-compatible) and bump the API minor to 5.

The Engine maps absent values to `last_played_at: None` and keeps rejecting negative values.
A side effect is that Steam's never-played entries, previously sent as 0 and therefore absent on
the wire, now become unknown instead of 1970. That fixes an existing display bug and is noted in
the PR.

The SDK exposes `Option<i64>`, and the Steam plugin is adjusted to pass `Some` only when it has a
date.

### 9. Anti-cheat blocklist as data

Move `BLOCKLISTED_STEAM_APP_IDS` to `crates/core/data/anticheat.toml`
(`[[blocked]] entry = "steam:1172470"`, `title = "…"`), embedded with `include_str!` and parsed
once. `anticheat.rs` matches the full entry key and keeps the title heuristics. Existing tests keep
their expectations.

### 10. Epic plugin (`crates/plugin-epic`, binary `gamenode-plugin-epic`)

The crate structure mirrors `plugin-steam`: `main.rs`, `runtime.rs`, `discovery`, `http`,
`account.rs` and `login.rs`. It depends only on `gamenode-plugin-protocol`,
`gamenode-plugin-sdk`, `reqwest` (rustls, json, gzip), `serde`, `serde_json`, `tokio`, `tracing`
and `url`, plus `winreg` under `cfg(windows)`.

The crate compiles and runs its unit tests on Linux, but its descriptor lists only `windows`.

- **Grants:**
  - `provider.context.read`, `library.discovery`, `library.catalog`.
  - `account.state.read`, `account.state.publish`.
  - `auth.webview`, `auth.webview.page.read`. It has no cookies grant.
  - `secrets.read`, `secrets.write`, `secrets.delete`.
  - `playtime.counter`, `provider.actions.declare`.
  - It has no achievements grant.
- **Native access:** `files`, `network`, `registry`.
- **Roots:** `providers/epic.toml` declares
  `[[roots.windows]] registry = 'HKLM\SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher'`, value
  `AppDataPath`, with a fallback path of `%ProgramData%\Epic\EpicGamesLauncher\Data`. The plugin
  expands the environment variable itself.
- **Discovery:**
  - Read `<data>\Manifests\*.item` as JSON. Keep an item when all of these hold:
    - `AppName` and `InstallLocation` are non-empty.
    - `bIsIncompleteInstall` is not true.
    - `MainGameAppName` is empty or equals `AppName`.
    - `AppCategories` contains `games`.
    - It is not the engine namespace `ue`.
  - Report `id = AppName`, `name = DisplayName`, `install_dir = InstallLocation`, and fields
    `namespace` and `catalog_item_id`.
  - A missing data directory is a successful empty result. An unreadable `Manifests` directory is
    a failure.
  - The host already runs discovery off the UI thread, and the plugin uses blocking reads inside
    `spawn_blocking`.
- **Actions:**
  - Play: `com.epicgames.launcher://apps/{id}?action=launch&silent=true`.
  - Install: `com.epicgames.launcher://apps/{id}?action=install`.
  - Templates already percent-encode values.
- **Account:**
  - **Client:** the public Epic launcher client id and secret are plugin constants. They are
    already widely published by open-source launchers and are not user secrets.
  - **Sign-in:** open login, then read the probe, exchange the `authorization_code` grant, and
    receive the access token, refresh token, `account_id` and `displayName`.
  - **Storage:** write the refresh token to the `session` secret, then publish the identity.
  - **Check:** read the secret, then exchange the `refresh_token` grant.
    - Success writes the rotated refresh token before publishing connected.
    - `invalid_grant` or HTTP 400/401 deletes the secret and publishes expired.
    - Network or 5xx errors publish unreachable and keep the secret.
  - **Sign-out:** delete the secret, then best-effort revoke via the OAuth `DELETE` session
    endpoint with a short timeout.
- **Catalog:**
  - Page through the library-service `items` endpoint with `includeMetadata=true` and a cursor.
  - Drop records in namespace `ue` and records that are not `appName`-addressable.
  - Resolve titles and DLC status through the catalog-service bulk-items endpoint, per namespace,
    with up to 8 concurrent requests and one retry each. Items with `mainGameItem`, or without a
    `games` category, are excluded.
  - Any page or title failure after the retry fails the owned category (`UNREACHABLE`,
    `RATE_LIMITED` or `SERVER`), so previous facts remain.
  - Chunks are streamed with the existing chunking.
  - The whole sync honors the host's 300-second `SyncCatalog` budget; per-request timeouts are
    20 seconds.
- **Counter:**
  - With a connected account, use the library-service playtime endpoint
    (`/playtime/account/{accountId}/all`), which returns `artifactId` and `totalTime` in seconds.
  - Report `id = artifactId`, `seconds = totalTime` and an absent `last_played`.
  - The manifest sets `source = "epicgames"` so submissions use the server's existing Epic source.
    The provider id stays `epic`.
  - Without a connected account or credential, return no observations.
- **Logging:** log category, status code and body size only. Never log tokens, codes, account ids
  or URLs with query strings.

All Epic endpoints are unofficial, as Steam's are. They are isolated in `http.rs` with fixture
tests based on recorded, scrubbed response shapes.

### 11. Sum store totals per matched game

Change the candidate building that feeds `Submitter`. Group counter totals by
`(account, provider, matched game_id)` and emit one candidate per group, with
`total_seconds = Σ` and `last_played_at = max` of the known values.

The group's accepted and rejected state is stored on a synthetic key
`<provider>:game:<game_id>` in the existing `playtime_totals` table. No schema change is needed
because the table is keyed by `(account_id, entry_key)`, and per-entry rows remain for display.
When a group contains a single entry, the candidate keeps that entry's own key and stored state,
so existing Steam rows behave exactly as before and do not re-submit.

When a second entry joins a group, the group switches to the synthetic key. Its first submission
is the new sum, which the server accepts as a total replacement.

This mirrors the server's own per-namespace summation, so desktop-only and server-connected Epic
users converge on the same value.

*Alternative:* submit only the largest entry. That under-reports games split into several apps,
and it diverges from what the server's Epic connection writes.

### 12. Packaging

Add `gamenode-plugin-epic` to:

- `crates/app/Cargo.toml` (build ordering, if Steam is listed there).
- `scripts/build-windows-wsl.sh`, `package-windows-wsl.sh`, `dev-windows-wsl.{sh,ps1}` and
  `run-desktop.ps1`.
- `packaging/windows/install.ps1` and `uninstall.ps1`.
- `scripts/verify-plugin-package.py` and its tests, which require the Epic executable on Windows
  and forbid it on Linux.

`package-linux.sh` and `run-desktop.sh` explicitly skip it.

## Risks / Trade-offs

- **[Epic changes or blocks its unofficial endpoints or the launcher client]**: isolate HTTP in the
  plugin and report categorized failures that keep cached facts. A fix ships as a plugin-only
  change.
- **[The Epic login page refuses the embedded WebView2, or captcha needs extra hosts]**: validate on
  the Windows VM. The host list is trusted data, so it can be adjusted without code. Social logins
  that leave Epic hosts open externally and are documented as unsupported; email/password plus
  2FA is the supported path.
- **[Large libraries with hundreds of free-game claims exceed the sync budget]**: bounded
  concurrency, per-namespace requests and a failure rather than a partial result. If acceptance
  shows the budget is too small, raise the per-provider timeout through descriptor data rather
  than special-casing Epic.
- **[Rename of the `InvalidCookies` snapshot string breaks a cached UI state]**: the UI accepts both
  strings for one release, and snapshots are regenerated on startup.
- **[Removing the Steam engine tests loses coverage]**: map each scenario before deleting
  (Decision 5). Review the mapping in the PR.
- **[The source-scan check produces false positives]**: a reviewed allowlist with justification
  comments, and test coverage for the checker itself.
- **[In-flight work conflicts in `engine/mod.rs`, `plugin_host.rs`, proto and UI]**: start
  implementation only after `add-achievement-tracking` is committed. Rebase the host refactor
  first, then add Epic.
- **[A wrong external category number]**: resolved. It is 1001, pinned by a manifest test
  that references the server enum.
- **[Both the desktop plugin and the server's Epic connection report Epic playtime]**: both
  write Epic's own cumulative total, summed per game, under `epicgames` and platform 6, so the
  writes are idempotent. The server's `lastPlayedDate` heuristic stays server-only; the desktop
  sends no last-played date for Epic.
- **[A game's entry set changes and the summed key switches]**: the first submission after the
  switch is a full total, which the server treats as a replacement. Tests cover joining and
  leaving a group.

## Migration Plan

1. Land the host refactor (Decisions 1-9 and 11) with Steam and PackageKit only. Verify that Steam
   behavior, credential location, entry keys and data rows are unchanged, and run the full test
   suite.
2. Add the Epic plugin, manifest, descriptor and packaging (Decisions 10 and 12), with fixture-tested
   discovery, HTTP and account logic on Linux CI.
3. Run Windows VM acceptance for Steam regression and Epic flows, after the user confirms VM
   availability.

There is no schema migration. Rollback means reverting the commits. An older build still reads the
Steam credential (same physical slot). Epic data would linger as `epic:` rows that the older build
ignores, because the provider would not be loaded. A `provider.epic` keyring entry may also remain;
uninstall cleanup removes it.

## Open Questions

- Whether the `launch&silent=true` URI also starts games that require the launcher to finish an
  update first. Acceptance records the behavior; no design change is expected.
