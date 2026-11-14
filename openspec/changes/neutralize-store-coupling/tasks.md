# Tasks

## 1. Baseline

- [x] 1.1 Record the base SHA and confirm the starting state.
  - Confirm that `add-achievement-tracking`, `integrate-steam-plugin-into-main` and
    `add-epic-games-store-plugin` are committed on the base.
  - Confirm that the working tree has no unrelated uncommitted work.
  - Verify: `cargo test --locked --workspace`, `pnpm --dir ui test --run`,
    `python3 -m unittest discover scripts/tests` and `python3 scripts/check-plugin-boundaries.py`
    all pass on the base. Record the results in the PR description.

## 2. Protocol and SDK: Family becomes Shared (design Decision 1)

- [x] 2.1 Rename the catalog identifiers in `plugin.proto` and regenerate.
  - `CATALOG_CATEGORY_FAMILY = 2` becomes `CATALOG_CATEGORY_SHARED = 2`.
  - `family_error = 9` becomes `shared_error = 9`.
  - Keep the numbers and the API minor.
  - Verify with a `crates/plugin-protocol/tests/contract.rs` case that decodes bytes encoded with
    the old names into `Shared`.
- [x] 2.2 Rename the protocol and SDK models.
  - In `catalog.rs`/`models.rs`: `CatalogTransfer.shared`, `CatalogUpdate.shared`,
    `CatalogError::SharedUnavailable`.
  - `as_str` returns `"sharedUnavailable"`, and `from_category` also accepts `"familyUnavailable"`.
  - Update `crates/plugin-sdk` and the Steam plugin's catalog code to the new names.
  - Verify: unit tests for both stored strings; `cargo test --locked -p gamenode-plugin-protocol -p
    gamenode-plugin-sdk -p gamenode-plugin-steam`.

## 3. Declared shared access in manifests and the host (Decisions 2–3)

- [x] 3.1 Add the manifest declaration.
  - Add `shared: Option<SharedSpec { label }>` to the core `Manifest`.
  - Validation: requires `catalog`; the label is 1–32 characters with no control characters.
  - Expose `shared_label` from the registry.
  - Add `shared = { label = "Steam Family" }` to `providers/steam.toml`.
  - Verify with registry tests in `crates/core/src/provider/registry/tests.rs`:
    - Steam's label loads and Epic has none;
    - shared without a catalog is rejected with a diagnostic;
    - empty, 33-character and control-character labels are rejected;
    - the other manifests still load.
- [x] 3.2 Enforce the declaration on the host side.
  - In the plugin-host catalog streaming, treat `Shared` chunks and completions from a provider
    without a `shared` declaration as a protocol error.
  - Verify with `crates/app/src/engine/tests/provider_runtime.rs` cases:
    - an undeclared provider streaming shared records fails the sync and keeps the previous owned
      and shared facts;
    - Steam's shared records are still accepted.
- [x] 3.3 Rename the core library model.
  - `LibraryEntry.shared`, `Row.shared`, `ProviderCount.shared_count`,
    `AvailabilityFilter.shared` with `#[serde(alias = "family")]`.
  - Snapshot fields `sharedCount`, `sharedError` and `sharedLabel`, on `ProviderCount` and
    `StoreAccountView`.
  - Keep the SQL columns `family_shared`/`family_error` and map them in `library_db.rs` with a
    comment.
  - Verify:
    - `crates/core/src/library/layout.rs` tests: a saved `{"availability":{"family":true}}` restores
      as `shared`;
    - `crates/core/tests/serialization.rs` and the snapshot tests: camelCase `shared*` fields;
    - `crates/platform/src/library_db/tests.rs` and the `db.rs` upgrade tests still pass with
      existing rows.

## 4. UI: neutral shared access and provider-named messages (Decision 8)

- [x] 4.1 Update the UI types and mock host.
  - In `ui/src/api/types.ts`: `shared`, `sharedCount`, `sharedError`, `sharedLabel`,
    `availability.shared`, and `CatalogError` `'sharedUnavailable'`.
  - Keep `'familyUnavailable'` accepted for one release, as `invalidCookies` is.
  - Update `ui/src/mock/host.ts`: Steam has a label, Epic has none.
  - Verify: `pnpm --dir ui exec tsc --noEmit`.
