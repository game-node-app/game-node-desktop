# Tasks

Prerequisite: `add-playtime-background` must be completed and its specs synced/archived before applying this change. Its `desktop-playtime` requirement is the baseline for this change's MODIFIED delta. Do not implement or archive that separate change as an incidental task here. Implementation is paused at the user's request. See the checkpoint below for completed work, partial implementation and verification limits.

## 1. Baseline and pinned backup contract

- [x] 1.1 Confirm the prerequisite is complete, `desktop-playtime` exists in main specs and the tracker/storage baseline matches the design; verify this change's strict validation and archive applicability against that baseline before editing application code.
- [x] 1.2 Add reproducible release inputs for Ludusavi v0.31.0 Windows x64, the design's archive SHA-256 and a pinned primary-manifest revision/hash. Record upstream URLs/notices and dataset provenance; verify digest failure rejects a modified asset and the release contract contains the required CLI options.
- [x] 1.3 Add isolated Windows helper contract fixtures for file saves under `<base>`, renamed installs, appdata/document saves, registry saves, empty saves and custom files. Verify preview/backup/history/selected-version restore JSON with the pinned helper without reading or writing real game saves.

## 2. Core models and durable local state

- [x] 2.1 Add OS-neutral registration, evidence, candidate, process-policy, save-profile and backup-job models/ports in `core`, plus scripted doubles. Verify state transitions can be exercised without OS calls and Linux core builds introduce no UI or platform dependency.
- [x] 2.2 Add the next transactional migration after step 3 (expected `0004_drm_free_backups.sql`) and platform repositories for registrations/tombstones, watched roots/candidates/ignores and inferred catalog provenance. Verify upgrade preserves Steam/match/playtime data, duplicate canonical registrations are prevented, repair retains UUIDs and migration failure rolls back.
- [x] 2.3 Persist save profiles/revisions, job identities/results, historical destinations and backup settings; extend the settings allowlist. Verify interrupted jobs survive reopening, profile changes invalidate old queued jobs, GameNode choices remain account-specific and OS-local save protection survives account changes without sharing playtime.
- [x] 2.4 Document tables, ownership, backup layout, unavailable registrations and recovery in `docs/local-state.md`; verify field names match the migration and the newer-schema refusal/keep-data uninstall rules remain documented accurately.

## 3. Manual provider and executable launch

- [x] 3.1 Embed a Windows-only `providers/manual.toml` and native scanner projecting approved registrations into `Record`s with source `manual`, no counter and no Install. Verify stable `manual:<uuid>` keys, unavailable-entry retention and absence of pending candidates from scanner output.
- [x] 3.2 Add tagged URI/manual-executable launch declarations and action validation in `provider::Manifest`/registry and `library::launch`. Verify all Steam URI tests still pass, non-manual executable declarations fail, and missing manual executables offer repair instead of Install.
- [x] 3.3 Implement Windows canonical-path/file-identity, executable validation, launch and working-directory adapters with an argument vector and no shell. Verify spaces, Unicode, literal shell metacharacters, junction aliases/escapes, invalid targets and failed process starts using isolated executable fixtures; Linux adapters return Unsupported.
- [x] 3.4 Implement Add/repair/edit/remove commands on workers and persist-before-publish sequencing. Verify picker cancellation writes nothing, one selection adds a playable offline entry, duplicates select the existing entry, repair keeps history and removal never deletes installations/saves/archives.
- [x] 3.5 Update `docs/providers.md` for the manual scanner/action and source semantics; verify its documented manifest examples load under the validator and existing Steam examples remain valid.

## 4. Executable evidence and first-attempt matching

- [x] 4.1 Implement bounded Windows PE title inspection behind a port and the pure conservative title-evidence pipeline. Verify meaningful product/file/folder names, generic engine/launcher names, installers and conflicting evidence with fixtures; no file is executed for identification.
- [x] 4.2 Implement the tolerant primary-manifest identity index with canonical titles, aliases, `installDir`, optional launch hints and external ids. Verify alias cycles/caps, absent fields, unknown fields, edition collisions and manifest folder hints without a matching executable do not establish unwarranted confidence.
- [x] 4.3 Extend `GameApi` search results to retain completion/truncation information and implement unique external-id or conservative exact-title automatic matching. Verify one strong exact result matches, ambiguous/full/unknown pages remain suggestions, sequel/edition distinctions survive normalization, and id hints do not change the manual source/provider.
- [x] 4.4 Store automatic local-evidence matches/provenance separately from `auto_matches` store ids and wire cached/offline retries with entry/evidence/account generations. Verify chosen/no-match overrides, lookup failure, repair/removal during lookup and account switching preserve the correct match.
- [x] 4.5 Document evidence precedence and correction behavior in a DRM-free user guide; verify its quick-add and Change match instructions work with both successful and ambiguous fixture matches.

