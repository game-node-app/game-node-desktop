# Proposal

## Why

GameNode only reads one store (Steam), so Epic Games Store games on a Windows PC never appear in the
library or report their playtime. The Steam plugin migration proved the plugin contract, but the
host still has Steam-specific code paths and data. Adding a second store today would mean copying
those Steam branches. Before Epic lands, the host needs to become store-agnostic so that every store
integration runs only through the plugin system and no store code connects to Core outside it.

## What Changes

- **Store-agnostic host.** Remove the store-specific code that still lives outside plugins:
  - `gamenode_core::steam` (`SteamWebApi` and its types).
  - `gamenode_api::SteamClient` and `crates/api/src/steam*`.
  - The orphaned native Steam scanner and VDF parser in `crates/core/src/provider/steam/`.
  - The test-only `engine::steam_account` and `engine::steam_login` adapters. Their regression
    fixtures move to plugin-runtime tests.
- **Trusted plugin descriptors become data.** The `PLUGINS` constant in `plugin_host.rs`
  (provider id, plugin id, name, platforms, grants, executable, manifest, login policy, credential
  alias, native access, legacy profiles) becomes bundled, validated descriptor files. Adding a store
  needs no host Rust branch.
- **Generic per-provider credentials and login profiles.** Each provider gets a logical secret slot
  derived from its provider id. Steam keeps its existing physical slot (`steam` keyring account and
  `steam-token` file) through a descriptor alias, so no credential migration is needed. Isolated
  sign-in profiles are created per provider.
- **Login extraction without cookies.** Login policies may approve only a bounded page probe, with
  no cookie names. This supports OAuth-style flows where the plugin receives a one-time
  authorization code rather than a cookie.
- **Generic account commands and UI.**
  - **BREAKING (internal IPC):** the `steam_sign_in`, `steam_focus_sign_in`,
    `steam_cancel_sign_in`, `steam_sign_out` and `steam_check` Tauri commands are replaced by
    `provider_*` commands that take a provider id.
  - `SteamAccountCard` becomes one store account card rendered for each catalog provider in the
    snapshot.
  - Store-specific copy, such as "invalid cookies" and "Steam changed its response", becomes
    provider-neutral and uses the provider's display name.
- **Unknown last played from counters.** A counter observation may report an unknown last-played
  time, and the host stores it as unknown instead of the Unix epoch.
- **Field-based matching.** A provider manifest may declare which record field is its GameNode
  external id (`match.uid_field`), and owned catalog games may carry named fields. Epic is matched
  by catalog namespace under GameNode's `EpicGamesNamespace` category (1001, a GameNode-only
  value, not IGDB's 26), while its entry key stays `epic:<appName>`.
- **Store totals summed per game.** When several entries of one provider match the same GameNode
  game, their counter totals are submitted as one summed total. This matches how
  `game-node-server` aggregates Epic apps per namespace.
- **Data-driven anti-cheat blocklist.** The Steam app-id blocklist used by game tools becomes data
  keyed by provider entry key.
- **Boundary enforcement.** An automated check fails when non-plugin crates (`core`, `api`,
  `platform`, `app` production code) reference store-specific modules, hosts or provider ids
  outside trusted descriptor data and test fixtures.
- **Epic Games Store plugin (`gamenode-plugin-epic`, Windows):**
  - Installed-game discovery from the Epic Games Launcher manifests, excluding DLC, incomplete
    installs and engine builds.
  - Play and Install actions through the `com.epicgames.launcher://` URI scheme.
  - Opt-in account sign-in in the isolated WebView using Epic's authorization-code flow.
  - The rotating refresh token is stored in the provider's secret slot; the access token stays in
    memory only.
  - Owned-library sync.
  - Epic playtime totals through `playtime.counter`, submitted with source `epicgames`, the
    value `game-node-server` already uses for its own Epic connection.
  - Matching through GameNode external lookups with the Epic Games Store category.

## Capabilities

### New Capabilities

- `desktop-epic-games`: Epic Games Store provider behavior: installed discovery, launcher actions,
  account sign-in and session refresh, owned catalog and playtime totals, delivered entirely by the
  official Epic plugin on Windows.

### Modified Capabilities

- `desktop-store-accounts`: Sign-in, credential protection, session refresh and secret-free
  failures become provider-neutral. The credential is whatever the provider's plugin stores in its
  own slot, such as a cookie or a refresh token, not only a store cookie.
- `desktop-providers`: Adds three rules:
  - Store integrations are delivered only through plugins, with no in-process store adapters in
    host crates.
  - Matching may use a declared record field, and catalog entries carry fields.
  - Per-store data in host features is keyed by entry key.
- `desktop-playtime`: Store totals of entries matched to the same GameNode game are summed into
  one submission.
- `desktop-plugins` and `desktop-plugin-services`: These capabilities are introduced by the
  unarchived `integrate-steam-plugin-into-main` change. This change adds requirements only:
  - Data-driven trusted descriptors.
  - Store-agnostic host code.
  - Per-provider secret slots and login profiles.
  - Cookie-free login extraction.
  - Unknown last-played counter values.

## Impact

- **Rust crates:**
  - `crates/core`: removes `steam.rs` and `provider/steam/`; adds anti-cheat blocklist data.
  - `crates/api`: removes `steam.rs` and its tests; the reqwest features that only Steam needs are
    reviewed.
  - `crates/platform`: generic `CredentialSlot` and paths.
  - `crates/app`:
    - `plugin_host.rs`: descriptor loading.
    - `plugin_host/login`: optional cookies, per-provider profile.
    - `commands.rs` and `main.rs`: generic provider commands.
    - `engine/mod.rs`: drops test-only Steam ports.
    - `engine/playtime.rs`: unknown last played.
  - `crates/plugin-protocol`: additive login-policy and counter semantics, with no field
    renumbering.
  - New `crates/plugin-epic`.
- **Bundled data:**
  - New `providers/epic.toml`.
  - New trusted plugin descriptors for `steam`, `packagekit` and `epic`.
  - `crates/plugin-epic/assets/login.toml`.
- **UI:**
  - `ui/src/api` command wrappers.
  - `features/settings` (`StoresPanel`, account card).
  - i18n (`en`, `pt-BR`).
  - Mock host gains an Epic provider.
- **Packaging and CI:** build and place `gamenode-plugin-epic(.exe)` next to the host. Extend the
  public-dependency checker and the new store-boundary check.
- **Docs:** `docs/plugin-development.md`, `docs/local-state.md` (credential slots) and a new
  `docs/epic-games-store.md`. `docs/steam-web-session.md` is updated to point at the plugin.
- **Data:**
  - No schema migration is expected. Entry keys are `epic:<appName>`.
  - Store accounts, scans and catalog columns are already keyed by provider.
  - Existing Steam credentials, accounts, matches and favorites are preserved.
- **GameNode server:** no server change is needed.
  - `GET /v1/game/external` already accepts category 1001 with namespace uids.
  - `epicgames` is an existing playtime source.
  - The server's own Epic connection (`src/sync/epic`) reports the same per-game totals under the
    same source, so a user with both connections gets idempotent writes, not double counting.
- **Sequencing:**
  - Builds on the Steam plugin code merged in `5e9a8824` and on the in-progress
    `add-achievement-tracking` working tree.
  - Apply after that work is committed, and preserve both.
  - Linux Epic support (Heroic/Legendary), Epic achievements and Epic Family-like sharing are out
    of scope.
  - DRM-free/manual games, PC ports and game tools stay Core features and are not converted to
    plugins.
