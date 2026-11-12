# Design

## Context

See proposal.md (Why). Current state, observed on `main` at `479d49fd`:

**Family is a host concept on the wire.**
- `plugin.proto` defines `CATALOG_CATEGORY_FAMILY = 2` and `optional CatalogFailure family_error = 9`.
- `CatalogTransfer`/`CatalogUpdate` carry a `family` category, and `CatalogError::FamilyUnavailable`
  is stored as `"familyUnavailable"`.
- Core keeps `LibraryEntry.family_shared`, `ProviderCount.family_count` and
  `AvailabilityFilter.family`. The last one is persisted in the saved layout JSON.
- SQLite has `family_shared` and `family_error` columns.
- The UI shows Family counts when `familyCount > 0 || familyError !== null`. That check is a
  heuristic, not a declaration.

**Steam text in the UI.** `GameGrid`, `GameList`, `DetailPane` and `AchievementListModal` use
hardcoded keys:
- `Library::Shared through Steam Family`
- `Library::Steam Family`
- `Library::Steam privacy settings prevent reading these achievements.`
- `Library::Connect Steam to see achievements.`
- `Library::Sign in to Steam again to see achievements.`

`Row.providerName` and `ProviderCount.name` are already in the snapshot.

**Icon hosts are a host constant.** `gamenode_platform::achievement_icons::TRUSTED_ICON_HOSTS`
lists five Steam CDN hosts. Four of them have boundary-allowlist entries; the fifth does not match
the checker's pattern. The constant is used:
- by `trusted_icon_url` (called from `engine/achievements.rs` and `icon_asset`);
- by `AchievementIconLoader::ensure`;
- by the loader's redirect policy.

The cache file name is the SHA-256 of the URL.

**Plugin executables are listed in each script.** The names appear in `run-desktop.{sh,ps1}`,
`build-windows-wsl.sh`, `dev-windows-wsl.{sh,ps1}`, `package-linux.sh`, `package-windows-wsl.sh`
(including per-plugin SHA fields in `build-manifest.json`), `verify-plugin-package.py`,
`packaging/windows/{install,uninstall}.ps1` and `check-plugin-boundaries.py`
(`STORE_PROVIDER_IDS`, `STORE_HOST_PATTERN`).

Each descriptor already carries the data these lists need: `executable` and `platforms`.

**Constraints:**
- Descriptors and manifests are embedded at build time and are trusted. Plugin-supplied runtime
  data is not.
- Unarchived changes already define `desktop-plugins` and `desktop-achievements`.
- AGENTS.md requires async commands, no blocking on the UI thread, Tailwind, and the Mantine
  controls.

## Goals / Non-Goals

**Goals:**
- Steam keeps its current visible behavior, with "Steam Family" text and counts, but the text and
  counts come from declared data.
- No behavior change for Epic or the package-manager provider.
- Existing databases, saved layouts and cached icons keep working without a migration.

**Non-Goals:**
- Renaming SQLite columns or adding a migration.
- Loading descriptors at runtime or from user-writable locations. A new store still needs a host
  rebuild, by design.
- Localizing the package-manager provider name (`ui/src/i18n/providerLabel.ts`). It is not a store.
  It gets one reviewed allowlist entry (Decision 7).
- Retuning the plugin operation timeouts. The comment in `plugin_host/provider.rs` that cites Steam
  is reworded only.

## Decisions

### 1. Rename the catalog category to "shared", wire-compatible

**Choice:**
- Proto: `CATALOG_CATEGORY_FAMILY = 2` becomes `CATALOG_CATEGORY_SHARED = 2`, and
  `family_error = 9` becomes `shared_error = 9`.
- Rust and SDK: `CatalogCategory::Shared`, `CatalogUpdate.shared`,
  `CatalogError::SharedUnavailable`.
  - `as_str` returns `"sharedUnavailable"`.
  - `from_category` also accepts `"familyUnavailable"`, following the existing
    `"invalidCookies"` precedent.
- The API minor does not change.

**Why:** protobuf encodes numbers, not names, so the change is wire-identical and no negotiation
is needed. The renamed identifiers break only in-tree code, which is updated in the same change.

**Alternatives:**
- A new category number with the old one deprecated: this adds a negotiation path for no wire
  gain.
- Keeping the Family names and fixing only the UI: the protocol would keep teaching plugin
  authors a Steam concept.

### 2. Shared access is declared in the provider manifest

**Choice:** `providers/<id>.toml` gains `shared = { label = "<proper name>" }`.
- Core `Manifest` gets `shared: Option<SharedSpec { label }>`.
- Validation: requires `catalog`; the label is 1–32 characters with no control characters.
  Failing it rejects the manifest through the existing diagnostic path.
