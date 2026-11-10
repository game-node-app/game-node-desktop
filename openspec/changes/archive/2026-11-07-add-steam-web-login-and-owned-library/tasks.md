# Tasks

## 1. Core catalog model

- [x] 1.1 Add `owned` and `family_shared` to `StoredEntry`, `Row`, `ProviderCount` and `LibraryView`, and add `CatalogRecord`, catalog error/outcome types, `Input::CatalogSynced`, `Input::CatalogCleared` and `Effect::SyncCatalog`. Verify `cargo test --locked -p gamenode-core` compiles after the type changes.
- [x] 1.2 Implement `merge_catalog` and `clear_catalog` in `LibraryMachine`: replace only the owned or family set, let owned win over family, preserve `first_seen`, `install_dir`, matches and covers, and guard results by `op`. Verify core tests cover owned-only, installed-only, owned+installed, family+installed, owned+family, failed owned, failed family and cleared account cases with `cargo test --locked -p gamenode-core`.
- [x] 1.3 Switch visibility, rows, provider counts, `installedTotal`, lookups, covers and playtime to `visible_entries()`, keeping `installed_entries()` for tracking-only work. Verify core tests show owned-only rows, correct visible/installed/owned/family counts, lookup eligibility for owned-only games, installed-first cover queuing and unknown playtime for owned-only entries.
- [x] 1.4 Add `installed_only` to `LibraryLayout` (serde default false), `Input::SetInstalledOnly`, the composed filter and the `NoInstalled` empty state. Verify core tests cover filter/state composition, filter-aware counts, the empty state, and loading a layout saved before this field existed.
- [x] 1.5 Add `catalog = { native = "steam" }` to `providers/steam.toml`, validate unknown catalog backends, expose `Provider::has_catalog`, and update `docs/providers.md` with the catalog schema and Steam records. Verify `cargo test --locked -p gamenode-core` and `cargo test --locked -p gamenode-app` cover the bundled manifest.
- [x] 1.6 Extend the `LibraryStore` trait with catalog save/clear and store-account load/save/clear methods, update any core test doubles, and verify `cargo test --locked -p gamenode-core` passes.

## 2. Platform persistence and credentials

- [x] 2.1 Add migration `0006_store_accounts_and_catalog.sql` with `library_entries.owned`, `library_entries.family_shared`, the four catalog columns on `provider_scans`, and the `store_accounts` table; bump `CURRENT_SCHEMA_VERSION` to 6. Verify a migration test upgrades a v5 database with entries, matches, covers and playtime and keeps them intact.
- [x] 2.2 Implement SQLite `load_cache`, `save_scan`, new `save_catalog`, `clear_catalog`, `load_store_accounts`, `save_store_account` and `clear_store_account`. Verify platform tests cover owned/family replacement, failed phases, owned-wins, name precedence, account rows, and that no cookie or token is written to the database.
- [x] 2.3 Make credential storage slot-aware: add `CredentialSlot`, route `open_store(paths, slot)`, add the Steam keyring account and `private/steam-token`, and add `DataPaths` helpers for the Steam credential file and the login profile directory. Verify platform tests cover separate slots, separate deletes, 0700/0600 permissions, symlink refusal and unchanged GameNode session behavior.
- [x] 2.4 Run `cargo test --locked -p gamenode-platform` and `cargo clippy --locked -p gamenode-platform --all-targets -- -D warnings`.

## 3. Steam web client

- [x] 3.1 Add the `SteamWebApi` port and shared types (`SteamSession`, `SteamOwnedGame`, `SteamFamily`, `SteamError`) without carrying the cookie or token in errors or `Debug`. Verify `cargo test --locked -p gamenode-core` and `cargo check --locked -p gamenode-api`.
- [x] 3.2 Implement the store-session request: authenticated store page, `#application_config` extraction, HTML-entity decode, `data-userinfo`/`data-store_user_config`, rotated-cookie capture, login redirect detection, 4 MiB body cap and the `ajaxgetasyncconfig` fallback. Verify httpmock tests cover logged-in, logged-out, rotated cookie, entity decoding, missing attributes and oversized bodies.
- [x] 3.3 Implement owned games and Steam Family: `GetOwnedGames` with app info, played free games, free-sub off and unvetted apps on; family group lookup; shared apps with `include_own=false`, excluded/free/non-game filters; no-group success; name cleanup, dedupe, `exclude_reason` handling and the bounded 429/5xx retry policy. Verify httpmock tests cover normal, empty, private, family, no-family, excluded, rate-limited and 5xx responses.
- [x] 3.4 Add a secret-free diagnostics test that asserts no cookie, token, full URL or page body appears in error strings or tracing output, and write `docs/steam-web-session.md` documenting the endpoints, parameters, cookie names, token lifetime and breakage behavior. Verify `cargo test --locked -p gamenode-api` and read the documented requests against the implementation.
- [x] 3.5 Run `cargo test --locked -p gamenode-api` and `cargo clippy --locked -p gamenode-api --all-targets -- -D warnings`.