## 5. Reliable process tracking and physical exits

- [x] 5.1 Extend process information/Windows adapters for canonical executable ownership and observable parent identity while keeping PID/creation-time guarantees. Verify PID reuse, inaccessible processes and parent reuse with doubles, plus an isolated real Windows child-process check.
- [x] 5.2 Extend tracker policies for explicit manual gameplay bindings and launcher roles; preserve store folder matching. Verify external manual starts, two installs, shared-folder executables, deepest-root precedence, existing store/manual conflicts and one-owner attribution without duplicate playtime.
- [x] 5.3 Implement the bounded ten-second handoff and persist positively identified gameplay targets. Verify a bootstrap gap, replacement game process, persistent launcher, surviving crash reporter, installer-only activity, unknown outside-root child and externally started learned executable.
- [x] 5.4 Add physical play-period identity and exactly-once stopped events separately from session closure. Verify duplicate exit/listing events, failed enumeration, sign-out, source changes, shutdown, 24-hour splitting and checkpoint recovery do not fabricate game exits; actual gameplay timestamps exclude settling delay.
- [x] 5.5 Feed policies/events through `engine/playtime.rs` on workers and keep measured manual sessions local with platform 6. Verify hidden-window tracking, matched/unmatched sessions, crash/restart while the game runs and no measured-session HTTP submission.
- [x] 5.6 Document polling, elevated/uninspectable processes, outside-root launchers and edit-tracking recovery in the DRM-free guide and Windows checklist; verify examples distinguish known limitations from supported handoff behavior.

## 6. Reviewed watched-folder discovery

- [x] 6.1 Implement Windows directory-change notifications plus startup/rescan/overflow/reconnect reconciliation, one-second debounce and bounded worker traversal. Verify event bursts coalesce, cancellation stops work, traversal caps report incomplete results, and overlapping roots/reparse cycles cannot duplicate or escape discovery; Linux returns Unsupported.
- [x] 6.2 Implement positive-evidence candidate classification and one candidate per installation with unresolved launch targets. Verify a corpus containing actual game layouts, engine tools, installers, uninstallers, redists, crash reporters, renamed folders and multiple bitness executables; extension/fuzzy-folder-only cases never become candidates.
- [x] 6.3 Implement review Add selected/Ignore/reset, stable ignore fingerprints, watch removal and registration tombstones. Verify no candidate affects library/tracker/backups before approval, ignored candidates survive restart, incidental files do not resurrect ignores and unplugging a drive preserves registrations.
- [x] 6.4 Add folder-watch setup and review instructions to the DRM-free guide; verify a new `F:\NonSteamLibrary`-style fixture root can be watched, reviewed, ignored and rescanned without automatic additions.

## 7. Manifest lifecycle and isolated backup adapter

- [ ] 7.1 Bundle the baseline manifest and implement cached daily ETag updates from the fixed source with size/alias bounds and atomic validation/activation. Verify offline fallback, 304, corrupt/oversized download, failed update and a job retaining its original snapshot while another update completes.
- [ ] 7.2 Implement independent save-profile resolution and manual file/folder setup, scoped preview and revision changes. Verify unsupported/no-saves/ambiguous states, wrong catalog rematch retaining its old save profile, explicit retarget preserving history and profile overlap detection between installations.
- [ ] 7.3 Generate private single-profile execution manifests/config with exact-install binding, empty roots/secondary lists and OS/user evaluation delegated to Ludusavi. Verify the section 1 fixtures for spaces/glob characters, renamed roots, store constraints and registry saves; a game-local `.ludusavi.yaml` and a second install cannot expand the backup scope.
- [ ] 7.4 Implement `BackupEngine` subprocess calls with absolute bundled path, isolated config, explicit one-game selection, no shell/cloud/implicit updates, bounded output/time and child cancellation. Verify empty title refusal, title-as-option handling, missing/incompatible helper, non-JSON stdout, nonzero status and per-file/registry partial failure.
- [ ] 7.5 Implement staged namespace copies, verified success promotion, operation journaling and interrupted-promotion reconciliation. Verify partial/disk-full/cancelled jobs retain the active ten-version history, hard links cannot corrupt mappings, and crashes at each rename recover a usable namespace without duplicate committed jobs.
- [x] 7.6 Document engine isolation, manifest provenance/update policy, destination overlap checks and local-only behavior in `docs/local-state.md` and the DRM-free guide; verify the documented paths and defaults match the adapter configuration.

