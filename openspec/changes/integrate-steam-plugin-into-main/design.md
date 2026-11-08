# Design

## Context

See `proposal.md` for motivation and the three delta specs for the behavior contract. This is a mainline-based integration of existing implementation, with fresh verification rather than a restart of Steam development.

Inspected baselines on 2026-11-08:

- `origin/main`: `578ffa72687847f95b781d6862a7fdb1dc05e390`, including the PC ports catalog/installer and `DESIGN.md`.
- Donor `integration/migrate-steam-to-plugin`: `77389ce1fb3a4e218ab4fbe3504b115883ea09de`.
- Common ancestor: `1cbac996a22d1cae1ecd1f26fe407cf407b0d007`. The donor has twelve subsequent commits; mainline has two.

Mainline's `engine/pc_ports.rs` composes `PortsService`, `PortProgressPublisher` and `PortStore`; `platform/ports` implements catalog, download/extraction, supplied-file handling, jobs and recovery. Migration `0008_ports.sql` stores snapshot, overlay, jobs, provenance and preferences. Installed ports use ordinary manual/DRM-free registrations, launch environment and executable roles. The frontend includes Windows-only navigation, virtualized catalog views, guided setup and helper-tool settings. None of those components needs a plugin contract for this integration.

The donor already has protocol/SDK/Steam crates, public DTOs, scoped services, an isolated login broker, persistent provider supervision, a Linux UDS host and Windows named-pipe support. Production Engine uses `providers`; old Steam adapter names remain under `cfg(test)` for regression fixtures. Public crates do not depend on internal application crates. Protobuf generation selects a vendored compiler for the build host. The donor also includes the Linux package-manager plugin and optional `ui.settings`, which are outside this integration's delivery scope.

The earlier merge preview found textual conflicts in `engine/mod.rs`, `main.rs`, `platform/src/db.rs`, `ui/src/api/index.ts` and generated `ui/dist/index.html`. Selective transplantation avoids treating a conflict-free file replacement as proof of mainline compatibility. Shared command, snapshot, launch, tracking, mock and packaging changes still require semantic review.

`migrate-steam-to-plugin` records 63/72 tasks, but its evidence is for an older mainline baseline. Native Linux and Windows gates remain open. The socket-cleanup commit `7b6edf7` needs full-application acceptance, and the login-close fix `77389ce` was committed without completed checks. Mainline's `add-pc-port-catalog` also records unfinished extractor/native Windows checks. These are evidence gaps to validate, not proof that either implementation is broken or accepted.

## Goals / Non-Goals

**Goals:**

- Preserve current mainline composition and PC ports behavior while replacing all production Steam interpretation with the external plugin.
- Reuse tested donor modules and preserve meaningful mainline regression fixtures, adapting their boundary rather than dropping cases.
- Keep host-owned platform resources and domain policy behind a small public contract with explicit service authority.
- Produce a reviewable integrated candidate and evidence tied to its actual revisions on both supported platforms.

**Non-Goals:**

- Extract or generalize PC ports into a plugin; add catalog browsing or installation RPCs; enable Linux port installation.
- Deliver the Linux package-manager executable, optional plugin settings UI, another store, community installation, WASM or a different transport.
- Redesign GameNode synchronization, process tracking or measured-session submission; add a production in-process Steam fallback.
- Rewrite source branch history, import obsolete CLI/daemon prototypes, merge generated bundles or mark predecessor acceptance as completed.

## Decisions

### 1. Start a new integration branch from current mainline

At apply time, fetch and record the exact selected mainline SHA, donor SHA and profile/schema baseline. Create a new branch from `origin/main`, carrying this planning directory forward and preserving the donor and original mainline branches. Suggested branch name: `integration/steam-plugin-on-main`. Preserve unexpected contributor changes; do not reset or overwrite them.

Transplant final useful code by coherent areas: public protocol/SDK, Steam implementation, host/runtime/services, provider routing, frontend accounts/inventory, then packaging and enforcement. Review the resulting diff against mainline after each stage. This reuses implementation rather than replaying CLI history or blindly cherry-picking the large `115148a` feature commit.