- [x] 4.2 Update the library views (`GameGrid`, `GameList`, `DetailPane`, `LibraryToolbar`).
  - Show the shared indicator and the access line as `Shared through {{label}}` and the provider's
    label.
  - Offer the Shared filter option and chip only when some loaded provider has a label.
  - Follow AGENTS.md: Tailwind classes, Mantine controls, no static inline styles.
  - Verify with Vitest component tests:
    - a Steam row shows `Steam Family`;
    - an Epic row shows no shared text;
    - with no labelled provider, the filter offers only Installed and Owned;
    - in pt-BR, the label is unchanged and the surrounding text is translated.
- [x] 4.3 Parameterize the achievement messages.
  - In `DetailPane` and `AchievementListModal`, use `{{store}}` with `providerName` for the
    privacy, connect and sign-in-again messages.
  - Verify with Vitest:
    - a Steam privacy failure names Steam;
    - a not-connected provider is named by its display name;
    - pt-BR strings render.
- [x] 4.4 Update the Stores settings.
  - In `StoreAccountCard` and `StoresPanel`, show shared counts and `settings.storeSharedWarning`
    only when `sharedLabel` is set. This replaces the `familyCount > 0 || familyError` heuristic.
  - Verify with Vitest:
    - the Steam card shows 8 shared items labelled `Steam Family` and its shared warning;
    - the Epic card shows no shared count or warning.
- [x] 4.5 Update the translations.
  - Replace the Steam-specific keys in `ui/src/i18n/en.ts` and `pt-BR.ts` with the parameterized
    keys and delete the old ones.
  - Verify:
    - the existing i18n key-parity test passes;
    - `grep -rn "Steam" ui/src --include=*.tsx` finds no Steam text outside tests and mocks.

## 5. Achievement icon hosts from descriptors (Decision 4)

- [x] 5.1 Add `achievement_icon_hosts` to the descriptor.
  - Add it to `DescriptorFile`/`PluginDescriptor` in `crates/app/src/plugin_host.rs`.
  - Validation: requires `achievements.read`; at most 16 entries; lowercase LDH host names with at
    least two labels; no wildcard, port, path, userinfo or IP literal.
  - Add the five current hosts to `plugins/steam.toml`.
  - Verify with descriptor tests:
    - Steam loads exactly those five hosts;
    - each invalid form is rejected with a diagnostic while the other descriptors load;
    - hosts without the grant are rejected.
- [x] 5.2 Remove the constant from `crates/platform/src/achievement_icons.rs`.
  - Remove `TRUSTED_ICON_HOSTS`.
  - Thread an `IconHosts` set through `trusted_icon_url`, `AchievementIconLoader::production` and
    `redirect_policy`. Keep the hash and the asset route unchanged.
  - Verify with the module tests:
    - a declared host is accepted;
    - an undeclared host, HTTP, or a lookalike suffix is rejected;
    - a redirect to an undeclared host is refused, using the loopback fixture.
- [x] 5.3 Resolve hosts per provider in the engine.
  - In `crates/app/src/engine/achievements.rs` and `icon_asset`, take the hosts from the
    descriptor of the entry's provider. A provider without hosts gets an empty set.
  - Verify with `crates/app/src/engine/tests/achievements.rs`:
    - Steam URLs become asset routes;
    - a URL on a Steam host reported by another provider stays empty and is not fetched;
    - icons cached before the change are still served.

## 6. Plugin set derived from descriptors (Decisions 5–6)

- [x] 6.1 Add `scripts/bundled_plugins.py`.
  - It provides `load`, `write_index` and the CLI formats `names`, `files`, `cargo-args` and `json`.
  - It validates plain `[a-z0-9-]+` executables, known platforms, unique ids, and that each
    executable is a binary in `cargo metadata`.
  - Add `scripts/tests/test_bundled_plugins.py` and verify:
    - Windows yields Steam and Epic;
    - Linux yields Steam and package-manager;
    - a temporary malformed or path-like descriptor fails with its file name.