- `ProviderRegistry` exposes `shared_label(provider)`.

**Why the manifest:** shared access describes the provider's library, alongside `catalog`, and the
host's manifest services already read the manifest. The plugin descriptor holds trust and
authority data, such as grants, the login policy and icon hosts (Decision 4).

**Why not translate the label:** "Steam Family" is a product name. Product names are passed into
translated templates as a parameter (`Shared through {{label}}`, `Compartilhado via {{label}}`), so
descriptors do not carry locale tables.

**Alternative:** the plugin returns the label at runtime. Rejected: the label would be untrusted
plugin text shown as host UI, and it would not be available when the plugin is missing.

### 3. Enforce the declaration and expose it in snapshots

**Enforcement:** on the host side of catalog streaming, a `Shared` chunk or completion from a
provider whose manifest has no `shared` declaration is a protocol error. This uses the existing
`WireError` path, so the sync fails and the previous facts are kept.

**Snapshot fields:**
- `ProviderCount.shared_label: Option<String>`
- `StoreAccountView.shared_label: Option<String>`
- Renamed counterparts: `shared_count`, `shared_error`, `Row.shared` (was `family_shared`),
  `AvailabilityFilter.shared`.

**UI:**
- The Shared filter option and the shared counts and warnings render only when a label exists.
  This replaces the `familyCount > 0 || familyError` heuristic.
- Rows look up the label through their provider.

**Compatibility:** the persisted layout reads `availability.family` through
`#[serde(alias = "family")]` and writes `shared`.

The platform layer keeps the SQL columns `family_shared` and `family_error`. They are mapped in
`library_db.rs`, with a comment noting the historical names.

**Alternative rejected:** renaming the columns. A migration would add risk with no user-visible
benefit, and the boundary checker does not scan SQL vocabulary.

### 4. Icon hosts move to the plugin descriptor, scoped per provider

**Choice:**
- `plugins/<id>.toml` gains `achievement_icon_hosts = [..]`, parsed into
  `PluginDescriptor.achievement_icon_hosts`.
- Validation at descriptor load:
  - requires the `achievements.read` capability;
  - at most 16 entries;
  - each entry is lowercase ASCII LDH labels, at least two labels, with no `*`, `:`, `/`, `@` or
    IP literal.
  - Failing it rejects the descriptor through the existing diagnostic path.
- `gamenode_platform::achievement_icons` drops `TRUSTED_ICON_HOSTS` and takes an `IconHosts` value
  (an `Arc<[String]>` set), threaded through:
  - `trusted_icon_url(raw, &hosts)`
  - `AchievementIconLoader::production(covers_dir, hosts)`
  - `redirect_policy(hosts, allow_loopback)`
- `engine/achievements.rs` resolves the hosts from the descriptor of the provider being read. The
  provider comes from `split_entry_key`. The engine uses the hosted provider runtime's descriptor
  table. A provider without declared hosts gets an empty set, so every icon shows a placeholder.
- The cache key and the `gamenode-asset://localhost/achievement/<hash>` route stay the same, so
  cached icons keep working.

**Why the descriptor:** icon hosts are trust data, like the login policy's allowed hosts, and only
trusted descriptor data may widen what the host fetches.

**Why per provider:** one provider's declaration must not let another provider's URLs be fetched.

**Alternative:** a union of all declared hosts. Rejected for that cross-provider reason.

### 5. A single descriptor reader for scripts

**Choice:** a new `scripts/bundled_plugins.py` uses stdlib `tomllib` and Python 3.11+. It does
three things:
- `load(platform) -> list[Plugin(provider_id, plugin_id, executable, platforms)]`. It validates
  that `executable` is a plain file name matching `[a-z0-9-]+`, that platforms are known, and that
  ids are unique.
- A CLI: `python3 scripts/bundled_plugins.py --platform {linux,windows} --format
  {names,files,cargo-args,json}`. Here `files` adds `.exe` on Windows, and `cargo-args` prints
  `--bin <exe>` pairs.
- `write_index(package, platform)` writes `bundled-plugins.json` as
  `{"schema":1,"plugins":[{"providerId","pluginId","file"}]}`.

**Users of the helper:**
- Bash scripts call the CLI. Cargo builds switch from `-p gamenode-plugin-*` to `--bin <exe>`,
  which resolves workspace binaries by name.
- `package-*.sh` write the index into the package. They replace the fixed
  `steamPluginSha256`/`epicPluginSha256` fields with `pluginSha256: {file: sha}`.
- `verify-plugin-package.py` requires exactly the helper's set and rejects any bundled executable
  for the other platform.
- `check-plugin-boundaries.py` imports the helper.

**Why Python:** every Linux/WSL workflow already requires `python3` (`docs/development.md`), and
`package-windows-wsl.sh` already uses `tomllib`.