Alternative considered: merge all donor changes and prune later. That imports unrelated package-manager/settings behavior and makes mainline regressions harder to attribute. The chosen approach permits small shared-file edits while retaining mainline ownership of the surrounding application.

### 2. Keep the complete Steam workflow in the plugin

Carry `gamenode-plugin-protocol`, `gamenode-plugin-sdk` and `gamenode-plugin-steam`, including Steam-owned parsing, root/registry discovery, HTTP/session logic, account transitions, login policy data, owned/Family retrieval and `localconfig.vdf` counter interpretation. Preserve provider ID `steam`, plugin ID `org.gamenode.plugin.steam-discovery`, stable `steam:<appid>` keys, exclusions and declared URI templates.

Core/Engine continues to own GameNode login/REST, reconciliation of installed/owned/family facts, storage, matching, favorites, covers, process tracking, Store/Internal policy, account attribution, deduplication and pending submissions. Generic host code must not parse Steam files, interpret its HTTP responses or branch into a native Steam production fallback. Test-only compatibility adapters may remain if dependency checks distinguish test fixtures from production linkage.

Reuse the existing additive API and host-issued bidirectional operations. Keep partial catalog categories staged until validated completion. Preserve success-empty versus failure, independent owned/Family outcomes, coalescing and late-result rejection. Optional wire definitions already in the donor protocol may remain inert to preserve compatibility; do not grant optional settings authority or import its GUI/workflow solely because the wire definition exists.

Alternative considered: move only discovery and keep login/catalog in the application. It would violate the complete Steam ownership boundary and retain store-specific coupling.

### 3. Retain the scoped host brokers and credential aliases

The integrated Steam descriptor explicitly approves only the existing service grants:

| Grant | Host-owned service |
| --- | --- |
| `provider.context.read` | Own validated manifest, platform, limits and operation context |
| `library.discovery` | Installed observations for the active scan |
| `library.catalog` | Complete owned/Family account facts |
| `account.state.read`, `account.state.publish` | Non-secret identity/state and authorized transitions |
| `auth.webview` | Open/focus/cancel/close one isolated attempt |
| `auth.webview.cookies.read`, `auth.webview.page.read` | Approved cookies and exact-origin bounded page probe |
| `secrets.read`, `secrets.write`, `secrets.delete` | Own logical credential through host-selected storage |
| `playtime.counter` | Provider total/last-played observations |
| `provider.actions.declare` | Approved Play/Install URI templates |

Authorization binds each request to session, provider, operation and applicable profile/account epoch; recheck before queued side effects commit. Serialize credential mutation and account transition commits so cancellation/sign-out cannot be undone by stale writes. Preserve mainline failure semantics for deletion, expiry, account replacement and session-only login.

Use the trusted descriptor's alias to the existing physical Steam credential slot and Linux fallback path. The plugin cannot choose an OS key or database location. Do not copy, rename or delete the authoritative credential merely to introduce a logical namespace. Windows retains Credential Manager and no plaintext fallback. Linux retains its existing Secret Service/private-file behavior.

Keep Tauri native-window management generic and asynchronous. Import and test the login close gate from `77389ce`, including user-close while cookies/page data are being collected. The host enforces origin/cookie/probe policy; Steam decides whether returned data establishes a valid session. Document native HTTPS/files/registry access separately from API grants; native processes are not sandboxed.

Alternative considered: direct plugin database/keyring access. It would couple the plugin to profile storage and bypass generation and ownership checks.

### 4. Preserve PC ports at its current boundary

Start from mainline's ports modules, `ports/overlay.toml`, tests, documentation and `add-pc-port-catalog` artifacts. Keep `PortsService`, startup recovery before job workers, dedicated progress events, cancellation, provenance, executable selection and the transactional registration/completion commit. `library.catalog` remains account ownership and gains no ports meaning.

Edit shared composition selectively:

