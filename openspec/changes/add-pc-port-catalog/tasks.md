# Tasks

## 1. Spikes

- [x] 1.1 Read Lantern Engine's launcher source and find out how it locates the game folder: config file, first-run picker or fixed relative path. Record the findings and the resulting overlay recipe in `docs/pc-ports.md` (D12). Done when that section names the mechanism and the recipe GameNode will use. Task 10.1 confirms it on Windows.
- [ ] 1.2 Evaluate PS4 PKG extractors (CLI, Windows build, open licence, base plus update merge, clear errors for encrypted packages). Pick one and record its repository, tag, asset, SHA-256, licence and argument template in `docs/pc-ports.md`. Done when a base and update package from a dump the developer owns extract to a folder that has the slot's markers and that Lantern Engine accepts.

## 2. Shared archive and download code

- [x] 2.1 Move archive detection, `validate_archive_entry_path`, the extracted-size cap and the zip and 7z extraction loops from `crates/platform/src/tools/catalog.rs` into `crates/platform/src/archive.rs`, with a cap the caller can set (D6). Verify that the existing game-tools tests pass unchanged with `cargo test --locked -p gamenode-platform`.
- [x] 2.2 Add support for bare `.exe` assets and a ports extraction cap of 16 GiB. Add tests for traversal, absolute paths, escaping links, the size cap and the bare `.exe` copy, and verify they pass.
- [x] 2.3 Generalize the cached HTTPS download with SHA-256 verification so ports and extractors can use it with their own cache roots (`<data_dir>/ports/cache/...`, `<data_dir>/ports/tools/...`). Verify with tests that a mismatched download is deleted and that a cached verified file is reused.

## 3. Local state

- [x] 3.1 Add migration `0008_ports.sql` (`port_catalog_snapshot`, `port_overlay`, `port_install_jobs`, `port_installations`, `port_preferences`). Verify with a migration test that upgrading a favorites-schema fixture keeps every existing table and value, and that an older build refuses the new schema.
- [x] 3.2 Implement the SQLite port store: an atomic snapshot replace, overlay storage, jobs whose created paths are journaled before each write, provenance rows that survive registration removal, and preferences. Verify with store tests, including the restart-offline and another-account scenarios.

## 4. Catalog domain and upstream client

- [x] 4.1 In `crates/core/src/ports/`, define the catalog model, the overlay schema (`schema_version`, `extractor`, `entry`, `setup_file`, `from_package`, `hidden`) and the merge rules (D4). Verify with unit tests for recipe replacement, hidden entries, executable roles and the Windows-only filter.
- [x] 4.2 Add `ports/overlay.toml` with the Lantern Engine entry from task 1.1 and the extractor from task 1.2, embed it at build time, and add the remote ETag refresh with validation fallback. Verify with tests that an invalid or unknown-major overlay keeps the previous valid one.
- [x] 4.3 Implement the Quiver client in `crates/platform/src/ports/quiver.rs` from the public OpenAPI description, using only GET requests: listing paging, app detail, releases, User-Agent, and `Retry-After` handling with backoff (D2, D3). Verify with tests that run against small trimmed responses recorded into `crates/platform/tests/fixtures/quiver/`, covering unknown fields, skipped entries, the 20 % skip threshold, a truncated listing and 429 handling.
- [x] 4.4 Implement the catalog worker: first fetch on first visit, daily refresh, manual Refresh, swap only after validation, and the release cache that lasts 15 minutes. Verify with tests using a fake clock and a fake source that a failed refresh keeps the old snapshot and that the daily refresh fires.
- [x] 4.5 Add the `port-art` route to the `gamenode-asset://` protocol. It resolves URLs only from the snapshot, fetches in a worker and caches on disk (D9). Verify with protocol tests that an unknown catalog id returns 404 and that arbitrary URLs cannot be fetched.

## 5. Install engine

- [x] 5.1 In core, define `PortInstallPlan` and the job state machine (D5): asset selection with the filter, OS and architecture, the folder-name rule, a non-empty target check, a space estimate, and the registration template with launcher, gameplay and arguments. Verify with unit tests for verified, withdrawn, unverified-checksum and no-asset entries.
- [x] 5.2 Implement supplied-file validation (D7): a single pass computing SHA-1 and SHA-256 with progress, folder `contains` markers, `hash_file`, and messages listing the accepted versions. Verify with tests for the accepted, wrong-version and optional-slot cases.
- [x] 5.3 Implement placement: verified copies to `app`, `data`, `config` and `localData` roots through temporary names, and `flag`, `positional` and `env` handling. Add an optional environment map to the DRM-free launcher. Verify with tests that originals are untouched, that `--iso=` joins the path to the flag, and that the user's own arguments come after the recipe arguments.
- [ ] 5.4 Implement the extractor runner (D8): fetch the pinned tool, no shell, `CREATE_NO_WINDOW`, a job object, placeholder substitution with validated paths, captured output with a cap, and validation of the output folder. Verify with a test that uses a stub executable to check argument passing, failure cleanup and cancel kill (the Windows-only parts run under `cargo xwin test`).
- [x] 5.5 Implement the job runner: staging folder, journaled paths, a single worker with a queue, cancellation between chunks, the finalizing rename, and registration plus completion in one transaction. Verify with tests that cancel and failure leave no files and no entry.
- [x] 5.6 Implement startup recovery: remove the recorded paths of unfinished jobs, keep cached downloads, mark jobs `Interrupted`, and keep jobs whose registration was committed. Verify with tests that crash at each phase boundary.
- [x] 5.7 After completion, feed the overlay's `gamenode_game_id` or the upstream title to the local matcher (D11). Verify with matching tests for the overlay-id, no-overlay and user-choice-wins scenarios.