## 4. App engine and Steam login window

- [x] 4.1 Add a `SteamAccount` service that owns status states, credential load/save/delete, account identity, last check/sync times, persistence failures and one-at-a-time checks/syncs. Verify unit tests cover every state transition, coalescing, save failure, expired, unreachable and sign-out using fake ports.
- [x] 4.2 Add the `SteamLoginWindow` port and the Tauri implementation: `steam-login` window, Steam URL, 600x760 size, private data directory, Steam-only navigation allowlist, cookie polling from Rust, cancel/close/timeout, attempt generation counter and profile cleanup. Verify app tests cover cancel, close, timeout, late result, capability isolation and no IPC exposure, and inspect `crates/app/capabilities/default.json` stays scoped to `main`.
- [x] 4.3 Wire the engine: load the credential and `store_accounts` row at startup, validate before catalog sync, handle rotated cookies and account changes, clear the catalog before applying a different account, sign out, coalesce `Effect::SyncCatalog`, and apply `CatalogSynced`/`CatalogCleared` to the library machine. Verify engine tests cover startup restore, sign-in success/cancel, expired, unreachable, persist failure, account switch, sign-out and a result from a superseded attempt.
- [x] 4.4 Expose `storeAccounts` in the snapshot and add `steamSignIn`, `steamCancelSignIn`, `steamFocusSignIn`, `steamSignOut` and `steamCheck` commands, all dispatched off the UI thread. Verify app tests assert the snapshot has no secret and include every status; run `cargo test --locked -p gamenode-app`.
- [x] 4.5 Run `cargo test --locked -p gamenode-app` and `cargo clippy --locked -p gamenode-app --all-targets -- -D warnings`.

## 5. Settings Stores UI

- [x] 5.1 Extend the UI API types, command wrappers and mock snapshot for `storeAccounts`, the new Steam commands and not-connected/expired/unreachable fixtures. Verify `pnpm --dir ui test --run` still passes and the TypeScript build accepts the new types.
- [x] 5.2 Build the Steam account card with the status pill, identity, owned/shared/installed counts, last check, storage method, family warning and every state/action combination from the settings spec. Verify component tests cover connected, not connected, checking, signing in, expired, unreachable, credential error, persist failure and save warning.
- [x] 5.3 Add the plain-text private-file warning for the Linux fallback before sign-in and while connected, plus the "GameNode never sees your Steam password" note. Verify tests cover keyring vs private-file storage.
- [x] 5.4 Add the Stores attention indicator to the settings navigation and make it disappear on recovery or sign-out, with an accessible name. Verify tests cover expired/credential-error, recovery, sign-out and screen-reader naming.
- [x] 5.5 Add toasts for Steam connected (with imported count) and signed out (installed games stay), preserving focus. Verify tests cover both toasts and the existing backup/rescan toasts.
- [x] 5.6 Restructure `StoresPanel` around the Steam card, the other detected stores and the existing DRM-free section, keeping global Rescan, playtime source and reset-hidden-games behavior. Verify existing settings tests still pass and new tests cover the layout without a GameNode session.
- [x] 5.7 Add all new English and Brazilian Portuguese strings and verify `pnpm --dir ui test --run` includes the locale coverage without changing store names, account names or paths.

## 6. Library UI

