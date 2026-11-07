# Tasks

## 1. Foundation: domain, ports and persistence

- [ ] 1.1 Create `crates/core/src/downloads/` with `Transfer`, `TransferState`, `EngineStatus`, `DownloadsView`, `DownloadPreferences`, `Consent`, `InstallJob`, `InstallPhase`, `InstallProposal` and `DetectedGame`; verify with unit tests for state mapping and serde round-trips
- [ ] 1.2 Add magnet parsing and info-hash normalization (40-hex and 32-base32), length limits and duplicate detection; verify with table-driven tests for valid, malformed, oversized and duplicate input
- [ ] 1.3 Add `TorrentEngine`, `DownloadStore`, `InstallRunner` and `FileReveal` ports with in-memory doubles; verify the crate compiles and the doubles satisfy every trait method
- [ ] 1.4 Add `crates/platform/migrations/0007_downloads.sql` for `download_preferences`, `download_associations` and `install_jobs`; verify fresh-database, upgrade-from-0006 and corrupt-consent tests pass
- [ ] 1.5 Implement `DownloadStore` persistence and the fail-closed consent rule (`DOWNLOADS_TERM_VERSION`); verify a missing, corrupt or older consent leaves the feature disabled
- [ ] 1.6 Add rqbit's Apache-2.0 notice to the bundled license notices; verify `get_license_notices` includes it and the About panel test still passes

## 2. rqbit adapter and engine supervisor

- [ ] 2.1 Implement the rqbit HTTP client in `crates/app/src/downloads/rqbit.rs` for add, list, stats, pause, resume, forget, details and file selection; verify `wiremock` tests cover success, 401, 404 and malformed bodies
- [ ] 2.2 Map rqbit states and stats to the domain (`metadata`, `downloading`, `paused`, `seeding`, `completed`, `failed`) and cover progress, ETA, peers and unknown values; verify unit tests
- [ ] 2.3 Add the pinned rqbit binary to CI with SHA-256 verification and target-triple naming, and declare `bundle.externalBin`; verify a Windows and a Linux build both produce the sidecar next to the app binary
- [ ] 2.4 Implement the supervisor: free-port selection, Basic Auth environment, readiness check, persistence arguments, visible/hidden polling, `downloads:view` emission and pause-then-terminate shutdown; verify with a fake engine and a manual smoke run against real rqbit
- [ ] 2.5 Add crash detection, exponential backoff, manual retry and localized failure states; verify tests for one crash, repeated crashes and missing binary
- [ ] 2.6 Add the lockfile plus Windows Job Object containment, and reconcile an orphan sidecar on startup; verify reuse, cleanup and no duplicate process in tests
- [ ] 2.7 Wire the `downloads_*` Tauri commands (view, consent, enable, add, pause, resume, forget, select files, open folder, retry) as async handlers; verify command tests and that no handler blocks the UI thread

## 3. Downloads UI: navigation, consent and transfers

- [ ] 3.1 Add `section` to `uiStore`, render `LibraryScreen` or `DownloadsScreen` in `App.tsx`, and add the Biblioteca/Downloads navigation to `Sidebar.tsx`; verify the app opens on Biblioteca and switches sections without a new window
- [ ] 3.2 Implement `useDownloads` (query plus `downloads:view` subscription), the TypeScript types and command wrappers; verify the mock host drives list, engine and install-job states
- [ ] 3.3 Build the consent screen with the five sections, checkbox-gated accept, decline path and term version; verify it appears on first Downloads visit and on an add attempt, and that accept/decline leave the correct state
- [ ] 3.4 Build the Downloads list with engine pill, global speeds, filters, rows, empty state and unavailable-engine state; verify Vitest covers each state and the sidebar badge count
- [ ] 3.5 Build the add-transfer dialog for magnet and `.torrent` with inline validation and duplicate selection; verify valid, malformed and duplicate cases
- [ ] 3.6 Build the transfer detail with info hash, destination lock, file table and selection updates; verify the destination cannot change after bytes are written
- [ ] 3.7 Add pause/resume, forget-keeping-files and open-folder actions with toasts; verify files remain on disk after forget and that every action is keyboard reachable
- [ ] 3.8 Add completion cards with the two primary actions, associated-entry shortcut, dismiss and disabled reasons; verify each state in Vitest

