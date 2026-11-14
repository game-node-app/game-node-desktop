# Tasks

## 1. Steam contract confirmation

- [x] 1.1 Using a real Steam session, confirm the two endpoints in design D2 accept the derived token, and record sanitized response shapes for a normal game, a game with no stats, a private profile and an unlocked achievement with `unlocktime` 0 in `docs/steam-web-session.md`; verify the document contains no token, cookie or SteamID.

## 2. Protocol and plugin host

- [x] 2.1 Add capability `achievements.read`, `ACHIEVEMENTS_API_MINOR = 4`, the operation, request and per-game outcome messages in `crates/plugin-protocol`; verify negotiation tests show an API 1.3 plugin is not granted the capability.
- [x] 2.2 Expose achievement reads through `crates/plugin-sdk`; verify an SDK contract test round-trips ready, none and failed outcomes.
- [x] 2.3 Broker the operation in `crates/app/src/plugin_host`, enforcing the grant, requested-id check, batch and string limits, and account fencing; verify permission tests reject each case with nothing delivered to the engine.
- [x] 2.4 Add the `achievements` manifest key with validation in `crates/core/src/provider` and register the Steam backend in `providers/steam.toml` and trusted host metadata; verify registry tests cover a valid plugin, an unregistered plugin and a provider without the key.
- [x] 2.5 Document the capability in `docs/plugin-development.md`; verify the capability table matches `capability.rs`.

## 3. Steam plugin

- [x] 3.1 Implement the definitions and unlock requests in `crates/plugin-steam/src/http.rs` using the shapes from 1.1 and the existing retries and error categories; verify HTTP fixture tests cover success, no stats, privacy, expiry, rate limiting and an unexpected shape.
- [x] 3.2 Join definitions and unlocks into per-game outcomes, map the locale to Steam's language and handle the operation in the runtime and account code; verify tests cover partial failure leaving a failed outcome, zero `unlocktime`, cancellation, no connected account, and that no file under `appcache/stats` is accessed.

## 4. Local storage and core rules

- [x] 4.1 Add migration `0009_achievements.sql` and its upgrade fixture; verify a populated schema 8 profile upgrades with existing data intact.
- [x] 4.2 Add the achievement model and store port in `crates/core` and the SQLite adapter in `crates/platform`; verify tests cover atomic per-game replacement, failures preserving data, removed definitions, the `none` state and account-scoped reads.
- [x] 4.3 Implement ordering, hidden masking and staleness rules in `crates/core`; verify unit tests cover list order including unlocks with no time, masking of locked hidden achievements, and each staleness threshold.
- [x] 4.4 Delete achievements in the Steam sign-out and account-replacement flows; verify engine tests show deletion on both, and retention on expiry and unreachability.

## 5. Engine scheduling and commands

- [x] 5.1 Implement the per-provider worker, priority queue, pacing and batch dispatch in `crates/app/src/engine`; verify tests show one batch at a time, selected games taking priority, and fresh games skipped after a restart.
- [x] 5.2 Wire the selection, game-exit, import and reconciliation triggers; verify tests cover the settling delay, the staleness rules and a language change marking a selected game stale.
- [x] 5.3 Handle rate limiting, expiry, unreachability and privacy outcomes; verify tests show the backoff pause, the worker stopping on expiry and resuming on reconnect, cached data retained, and error text free of secrets.
- [x] 5.4 Publish the summary in the detail snapshot and add the async list command; verify engine tests check the summary fields, and that an entry with no GameNode match and no GameNode network still returns data.
- [x] 5.5 Add an engine test with Sync with GameNode on that runs an achievement refresh; verify the GameNode client receives no achievement request.

## 6. Icons

- [x] 6.1 Add the trusted host list, size-capped off-thread icon download and on-disk cache; verify tests reject non-HTTPS URLs, untrusted hosts, redirects off the trusted hosts and oversized bodies.
- [x] 6.2 Add the `achievement` route to `crates/app/src/protocol.rs` and include icons in cache usage and clearing; verify protocol tests cover a cached icon, a missing icon and path traversal input, and a settings test shows clearing removes icons.

## 7. User interface

- [x] 7.1 Add the host types, command binding and summary and list fixtures to the UI API layer and `ui/src/mock/host.ts`; verify `VITE_UI_MOCK=1` serves them and mock tests pass.
- [x] 7.2 Add the achievements section to `DetailPane` for ready, none, loading, failed, privacy and not-connected states; verify component tests cover each state and that entries without a backend show no section.
- [x] 7.3 Build the `FloatingModal` list with virtualized rows, filters, hidden placeholders and lazy icons with placeholders; verify tests cover ordering, filters, keyboard opening, Escape with focus restoration, and a game with hundreds of achievements rendering only visible rows.
- [x] 7.4 Add English and Brazilian Portuguese strings; verify the catalog tests pass and the views render in both languages.

## 8. Integration

- [x] 8.1 Run `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `pnpm --dir ui test --run` and `python3 scripts/check-plugin-boundaries.py`; verify all pass.
- [ ] 8.2 On a Windows build with a real Steam account, confirm import, the post-exit update after unlocking an achievement, offline display with cached icons, sign-out clearing, and a responsive UI during import; record the result in `docs/windows-verification.md`.
- [ ] 8.3 Inspect network traffic or logs during 8.2 and verify no achievement data is sent to GameNode.