- Add plugin initialization and generic provider observers alongside existing `pc_ports`, `port_store` and `port_progress` fields. Keep PC ports snapshot projection and completion callbacks.
- Preserve mainline manual launch environment and process-policy handling, port relinking, `drmfree:`/`manual:` selection adaptation and tools/cache commands.
- Combine plugin shutdown with ports shutdown on background tasks; keep hide-to-tray distinct from Quit. A Steam endpoint or process failure must not gate ports startup or cancel its jobs.
- Keep `PortArtState` and the asset route. Frontend command exports, default snapshots and mocks include both features; preserve sidebar collapse, destination selection, account controls, theme, query/store bridge, modal behavior and virtualization.

Alternative considered: extract ports now through `CatalogSource` and `PortInstallPlan`. Feasible, but it requires catalog/install contracts and additional permissions outside the confirmed scope.

### 5. Preserve schema 8 and existing data ownership

Keep mainline migrations 0001-0008 and ports table shapes. The Steam integration should require no new schema migration: provider IDs, account rows and credential aliases remain stable. Adapt the donor's v6-to-current migration fixture to schema 8 while retaining mainline's v7-to-v8 preservation fixture. Also cover opening an already populated schema-8 profile.

Verify account metadata, installed/owned/Family flags, first-seen values, matches, favorites, artwork and playtime history alongside ports snapshots, jobs, provenance, preferences, launch arguments/environment and removed/re-added registrations. Ports data remains scoped to the OS-user profile and shared across GameNode accounts under existing mainline rules; do not partition it by Steam account.

Alternative considered: rebuild profile tables or move them into plugin storage. That introduces unnecessary migration risk and breaks host ownership.

### 6. Keep platform delivery independent of developer build state

Use the donor's protected local transports: Linux private directory/socket permissions and peer UID verification; Windows user/SYSTEM DACL, remote rejection, first-instance ownership, asynchronous instance recreation and bounded busy-pipe retry. Preserve single-instance IPC semantics and explicit endpoint cleanup. Plugin infrastructure failure produces provider unavailability while the desktop remains usable.

Adapt development and release scripts to build and locate the Steam executable adjacent to the host on Linux and Windows. Build-host vendored `protoc` works in native and cross builds. Keep mainline resources, port overlay/extractors and existing packaging requirements. The verifier and CI must not require the deferred package-manager binary or grant it permissions. Do not use PATH or a leftover build-tree executable as an implicit fallback.

Carry the public-dependency checker and reference-host tests, scoped to protocol/SDK/Steam. Import packaging changes needed for this delivery; broader distribution changes remain separately reviewable. Update the affected development and plugin documentation to describe the actual delivered surface.

### 7. Make the new checklist authoritative for integrated evidence

This change consolidates the Steam runtime/service/provider deltas under the existing capability paths `desktop-plugins`, `desktop-plugin-services` and `desktop-providers`. Its required subset intentionally excludes package-manager and optional settings requirements from the older unsynchronized proposals. Those remain deferred work on the donor, not silently discarded or declared done.

During integration, record predecessor task-to-task mapping and supersession in the integration evidence. Carry relevant historical source/tests, but run and record checks on the new candidate. Preserve mainline OpenSpec archive paths and `add-pc-port-catalog`; do not resurrect its removed downloads proposal or import duplicate archive renames. After acceptance, synchronize this consolidated delta once through the requested OpenSpec workflow. Do not subsequently overwrite it with a conflicting predecessor delta or archive a predecessor while deferred work is unresolved.

Use `docs/steam-plugin-integration.md` for the actual baseline/candidate SHAs, commands, environment, per-scenario Linux/Windows evidence, failure fixes and unresolved items. Historical donor results remain clearly labeled. All tasks start unchecked.

### 8. Validate in stages with native platform gates

Run appropriate locked Rust tests and strict lint, public dependency/reference-host checks, packaging fixtures, UI tests/build and OpenSpec validation on the integrated code. Retain meaningful Steam account/login/session/catalog/race/counter and mainline ports regression fixtures. Rebuild `ui/dist` from source when the repository workflow requires it. Use reduced debug/incremental settings when needed; avoid unrelated build-directory deletion.

Native Linux acceptance uses a safe isolated profile and, for credential-upgrade checks, a controlled copy of existing state with a credential setup that cannot mutate live slots. Record when a private D-Bus session or fallback path changes the tested storage backend. Test real login/Guard when applicable, focus/cancel/close, restart restore, discovery, ownership/Family, actions, crash/restart and socket/child/profile cleanup. Do not treat an empty isolated credential store as a migration failure or live-login proof from fixtures. Keep Linux's existing process-tracking limitation explicit.