**Alternatives:**
- A committed generated index with a drift check: this duplicates the source of truth and needs a
  regeneration step.
- Asking the host binary to print its descriptors: on Windows it is a GUI-subsystem executable,
  and the plugins must be built before the host can launch them.

### 6. PowerShell reads JSON, never TOML

**Choice:**
- `install.ps1` reads `bundled-plugins.json` from the package with `ConvertFrom-Json`.
  - It checks that every listed file exists before touching the install directory.
  - It copies them and records `plugins: [file…]` in `.gamenode-desktop-install.json`.
- `uninstall.ps1` removes the union of the marker's `plugins` and the installed
  `bundled-plugins.json`. An older marker without `plugins` falls back to the index alone.
- `dev-windows-wsl.sh` writes the index next to the built binaries. `dev-windows-wsl.ps1` reads it.
- `run-desktop.ps1` (native Windows) runs the helper with `py -3` or `python`. If neither exists,
  it fails with an actionable message, and `docs/development.md` lists Python 3.11+ for native
  Windows development.

**Why:** PowerShell has no TOML parser. JSON is native to it, and only development checkouts, not
end users, need Python.

### 7. The boundary checker derives store vocabulary from descriptors

**Choice:** `check-plugin-boundaries.py` builds its rules from the helper and the manifests:
- `STORE_PROVIDER_IDS`: every descriptor's `provider_id`.
- The store host pattern: the registrable domains (last two labels) of the login-policy hosts and
  `achievement_icon_hosts`.
- The store URI schemes: the `launch` and `install` URI templates.

Expected results:
- The four icon-host allowlist entries become stale and are deleted. The checker already fails on
  stale entries.
- One new reviewed entry is needed for the `'packagekit'` literal in `ui/src/i18n/providerLabel.ts`.

The checker fixtures in `scripts/tests/test_plugin_boundaries.py` get a temporary descriptor
directory, so the tests do not depend on the real store set.

**Alternative:** keep a hand-written seed list next to the derived one. Rejected: it would bring
back the edit-per-store problem.

### 8. Provider-named messages

**Choice:**
- The UI keys become parameterized:
  - `Library::{{store}} privacy settings prevent reading these achievements.`
  - `Library::Connect {{store}} to see achievements.`
  - `Library::Sign in to {{store}} again to see achievements.`
  - `Library::Shared through {{label}}`
  - `Library::Shared` (the filter label and chip).
- `settings.storeFamilyWarning` becomes `settings.storeSharedWarning` with `{{label}}`.
- `{{store}}` is `Row.providerName` or `ProviderCount.name`.
- The old keys are removed from `en.ts` and `pt-BR.ts`.
- `main.rs` logs `"bundled providers are unavailable"`.

## Risks / Trade-offs

- **[Risk]** A typo in `achievement_icon_hosts` silently disables Steam icons.
  - Mitigation: a registry test asserts that the Steam descriptor declares the five current hosts.
    An engine test fetches a declared host through a loopback fixture.
- **[Risk]** Rejecting undeclared shared data fails a whole sync if a plugin bug sends it.
  - Mitigation: official plugins are in-tree. A contract test covers Epic sending only owned data,
    and the failure keeps cached facts.
- **[Risk]** `--bin <exe>` builds may differ from `-p <crate>` builds in features or targets.
  - Mitigation: the helper also validates that each executable matches a `[[bin]]` or package name
    in `cargo metadata`.
  - Mitigation: `scripts/tests/test_desktop_launcher.py` asserts the exact cargo argument lists
    per platform.
- **[Risk]** Upgrading from an install made before the marker recorded `plugins`.
  - Mitigation: the uninstaller falls back to the package index. `test_windows_installer.py` covers
    an old marker.
- **[Trade-off]** Native Windows `run-desktop.ps1` now needs Python 3.11+. This is documented, and
  the failure message names the requirement.
- **[Trade-off]** Deriving registrable domains from the last two labels misclassifies multi-part
  public suffixes such as `co.uk`. This is acceptable for a lint over our own descriptors, and the
  limitation is documented in the helper.

## Migration Plan

- No data migration.
- Saved layouts with `family` restore through the serde alias.
- Stored `familyUnavailable` categories parse as `SharedUnavailable`.
- Cached icons keep their hashes.
- Rollback: an older build reads the same SQLite columns. A layout rewritten with `shared` would be
  read by an older build as a missing `family` field, which defaults to off. That is acceptable.

**Ordering:** archive this change after `add-achievement-tracking`, `integrate-steam-plugin-into-main`
and `add-epic-games-store-plugin`. Its `desktop-achievements` and `desktop-plugins` deltas add to
capabilities those changes create.