## 8. Automatic backups, history and restore

- [ ] 8.1 Implement the pure durable scheduler keyed by installation/profile/play-period, three-second post-handoff settling, fresh stopped checks, single-writer serialization and bounded retries. Verify external starts, immediate restart, enumeration uncertainty, repeated exits, administrative closes and app restart produce the expected jobs without backing up running games.
- [ ] 8.2 Wire global/per-game automatic control, Backup now, default local destination, destination changes and ten-full-version/zero-differential retention. Verify sync-off independence, queued-job cancellation on disable, unchanged saves, successful eleventh version, unavailable destination and retention never deleting previous good data after failure.
- [ ] 8.3 Add history/result loading with engine backup ids and installation/profile/destination namespaces. Verify two copies of one title remain distinct, partial results are not success, old destinations remain visible and removal/rematching cannot detach archives.
- [ ] 8.4 Implement selected-backup restore preview, user confirmation, repeated stopped check and verified recovery of all affected file/registry targets before restore. Verify recovery failure aborts writes, an empty current state is valid, exact historical ids restore, unrelated files remain, and a partial restore exposes its recovery snapshot.
- [ ] 8.5 Protect recovery snapshots from ordinary retention and serialize overlapping save resources. Verify two installations sharing a save folder cannot back up/restore concurrently, an external restart during preparation cancels writes, and interrupted operations retain sufficient journal/history for recovery.
- [ ] 8.6 Document automatic timing, version history, same-disk limitations, restore recovery, custom save locations and manual cleanup of retained recovery data. Verify all described user actions and failure messages against isolated Windows fixtures.

## 9. Native UI, translations and package integration

- [ ] 9.1 Add library/empty-state Add game, edit/Locate/Remove, inferred-match provenance and discovery review overlays to the existing Slint view/engine command boundary. Verify keyboard focus, Escape/cancel, multi-select review, unavailable entries and offline quick add; callbacks return promptly while workers are deliberately stalled.
- [ ] 9.2 Add detail-pane save status/profile correction, custom location picker, Backup now/history/restore preview and Settings backup controls/destination. Verify unsupported, no-saves, queued, running, unchanged, partial and failed states, plus profile correction without catalog changes.
- [ ] 9.3 Add all new English and `pt_BR` strings through the existing translation pipeline. Verify catalog completeness and keyboard-accessible layouts in both locales, including long error/status strings.
- [ ] 9.4 Extend Windows build/package/install scripts and license/About assets for the pinned helper/manifest and build digests. Verify a fresh offline package installs with working backup support, rejects bad release input checksums, uninstalls without deleting archives and leaves Linux packaging without a Windows executable dependency.
- [x] 9.5 Update `docs/mvp-roadmap.md` to step 4a DRM-free/local backup and 4b Epic/GOG/Ubisoft, retaining later-step dependency meaning and documenting native-helper/restore-write rules. Verify no store scanner, authentication or cloud-save implementation is implied by 4a.

## 10. Cross-feature acceptance