- [x] 6.2 Switch the build and run scripts to the helper.
  - Scripts: `run-desktop.sh`, `build-windows-wsl.sh`, `dev-windows-wsl.sh`, `run-desktop.ps1`
    (via `py -3`/`python`, with an actionable error if Python is missing), and
    `dev-windows-wsl.ps1` (reads the index written by `dev-windows-wsl.sh`).
  - Use `--bin` cargo arguments and existence checks instead of literal names.
  - Document Python 3.11+ for native Windows in `docs/development.md`.
  - Verify: `scripts/tests/test_desktop_launcher.py` asserts the per-platform cargo arguments and
    checks, and no script outside the helper contains `gamenode-plugin-`.
- [x] 6.3 Switch packaging and verification to the helper.
  - `package-linux.sh` and `package-windows-wsl.sh` write `bundled-plugins.json` and copy the
    helper's files.
  - `build-manifest.json` gets `pluginSha256: {file: sha}` instead of the per-store fields.
  - `verify-plugin-package.py` requires exactly the helper's set plus the index, and rejects any
    plugin bundled for the other platform.
  - Verify: `scripts/tests/test_plugin_package.py` covers both platforms, a missing plugin, an
    extra cross-platform plugin and a missing index.
- [x] 6.4 Switch the Windows installer and uninstaller to the index.
  - `install.ps1` reads `bundled-plugins.json`, checks every listed file before changing
    anything, copies them, and records `plugins` in `.gamenode-desktop-install.json`.
  - `uninstall.ps1` removes the union of the marker's plugins and the index, falling back to the
    index for older markers.
  - Update `packaging/linux/README.md` and the Windows packaging docs for the index.
  - Verify: `scripts/tests/test_windows_installer.py` covers:
    - the file list derived from the index;
    - stopping on a missing indexed file;
    - removing a plugin dropped since install;
    - an old marker without `plugins`.
- [x] 6.5 Derive the boundary checker's vocabulary.
  - In `check-plugin-boundaries.py`, take provider ids, registrable store domains (from the
    login-policy hosts and icon hosts) and launch/install URI schemes from the helper and the
    manifests.
  - Delete the four stale icon-host allowlist entries and add one reviewed entry for
    `ui/src/i18n/providerLabel.ts`.
  - Verify:
    - `scripts/tests/test_plugin_boundaries.py` uses a temporary descriptor set, and a new store id
      literal in host code fails;
    - `python3 scripts/check-plugin-boundaries.py` passes on the tree.

## 7. Host residue and integration

- [x] 7.1 Neutralize the remaining Steam wording in host code.
  - The `main.rs` startup log becomes "bundled providers are unavailable".
  - Reword the `operation_timeout` comment in `plugin_host/provider.rs` and the Family doc comments
    in `core/src/library/{store,query,layout}.rs` without naming Steam as the model.
  - Verify: `grep -rniE "steam|epic" crates/{app,core,platform,api}/src` outside tests shows only
    the Ludusavi manifest field and illustrative examples that the boundary check allows.
- [x] 7.2 Run the full verification.
  - `cargo test --locked --workspace`, `cargo clippy --locked --workspace -- -D warnings`,
    `pnpm --dir ui test --run`, `pnpm --dir ui exec tsc --noEmit`, `python3 -m unittest discover scripts/tests`
    and `python3 scripts/check-plugin-boundaries.py`.
  - Verify: all of them pass, with results recorded in the PR description.
- [x] 7.3 Check the UI visually.
  - Run `VITE_UI_MOCK=1 pnpm --dir ui dev` and confirm in the browser:
    - the Steam shared indicator, detail access line, Shared filter and Stores card show
      `Steam Family`;
    - the Epic card and rows show no shared text;
    - achievement messages name their provider in English and pt-BR.
  - Verify: screenshots attached to the PR.
- [x] 7.4 Accept the Windows package natively.
  - Run `scripts/package-windows-wsl.sh` and install with `install.ps1` under a path with spaces.
  - Confirm that the Steam and Epic plugins start, Steam achievement icons load, and
    `uninstall.ps1` removes every plugin file.
  - Verify: results recorded as native evidence, separate from build and fixture results.

## Workflow follow-up

- Archive only after `add-achievement-tracking`, `integrate-steam-plugin-into-main` and
  `add-epic-games-store-plugin` are archived, so the `desktop-achievements` and `desktop-plugins`
  deltas apply to existing specs.
- After archiving, confirm that `openspec/specs/desktop-providers`, `desktop-library` and
  `desktop-settings` no longer describe Steam Family as a host-wide concept.
