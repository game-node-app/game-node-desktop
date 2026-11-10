# Tasks

## 1. Baseline

- [ ] 1.1 Confirm that `add-achievement-tracking` is committed and the working tree has no unrelated uncommitted work.
  - Record the base SHA and the green results of `cargo test --locked --workspace` and `pnpm --dir ui test --run` in the PR description.
  - Done when both suites pass on the recorded base.
- [ ] 1.2 Resolve GameNode's numeric external category for `EpicGamesNamespace`.
  - Use the server's category enum or a `GET /v1/game/external` call with a known namespace, for example GTA V `0584d2013f0149a791e7b9bad0eec102`.
  - Record the value in `docs/epic-games-store.md`.
  - Done when a lookup with that category and namespace returns the expected answer shape.

## 2. Protocol and SDK (additive)

- [ ] 2.1 In `plugin.proto`, make the following additive changes, then regenerate:
  - Make `CounterObservation.last_played` `optional`.
  - Add `map<string,string> fields = 3` to `CatalogGame`.
  - Rename `CATALOG_FAILURE_INVALID_COOKIES` to `CATALOG_FAILURE_INVALID_CREDENTIAL`, keeping number 7.
  - Bump the API minor to 5.
  - Verify that the `crates/plugin-protocol/tests/contract.rs` cases cover the old-minor negotiation and the absent/present `last_played` round trip.
- [ ] 2.2 Make `LoginPolicy.cookie_url` optional and allow empty `cookie_names`, with the rule `cookie_url.is_some() == !cookie_names.is_empty()`.
  - Verify with unit tests for valid cookie-free, valid cookie and mismatched policies.
  - Verify that `assets/login.toml` still parses.
- [ ] 2.3 Update the SDK types:
  - `Option<i64>` last played.
  - Catalog game fields with a bounded builder.
  - Adjust `plugin-steam` to send `Some` only for real dates.
  - Verify with `cargo test --locked -p gamenode-plugin-sdk -p gamenode-plugin-steam` and the reference-host tests.

## 3. Descriptors as data and generic credentials

- [ ] 3.1 Add `plugins/steam.toml` and `plugins/packagekit.toml` with the same content as today's `PLUGINS` entries, including Steam's `credential_alias = { keyring_account = "steam", private_file = "steam-token" }`.
  - Extend `crates/app/build.rs` to embed `plugins/*.toml` and their referenced login policies.
  - Verify with a test that every file on disk is embedded.
- [ ] 3.2 Replace the `PLUGINS` constant with parsed, validated `PluginDescriptor`s, following design Decision 1. The validation rules are:
  - Manifest id matches the provider id.
  - Platforms are a subset of the manifest's platforms.
  - The login policy matches the login grant.
  - The cookie grant requires cookie names.
  - Capabilities are known.
  - Provider ids are unique.
  - Alias names are safe.
  - Verify with tests for each rejection, and that `inventory`, `backends` and `load_bundled_for_tests` derive from descriptors with no provider literals.
- [ ] 3.3 Replace `CredentialSlot::Steam` with a descriptor-built provider slot, keeping `Session`.
  - The default is `provider.<id>` in the keyring and `provider-<id>.secret` on Linux.
  - Steam resolves through its alias.
  - Remove `steam_credential_file` and `STEAM_CREDENTIAL_FILE`.
  - Verify with platform tests that Steam still maps to `steam` and `steam-token`, that Epic maps to its own names, and that cross-provider keys are rejected in `plugin_host/services/tests.rs`.
- [ ] 3.4 Replace the Steam login-profile helpers with `login_profile_dir(provider)` under `private/login/<provider>`.
  - Remove the directory at the end of each attempt.
  - Keep startup cleanup of descriptor `legacy_login_profiles`.
  - Verify with login tests for success, cancel and timeout cleanup, and for two providers' concurrent attempts using separate directories.
- [ ] 3.5 Make the login `Read` operation skip cookie collection when the policy declares no cookies.
  - Add a test asserting that `page_result_json` and cookies never appear in tracing output or snapshots.
  - Verify with `cargo test --locked -p gamenode-app plugin_host::login`.
- [ ] 3.6 Update `docs/plugin-development.md` (descriptor format, per-provider secrets, cookie-free login, API minor 5) and `docs/local-state.md` (credential slot names).
  - Done when the docs describe the delivered behavior with no Steam-only wording for generic services.

## 4. Matching and catalog fields

- [ ] 4.1 Add an optional `uid_field` to the manifest `MatchSpec`.
  - Use it in `plan_lookups` and when applying lookup answers. When declared, the uid is the field value; entries without it are skipped; one answer applies to every entry with that uid.
  - Verify with `core/library/lookup` tests, including the unchanged Steam behavior.