## 4. Install pipeline: Windows

- [ ] 4.1 Implement installer discovery with setup/install name matching, maintenance-executable exclusions, multiple-choice and no-installer cases; verify unit tests for each filename and folder shape
- [ ] 4.2 Add SHA-256 and publisher extraction to the install proposal; verify the hash matches a fixture file and a missing publisher is shown as unknown
- [ ] 4.3 Implement the persisted install-job state machine and restart reconciliation; verify every transition and that a `running` job becomes `interrupted` without re-running
- [ ] 4.4 Implement the Windows process runner (no shell, visible window, Job Object, exit code) with a fixture process; verify success, nonzero exit and cancellation
- [ ] 4.5 Implement bounded pre/post detection (chosen destination, Program Files, LocalAppData, AppData, transfer folder) plus the registry diff fallback; verify unique, ambiguous and no-result tests with an in-memory filesystem and registry
- [ ] 4.6 Build the install confirmation, installing, detecting and review screens with the consent checkbox and cancel warning; verify the primary action is disabled until consent and cancel leaves no entry
- [ ] 4.7 Create the resulting `manual` entry with executable, provenance and background matching, including duplicate selection and removal; verify integration tests and that removal keeps transfer and installed files
- [ ] 4.8 Implement the portable-game path using the existing discovery review; verify a recognized folder is offered directly and an unrecognized one falls back to manual selection
- [ ] 4.9 Enforce the platform scope: on Linux, disable Instalação automática with a localized explanation and keep Abrir pasta de destino; verify no runtime probe, prefix, launch registration or entry is created and that no UI copy claims Proton/Wine support

## 5. Settings and local-state integration

- [ ] 5.1 Add the Downloads topic and panel with engine status, enable/disable, destination, limits, seeding and license; verify the topic order and keyboard navigation tests
- [ ] 5.2 Persist and validate destination, limits and seeding policy, applying them before the engine starts; verify restart and invalid-destination tests
- [ ] 5.3 Implement disable/enable semantics: stop the engine, open no ports, preserve resumable state and keep consent unless the term changed; verify with the fake engine and a real sidecar smoke test
- [ ] 5.4 Add all new strings to `en.ts` and `pt-BR.ts` and audit focus, labels and screen-reader roles; verify the i18n test and a keyboard-only pass over consent, list, add, detail and settings

## 6. Hardening, documentation and acceptance

- [ ] 6.1 Audit the security boundaries: Basic Auth redaction, loopback-only API, path containment for `content_path`, magnet length caps and no arbitrary URL intake; verify with focused tests and a review of logs
- [ ] 6.2 Add free-space checks and map disk-full, permission and engine-unavailable errors to localized messages; verify each error path leaves data intact
- [ ] 6.3 Verify background and quit behavior: downloads continue when the window is hidden on Windows, Quit stops the engine safely and Quit during an install asks for confirmation; verify with integration tests and a manual Windows run
- [ ] 6.4 Write `docs/downloads.md` covering consent, engine lifecycle, Windows installer execution and the explicitly deferred Linux/Proton work, and update `docs/development.md` with the sidecar build step; verify the documented commands run as written
- [ ] 6.5 Run the acceptance pass on Windows with a legal test torrent and a real installer, and on Linux with a transfer-only smoke test confirming the install action is disabled; verify the Windows transfer becomes a playable entry and the Linux flow falls back cleanly
- [ ] 6.6 Confirm CI fetches and verifies the sidecar, builds both targets and includes the license notice; verify a clean CI run
- [ ] 6.7 Measure the poller and UI with at least 50 transfers, including hidden-window polling; verify the UI stays responsive and no library snapshot is republished
- [ ] 6.8 Run `cargo test --locked -p gamenode-app`, `pnpm --dir ui test --run` and `openspec validate add-downloads-and-rqbit-engine --strict`; verify all pass

## Workflow follow-up

- Review the change artifacts and implementation against the specs before archive.
- Archive the change only after the acceptance pass and the project's review requirements are satisfied.
- Verify the archived specs and that `openspec validate` still passes.