- [x] 6.1 Add the visible Installed filter chip to the library toolbar, wired to `setInstalledOnly`, with `aria-pressed`, a count and persisted state. Verify UI tests cover toggling, the active style, keyboard use and restart persistence.
- [x] 6.2 Make sidebar counts follow the active filter and show a muted "X installed" subline when a provider has owned or shared games beyond the installed ones. Verify tests cover the full, filtered and no-account cases.
- [x] 6.3 Show installed and Steam Family indicators in the grid and list without relying on color, and hide the family label when the same app is owned. Verify tests cover installed, owned-only, family-shared and owned+shared rows.
- [x] 6.4 Replace the install-folder row with "Not installed on this PC" for owned-only entries, show the Steam Family label in details, and keep Play/Install available. Verify tests cover owned-only, family-shared, installed and DRM-free details.
- [x] 6.5 Add the expired and unreachable Steam banners with a "Fix in Settings" action that opens Settings > Stores, without blocking library interactions. Verify tests cover both states, the action and recovery.
- [x] 6.6 Add the `NoInstalled` empty state with "Show all games", and keep the existing no-games and nothing-matches states distinct. Verify tests cover no installed games with owned games, a truly empty library and a search with no results.
- [x] 6.7 Verify the whole library flow in `VITE_UI_MOCK=1 pnpm --dir ui dev`: 300+ owned games, 18 installed, family-shared games, expired banner, Installed filter, detail pane and restart persistence; capture any layout regressions before closing the task.
- [x] 6.8 Run `pnpm --dir ui test --run` and `pnpm --dir ui build`.

## 7. Integration and acceptance

- [x] 7.1 Update `docs/mvp-roadmap.md` so "installed games only" and the "Later" store-login entry reflect the new behavior and Steam as the first account provider. Verify the roadmap no longer says owned-but-not-installed games are out of scope.
- [x] 7.2 Run `cargo test --locked --workspace`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `pnpm --dir ui test --run` and `pnpm --dir ui build`.
- [ ] 7.3 On Windows, sign in to Steam with Steam Guard, import a real library, verify owned/shared/installed counts, install filter, Play/Install on an uninstalled game, restart restore, sign-out and re-sign-in. Record the result in the change notes.
- [ ] 7.4 On Linux, verify WebKitGTK login, Secret Service storage, the private-file fallback warning and permissions when no Secret Service exists, offline startup, expired-session recovery and the installed filter. Record the result in the change notes.
- [ ] 7.5 Verify family behavior with a real Steam Family: shared games appear, owned games win, leaving the group clears shared entries and keeps installed ones. Record the result in the change notes.
- [ ] 7.6 Verify the 300+ game first sync keeps the UI responsive, covers download installed-first, and restart renders the cached catalog before the network finishes. Record the result in the change notes.

## Workflow follow-up

- Archive the change after implementation and the project's review requirements are satisfied.
- Verify the archived specs render the new `desktop-store-accounts` capability and the modified requirements correctly.

## Reconciliação documental do histórico — 2026-11-09

As quatro tarefas manuais 7.3–7.6 permanecem abertas. O [registro de aceitação](../../../../docs/steam-acceptance.md) as descreve como não executadas ou parcialmente verificadas; esta reconciliação não altera esses resultados. Testes/mock e resultados de uma revisão antiga não comprovam aceitação integral do plugin no candidato atual.

| Tarefa original | Evidência e acompanhamento sucessor | Destino e condição restante |
| --- | --- | --- |
| 7.3 | O registro de aceitação exige Windows, Guard, credenciais, import/restart/sign-out e Play/Install reais. | Correspondência com [integração Steam](../../integrate-steam-plugin-into-main/tasks.md) 9.2 e critérios de UI/library aplicáveis. A tarefa histórica permanece aberta até resultados explícitos cobrirem seu conjunto completo. |
| 7.4 | Fallback privado, offline e recuperação têm fixtures; login WebKitGTK e keyring reais continuam distintos dessa evidência. | Correspondência com integração Steam 8.1–8.2, preservando a distinção Secret Service/fallback e seus resultados por candidato. |
| 7.5 | Fixtures cobrem grupo/no-group e owned-wins; sair de uma Steam Family real e observar revogação não foi estabelecido no registro. | Integração Steam 8.2/9.2 cobre owned/Family de forma geral; a transição real de saída/revogação continua com este ID até ser explicitamente verificada. |
| 7.6 | Mock de biblioteca grande e testes de cache-first não comprovam primeiro sync de conta real, prioridade installed-first nem restart durante esse fluxo. | Integração Steam 8.3 cobre responsividade geral; a medição real de 300+ jogos e a matriz específica continuam nesta tarefa histórica, sem sucessora ativa completa. |

A [integração Steam](../../../../docs/steam-plugin-integration.md) é o registro do candidato sucessor para os testes que realmente repetir. As referências acima são correspondências de escopo, não transferência formal nem conclusão. Nenhum checkbox foi alterado.