- [ ] 4.2 Persist catalog game fields into `fields_json` during catalog merge.
  - Installed discovery fields win, and catalog fields fill only missing keys.
  - Bound the field count and lengths at the host.
  - Verify with core library and plugin-host tests, and with a restart test showing that fields survive.
- [ ] 4.3 Map an absent counter `last_played` to `last_played_at: None` in `engine/playtime.rs`, and keep rejecting negative values.
  - Verify with `engine/tests/playtime.rs` that the entry shows last played as unknown and the total is still submitted.

## 5. Generic account commands and UI

- [ ] 5.1 Replace the `steam_*` Tauri commands with the `provider_*` commands from design Decision 4.
  - Each command takes a `provider` id and validates it against loaded catalog providers.
  - Update the `main.rs` registration.
  - Verify with command tests that an unknown provider is rejected and `epic`/`steam` route to the right runtime.
- [ ] 5.2 Rename `CatalogError::InvalidCookies` to `InvalidCredential` across the core, the engine mapping and the snapshot.
  - The UI accepts both strings.
  - Verify with `cargo test --locked -p gamenode-core -p gamenode-app` and a UI test for both strings.
- [ ] 5.3 Replace `SteamAccountCard` with `StoreAccountCard` and render one card per snapshot account in `StoresPanel`.
  - Hide family rows when the provider has reported no family data.
  - Update `ui/src/api` wrappers.
  - Verify with Vitest tests for two providers, independent actions and the absence of family rows for Epic.
- [ ] 5.4 Make `en` and `pt-BR` account and error copy provider-neutral, with `{provider}` interpolation.
  - Add an Epic provider (Windows, catalog, no family) to `ui/src/mock/host.ts`.
  - Verify with `pnpm --dir ui test --run` and with `VITE_UI_MOCK=1 pnpm --dir ui dev` showing both cards.

## 6. Remove leftover Steam code from host crates

- [ ] 6.1 Map every scenario in `engine/tests/steam.rs`, `engine/steam_account.rs` tests, `engine/steam_login.rs` tests and `api/src/steam/tests.rs` to an existing or new test.
  - Targets are `engine/tests/provider_runtime.rs` and the `plugin-steam` runtime and HTTP tests.
  - Write any missing tests.
  - Done when the mapping table is in the PR description and every mapped test passes.
- [ ] 6.2 Delete the following:
  - `core/src/steam.rs` and its `lib.rs` export.
  - `core/src/provider/steam/**`.
  - `api/src/steam.rs`, `api/src/steam/**` and the `SteamClient` re-export.
  - `engine/steam_account.rs`, `engine/steam_login.rs` and `engine/tests/steam.rs`.
  - The `Engine.steam` field and `SteamPorts`.
  - Steam-only `reqwest` features in `gamenode-api`.
  - Verify with `cargo build --locked --workspace`, `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo test --locked --workspace`.
- [ ] 6.3 Update `docs/steam-web-session.md` to describe the flow as implemented in `crates/plugin-steam`, with no references to deleted host modules.
  - Done when every path the doc mentions exists.

## 7. Data-driven tools blocklist and boundary check

- [ ] 7.1 Move the anti-cheat app-id list to embedded `crates/core/data/anticheat.toml`, keyed by full entry key, and match on the entry key in `tools/anticheat.rs`.
  - Verify that the existing anti-cheat tests pass unchanged, and that a new test with an `epic:` key passes.
- [ ] 7.2 Extend `scripts/check-plugin-boundaries.py`:
  - Add `gamenode-plugin-epic` to the crate lists.
  - Add the production-source store scan with `scripts/plugin-boundary-allowlist.txt`, following design Decision 6.
  - Add `scripts/tests/test_plugin_boundaries.py` cases for a violation, an allowlisted line and test-only code.
  - Verify that the checker passes on the tree, and fails when a `steampowered` string is added to `crates/app/src`.

## 8. Epic plugin

- [ ] 8.1 Scaffold `crates/plugin-epic` as binary `gamenode-plugin-epic`, with the dependencies from design Decision 10. It opens an SDK session with plugin id `org.gamenode.plugin.epic` and provider `epic`.
  - Verify that the crate builds on Linux and that the public-dependency check passes.
- [ ] 8.2 Implement root resolution (registry `AppDataPath`, then `%ProgramData%` fallback) and manifest discovery with the filters from design Decision 10.
  - Report `namespace` and `catalog_item_id` fields.
  - Run blocking I/O in `spawn_blocking`.
  - Verify with fixture tests: two games, DLC, incomplete install, engine item, malformed JSON, a missing launcher (empty success) and an unreadable directory (failure).