Present the Linux candidate and results and obtain explicit confirmation before scheduling native Windows acceptance. The user provides a Windows VM. Then verify real named-pipe protection/recreation, WebView2/Guard, Credential Manager, registry/multiple drives, ownership/account transitions and native tracking/counter triggers. Exercise PC ports end to end there: release-only/guided/package installs, update overlay, progress/cancel/Quit/recovery, missing-executable selection, removal/re-add, launch environment and gameplay tracking. User-owned dump/package inputs are supplied only where applicable; untested required scenarios remain open.

Cross-target compilation can establish build evidence; test binaries linked with `--no-run` are not executed tests. No Windows test, VM session or browser is started by this planning change. Final readiness requires both native gates and a Steam-independent ports regression pass.

### 9. Parallelize independent implementation work

Once the mainline branch and shared-file ownership are established, run independent workstreams concurrently where they do not edit the same files or depend on unfinished contracts. For example, protocol/SDK work, Steam-owned implementation and its fixtures, and plugin documentation can proceed in parallel after task 1.1. Host transport, brokers and packaging can proceed as their public interfaces stabilize. Engine composition, GUI adapters and ports-coexistence checks can then proceed in parallel behind those interfaces. Keep one integrator responsible for shared files, API decisions, lockfile/build-script conflicts and combining results. Rebase or integrate each stream before running cross-cutting checks; do not parallelize native Linux/Windows acceptance or declare a task done before its dependencies and verification complete.

Prefer parallel execution for independent modules, platform-independent fixture work and documentation. Serialize edits to shared files such as workspace manifests/lockfile, protocol definitions, `engine/mod.rs`, `main.rs`, provider descriptors, snapshots, UI API exports and generated bundles. Parallelism is an implementation scheduling choice; it does not change task dependencies, evidence requirements, or the Linux-confirmation-before-Windows-acceptance gate.

## Risks / Trade-offs

- [Donor file replacement overwrites newer ports composition] -> Apply selective edits and inspect the diff against pinned mainline, especially launch, shutdown, snapshots and mock defaults.
- [Old passing tests are mistaken for integrated acceptance] -> Re-run affected checks and tie evidence to candidate SHAs; keep native tasks unchecked until observed.
- [Plugin removal from a package prevents application startup] -> Exercise missing executable and endpoint-failure paths while ports remains usable.
- [WebView closes during extraction] -> Test the donor close gate and native close/cancel behavior before accepting the candidate.
- [A schema-7 fixture is copied over schema-8 expectations] -> Preserve both upgrade paths and a populated v8 fixture without editing released migrations.
- [Optional features leak into the narrowed scope] -> Keep package-manager/settings work deferred; inert public wire compatibility does not authorize controls, grants or executable delivery.
- [Windows validation depends on the VM and real inputs] -> Finish Linux first, request the agreed VM at its gate and record missing acceptance evidence explicitly.

## Migration Plan

1. Pin/fetch mainline, carry this plan to a new mainline-based integration branch and record the donor selection and parity matrix.
2. Transplant the required public crates, full Steam implementation, generic brokers/routing and GUI adapters while preserving ports composition and state.
3. Adapt Steam packaging, documentation and CI; verify public dependency boundaries and automated regressions on the integrated candidate.
4. Validate safe profile upgrades and real Linux flows; record explicit Linux confirmation.
5. Build/install the integrated Windows candidate and complete native Steam plus PC ports acceptance in the user's VM.
6. Prepare the readiness report and reconcile OpenSpec only after required acceptance, with commit/push/merge actions separately requested.

Rollback uses preserved branches and a pre-upgrade profile backup. The integration preserves mainline schema 8, so an equivalent mainline build remains schema-compatible; confirm the selected binary's schema support before opening a test profile. Never downgrade a schema-8 profile with a donor build supporting only schema 7. Restore the isolated backup for destructive acceptance/account-transition scenarios, and never replace live credentials automatically.