## 6. App wiring

- [x] 6.1 Add async Tauri commands: `ports_open` (start the first fetch), `ports_refresh`, `ports_detail`, `ports_pick_parent`, `ports_pick_supplied`, `ports_install`, `ports_cancel`, `ports_mark_seen`, `ports_set_preferences` and `ports_pick_executable`. Add them to the capability scope. Verify with `cargo test --locked -p gamenode-app` command tests showing that no command blocks the main thread.
- [x] 6.2 Add `PortsView` to the `engine:snapshot` payload (catalog status and age, entries, provenance by registration key, job phases and results, unseen count) and add the coalesced `ports:progress` event. Verify with snapshot tests, and check that progress ticks do not republish the snapshot.
- [x] 6.3 Add the port provenance line and its PC ports link to the DRM-free detail pane data. Add the install folder and Open folder action to the removal confirmation. Verify with app tests that removal keeps the files and the provenance row.

## 7. UI: navigation and catalog

- [x] 7.1 Add `activeDestination` to `uiStore` and the `EXPLORE` group with the PC ports item to `Sidebar.tsx`. Show it only on Windows signed-in builds, with one active item at a time, an accessible badge and keyboard order. Update the shell tests for switching, returning with the filter preserved, the badge count, keyboard activation and the Linux case. Verify with `pnpm --dir ui test --run`.
- [x] 7.2 Build `features/ports`: the toolbar (search, console filter, guided, verified, installed, grid or list, Refresh), virtualized grid and list using the library tile treatment and `port-art` covers, the empty, unavailable, refreshing and stale states, and the credit line. Verify with Vitest for filtering, the unavailable state with Retry, the credit text being present with no Quiver image, and remembered preferences.
- [x] 7.3 Build the port detail pane: art, game, console, project and source link, licence when the overlay has one, release and verification badges, the requirement list, the Install or Open in library action with a reason caption when disabled, the update-available notice and the install progress row. Verify with Vitest for the installed, withdrawn, no-asset and guided states.
- [x] 7.4 Build the guided setup `FloatingModal`: confirm the release, choose the parent folder, add a slot for each supplied file (including the package option), show hashing progress and rejections, confirm unverified or flagged files, then start. The modal can be closed while the job keeps running. Verify with Vitest for blocking required slots, skipping optional ones and the confirmation gates, plus a focus and Escape test using the shared modal stack.
- [x] 7.5 Add the completion and failure toasts, the "pick executable" follow-up, failure details with the tool output, and Retry for interrupted jobs. Verify with Vitest.
- [x] 7.6 Add the English and pt-BR strings for every new surface, and make sure the layouts fit the longer Portuguese text. Verify with the i18n catalogue tests and a pt-BR render test.
- [x] 7.7 Extend `ui/src/mock/host.ts` with a fictional catalog: guided, plain, pre-release, withdrawn and installed entries, plus a simulated install with progress, cancel and failure. Document the mock in `docs/development.md`. Verify by checking the flows in `VITE_UI_MOCK=1 pnpm --dir ui dev`.

## 8. Settings

- [x] 8.1 Extend the Tools panel with the helper-tools group (extractor licence, source, pinned version and cache size), the ports download cache size, and Clear cache, which is refused while an install runs. Verify with settings Vitest and with a platform test showing that installed ports are untouched.

## 9. Documentation

- [x] 9.1 Write `docs/pc-ports.md`: the catalog source and the text-only credit, accepted API risk and offline behaviour, the overlay format and authoring guide, the guided setup and extraction flow, safety rules (verified, withdrawn, checksums, no content distribution), the self-updating port policy and the deferred Linux scope. Link it from `docs/drm-free-games.md`. Done when the overlay example in the doc parses with the overlay loader test.

## 10. Integration

- [ ] 10.1 Windows acceptance with `scripts/install-windows-wsl.sh`: install a release-only port, a guided port whose upstream recipe lists a file hash, and Lantern Engine from a folder and from packages. Confirm the library entries launch, playtime follows the gameplay executable, cancelling and quitting mid-install leave nothing behind, and removing an entry keeps its files. Record the results in `docs/windows-verification.md`.
- [ ] 10.2 Run `cargo test --locked -p gamenode-app`, `cargo xwin test --workspace --target x86_64-pc-windows-msvc --locked`, `pnpm --dir ui test --run` and `openspec validate add-pc-port-catalog --strict`, and confirm that all of them pass.

## Workflow follow-up

- Archive the change after implementation and review, then verify the synced specs.