- [ ] 8.3 Implement the Epic HTTP client covering the token exchange (authorization code and refresh), session revoke, library item paging, catalog bulk titles and playtime.
  - Use 20-second timeouts, rustls and secret-free errors that log only category, status and size.
  - Verify with scrubbed fixture tests for success, `invalid_grant`, 401, 429, 5xx, a malformed body and a multi-page cursor.
- [ ] 8.4 Implement the account flow:
  - Sign-in through the login broker, probe, then code exchange.
  - Store the refresh token in `session`.
  - Check by refresh with rotation written before publishing connected.
  - Expired, unreachable and credential-error transitions.
  - Sign-out with delete and best-effort revoke.
  - Verify with runtime tests against a fake host for each transition, for cancel and close, and for a late result after sign-out.
- [ ] 8.5 Implement the owned catalog sync:
  - Paging, title resolution with up to 8 concurrent requests and one retry, and the exclusion rules.
  - Fields on each game.
  - Whole-category failure on any unrecovered error.
  - No family category.
  - Verify with runtime tests for complete, empty, failing midway and DLC-excluded results, and that the run stays within a 300-second budget against a slow fake.
- [ ] 8.6 Implement the counter: playtime totals with an absent `last_played`, and no observations without an account.
  - Implement action declarations with the Play and Install templates.
  - Verify with runtime tests, and with core template tests that the filled URIs keep the `com.epicgames.launcher` scheme.
- [ ] 8.7 Add `crates/plugin-epic/assets/login.toml` (cookie-free, exact probe origin, allowed hosts), `providers/epic.toml` (Windows, plugin scan, catalog and playtime, `match = { external_category = <1.2 value>, uid_field = "namespace" }`) and `plugins/epic.toml` (grants and native access as in design Decision 10).
  - Verify with `cargo test --locked -p gamenode-app` that the descriptor loads on Windows and is absent on Linux.
- [ ] 8.8 Write `docs/epic-games-store.md`. It covers:
  - Discovery sources and filters.
  - The login flow and the supported login methods.
  - Credential storage.
  - The endpoints used and their unofficial status.
  - Matching by namespace.
  - Known limitations: social login, launcher required, Windows only.
  - Done when the doc matches the implementation and is linked from `docs/plugin-development.md`.

## 9. Packaging

- [ ] 9.1 Add the Epic executable to the Windows build, dev, package, install and uninstall scripts. Explicitly exclude it from the Linux scripts.
  - Uninstall removes the `provider.epic` credential only on the existing user-data removal path.
  - Update `scripts/verify-plugin-package.py` and its tests so Windows requires the executable and Linux forbids it.
  - Verify with `python3 -m pytest scripts/tests` and a `scripts/build-windows-wsl.sh` cross-build that produces `gamenode-plugin-epic.exe` next to the host.

## 10. Integration checks

- [ ] 10.1 Run `cargo test --locked --workspace`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo xwin test --workspace --target x86_64-pc-windows-msvc --locked --no-run`, `pnpm --dir ui test --run`, `pnpm --dir ui build`, the boundary and package checkers, and `openspec validate add-epic-games-store-plugin --strict`.
  - Done when all pass.
  - Record that cross-built Windows tests were linked but not executed.
- [ ] 10.2 Linux regression on a copied profile:
  - Steam sign-in restore from the existing credential slot, discovery, owned/Family sync, playtime and Play.
  - PackageKit discovery.
  - Verify that no Epic UI appears and no credential or data row changed.
  - Record the evidence in the PR.
- [ ] 10.3 Ask the user to confirm Windows VM availability, then install the Windows candidate and accept the Epic flows:
  - Discovery with multiple games and DLC.
  - Play and Install via the launcher.
  - Sign-in with 2FA, cancel and close.
  - Restart restore and rotation.
  - Sign-out.
  - Owned catalog, including a large library within the timeout.
  - Namespace matching.
  - Store-source playtime submission with unknown last played.
  - A missing executable.
  - An Epic crash while Steam keeps working.
  - Done when each scenario has recorded evidence, or is listed as unresolved.
- [ ] 10.4 On the same Windows candidate, run Steam regression: Credential Manager restore, sign-in, catalog, playtime and actions. Also run the PC ports and tools smoke tests.
  - Done when the results confirm parity with the pre-change build, recorded in the PR.

## Workflow follow-up

- Sync the delta specs and archive after `integrate-steam-plugin-into-main` is archived, so that the `desktop-plugins` and `desktop-plugin-services` requirements append to the main specs created by that change.