- [ ] 10.1 Run `cargo fmt --all -- --check`, `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and the repository Windows cross-build/package checks. Verify Linux still compiles with unsupported Windows-only features and the produced Windows archive includes all declared assets.
- [ ] 10.2 Run Windows acceptance from `docs/windows-verification.md` with at least one ordinary and one launcher-based DRM-free game: one-picker add, first automatic match/correction, external start while hidden, accurate stop/session and after-play backup. Record actual outcomes and any supported inspection limits rather than marking unrun checks complete.
- [ ] 10.3 Run an isolated Windows discovery/backup/restore acceptance scenario covering a mixed executable folder, ignore persistence, removable-drive loss, offline start, wrong save profile, disk-full staging failure, interrupted promotion and registry/file restore with recovery. Verify existing Steam launch/tracking and account-specific matching still work.

## Workflow follow-up

- Review implementation against all seven delta specs, then archive this change only after the prerequisite spec is present and required acceptance has passed.
- Plan Epic/GOG/Ubisoft and their generic scanners as the next separate change; cloud saves and Linux platform support remain future work.

## Paused implementation checkpoint — 2026-11-05

Paused at the user's explicit request. **16/47 tasks are fully checked off.**
Unchecked tasks below often contain substantial implementation; their full acceptance
criteria have not yet been demonstrated. Do not interpret the checkbox count as the
percentage of code written, and do not check off an entire task for partial coverage.

### Implemented since the previous checkpoint

- Manual launch now passes through the core launch boundary and validates approved
  registrations. Add, duplicate selection, repair and removal persist on workers.
  The engine test verifies cancellation and responsiveness with inspection stalled.
- Windows adapters canonicalize executable paths, retain file identity, validate PE
  headers, reject DLLs and revalidate launch/settings targets. Native fixtures cover
  Unicode, spaces/metacharacters, hard-link aliases and a replaced junction escaping
  the registered folder. The remaining 3.3 failure-path matrix is still open.
- Conservative executable/manifest evidence, bounded aliases, search completeness,
  external-id hints, account/evidence generations, cached inference loading and cover
  requests are wired. Provenance is visible in details. The complete stale-response,
  offline-retry and user-override acceptance matrix remains open (4.1/4.4/4.5).
- Tracking has explicit gameplay/launcher policies, deterministic ownership,
  ten-second handoff and physical stop events. Windows process observations include
  canonical paths and observable parent creation identities. Store/manual conflicts
  offer an explicit transfer, and positively identified gameplay bindings can be
  persisted with revision checks. Full engine/real-game coverage remains open.
- Watched-folder notifications, debounce, reconciliation, cancellation, bounded
  traversal, overlap suppression, executable fingerprints and incomplete-result
  reporting are implemented. UI review supports selection, target correction,
  Add/Ignore, reset and watch removal. The discovery corpus and complete restart,
  overflow, unplug/reconnect and tombstone acceptance scenarios remain open.
- Save-profile preview/correction, native custom file/folder pickers, independent
  global/per-game controls, Backup now and history/restore UI are wired. The bundled
  adapter isolates configuration, stages namespace copies, journals promotion,
  records durable job markers, creates separate recovery backups and checks stopped
  state before/during mutation. The scheduler persists jobs and retries, cancels
  disabled automatic jobs and recovers interrupted jobs. Full fault-injection,
  retention, overlapping-resource and restore acceptance remains open.
- English/pt_BR strings, Slint overlays, detail/context-menu actions, documentation,
  Windows asset packaging/install/uninstall and About notices were added. A native
  pt_BR protection-panel render was inspected. Complete keyboard/layout/error-string
  review, fresh offline install/uninstall verification and final packaging remain open.

### Verification actually completed

- A full Linux workspace test run passed **461 tests** (log:
  `/tmp/drm-validation-linux.log`).
- A full Windows workspace test run through WSL interoperability passed **450 tests**
  (log: `/tmp/drm-validation-windows.log`). This executes Windows binaries; it is not
  a native Visual Studio build.
- Workspace Clippy with `--all-targets --locked -- -D warnings` passed
  (`/tmp/drm-clippy6.log`). Formatting was applied throughout implementation.
- The isolated pinned-helper contract passed for renamed install paths, AppData,
  Documents, custom files, registry, empty preview, history and selected restore.
- A Windows adapter fixture passed backup/history/selected restore and verified a
  separately retained recovery namespace. Tracker fixtures passed replacement,
  persistent launcher/crash reporter exclusion, single physical stop and
  administrative-close/session-split behavior.
- The initial junction fixture failed because of its fixture command invocation.
  It was corrected to create the junction through an isolated PowerShell script;
  the targeted test then passed (`/tmp/drm-junction-test6.log`).
- A Windows ZIP containing the helper, manifest and input metadata was produced
  (`/tmp/drm-package.log`). Further source/notice changes followed that package build;
  the existing ZIP is an intermediate artifact and must be regenerated before release.

These passing runs precede some subsequent edits. They establish tested checkpoints,
not a final clean bill of health for the exact paused workspace. Repeat the full Linux
and Windows suites, formatting check, Clippy and packaging after finishing the change.

### Remaining acceptance and usability limits

- The current build is a development preview suitable for deliberate local add/launch
  and isolated save-protection trials. Basic paths are implemented, but complete
  matching, discovery, launcher and recovery behavior is not yet accepted end to end.
- No real installed game was launched or played during this session. Installed game
  locations were inspected read-only; this does not satisfy 10.2.
- Automatic after-play protection and restore must not yet be described as production
  ready. The full disk-full/cancellation/interrupted-promotion, eleven-version
  retention, missing-destination and file/registry recovery matrix is unfinished.
- The app already installed on the user's machine was not upgraded. The workspace,
  intermediate package and installed application must be distinguished.
- No commit, push, spec synchronization or archive was performed for this change.
