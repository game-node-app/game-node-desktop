# Tasks

All tasks refer to the new mainline-based candidate. Donor implementation and historical test results are reusable inputs, not completed integration evidence. Keep a task unchecked until its stated verification has been performed on this candidate.

## 1. Mainline baseline and integration record

- [ ] 1.1 Fetch `origin/main`, pin its SHA and donor `77389ce`, and create a new integration branch from mainline carrying this planning directory without resetting either source branch or overwriting contributor changes; verify ancestry includes the selected mainline and record the refs in `docs/steam-plugin-integration.md`.
- [ ] 1.2 Create the Steam/mainline/PC ports parity matrix and predecessor task mapping in the integration record; verify it names native versus fixture evidence, maps the nine unresolved predecessor gates, retains ports checks and explicitly defers package-manager/settings/ports-plugin work without copying completed checkboxes.
- [ ] 1.3 Preserve mainline `DESIGN.md`, migrations, port source/tests/overlay, documentation and `add-pc-port-catalog` artifacts during preparation; verify the baseline diff contains no restored downloads proposal, duplicate archive renames or unrelated donor functionality.

## 2. Public contract and complete Steam implementation

- [ ] 2.1 Port the public protocol/SDK, negotiated operations, bounded DTOs and build-host vendored Protobuf compiler with required workspace/lockfile changes; verify the public crates build and their contract/negotiation tests pass without a system `protoc`, and no internal application dependency enters their closure.
- [ ] 2.2 Port the complete Steam crate and trusted provider/login policy data, preserving IDs, exclusions, actions, native/Flatpak/registry roots, session HTTP, owned/Family logic and local counters; verify the Steam crate's scanner, HTTP, account, login and counter fixtures pass against the public contract.
- [ ] 2.3 Adapt the reference-host and command/event fixtures to exercise initialization, discovery, login/check/sign-out, catalog, counters, cancellation and typed failures independently of the application; verify interrupted/empty/failed category outcomes and unsupported operations are distinguishable and stale completions are rejected.
- [ ] 2.4 Port the required plugin-development guidance for the delivered Steam API, including the approved permissions and native-access distinction; verify examples match public DTOs and describe neither package-manager delivery nor optional settings or install/catalog-browsing services as available features of this integration.

## 3. Host runtime, transport and protected services

- [ ] 3.1 Port protected Linux UDS and Windows named-pipe transport plus client connectors and connection limits, preserving single-instance IPC; verify Linux authorization/ownership tests and Windows-target compilation, retaining native DACL/recreation/remote-client execution checks for section 9.
- [ ] 3.2 Port provider supervision, launch resolution, authenticated sessions, concurrent operations, cancellation/timeouts and bounded shutdown; verify process fixtures cover one Steam child, crash/restart, missing/incompatible executables, endpoint failure, stale handles and preservation of cached facts without crash loops.
- [ ] 3.3 Port context, discovery, catalog, account and action brokers with explicit descriptor grants and generation fencing; verify service/permission fixtures reject absent grants, cross-provider or cross-operation access, oversized/incomplete results and unauthorized URI declarations before side effects.
- [ ] 3.4 Port namespaced secret access and serialized account/credential mutation using the existing physical Steam alias; verify credential fixtures preserve restore, rotation, sign-out deletion failures, cancellation races, account replacement, session-only storage and Windows no-plaintext behavior, and document the actual alias/storage rules.
- [ ] 3.5 Port the isolated native login broker and fixes from `77389ce` and `7b6edf7`; verify controlled tests cover approved navigation/cookies/page probe, focus/cancel/timeout, close during extraction, temporary profile cleanup and endpoint removal outside UI callbacks, and record real-application verification still required in section 8.

## 4. Engine integration and state preservation

- [ ] 4.1 Replace production mainline Steam composition and adapters with generic provider runtime calls; verify scans and account controls reach the external Steam process on both configured platforms and production linkage has no native Steam parsing/HTTP fallback or application-internal dependency from public plugin crates.
- [ ] 4.2 Integrate owned/Family streams and account transitions with existing reconciliation and storage queues; verify mainline/donor Engine regression fixtures preserve stable IDs, installed facts, independent category failures, cache-first startup, coalescing, sign-out/account replacement and late-result fencing.
- [ ] 4.3 Integrate plugin-owned action declarations and asynchronous counters while retaining Core tracking/submission policy; verify URI and playtime fixtures cover cached actions, missing handlers, startup/scan/exit triggers, latest Steam account, Store/Internal changes, GameNode-account attribution and unchanged-total deduplication.
- [ ] 4.4 Compose the runtime alongside mainline `PortsService`, observers, progress, artwork, registration relinking, launch environment/process policy and background shutdown; verify existing port/DRM-free/job tests pass and a failing Steam runtime neither blocks startup nor cancels/changes a ports job.
- [ ] 4.5 Preserve migrations 0001-0008 and add/adapt focused v6-to-v8, v7-to-v8 and populated-v8 upgrade fixtures; verify Steam accounts/facts/credentials aliases, matches/favorites/history and all ports jobs/provenance/preferences/arguments/environment survive, with no new migration or table rewrite required.

## 5. GUI accounts, inventory and ports coexistence

- [ ] 5.1 Port generic asynchronous account commands, account presentation and trusted Steam plugin inventory with actual grants/status/native-access descriptions; verify account/inventory component tests cover actions, progress, failures, official origin, missing executable and secret-free state without optional settings controls.
- [ ] 5.2 Adapt shared command exports, snapshots, defaults and mocks while preserving mainline sidebar collapse, Windows ports destination, filters, guided setup, tools/cache controls, modal behavior and artwork; verify existing shell/ports/settings tests plus a combined Steam-and-ports snapshot case pass in English and pt-BR.
- [ ] 5.3 Run affected UI suites and build, regenerating `ui/dist` from integrated source; verify query/store updates, virtualization, lazy/async artwork and keyboard/focus behavior remain intact, and update development docs with the actual integrated mock and account/inventory flows.

## 6. Development, packaging and boundary enforcement

- [ ] 6.1 Adapt documented development workflows to build/locate host and Steam executable together on Linux and Windows without PATH fallback; verify the clean locked workflow resolves the intended adjacent plugin and records required toolchain/dependencies, with no package-manager binary requirement.
- [ ] 6.2 Port the required release-packaging checks while retaining mainline ports and other resources; verify package fixtures reject missing/wrong-platform Steam binaries, retain paths with spaces and validate existing assets/licenses, and document each platform's install/run workflow.
- [ ] 6.3 Port dependency-boundary enforcement and relevant CI jobs scoped to protocol/SDK/Steam; verify checker fixtures detect forbidden direct/transitive/build/platform-only edges, production host linkage excludes Steam implementation, and CI definitions do not require deferred plugin features.

## 7. Integrated automated and upgrade checks

- [ ] 7.1 Run the integrated locked Rust workspace tests, formatting, strict Clippy, public-boundary/reference-host and packaging checks, UI suites/build and strict OpenSpec validation; verify all required checks pass and record exact candidate revision, commands and environment rather than copying donor totals.
- [ ] 7.2 Exercise controlled baseline profile copies across schema 6/7/8 and a restart using a safely isolated credential setup; verify both Steam metadata and existing port registrations/jobs/provenance are preserved, rollback uses a schema-compatible binary and no live profile/keyring is mutated.
- [ ] 7.3 Build and verify the Linux release package and smoke-run it from its packaged location with an isolated profile; verify desktop startup, Steam launch, missing-plugin degradation and normal Quit work without developer-tree executables, and record package identity and any remaining native-only cases.

## 8. Native Linux acceptance and confirmation

- [ ] 8.1 Validate real Steam login/Guard where applicable, authenticated-page extraction, focus/cancel/close during cookie collection, restart restore and credential backend behavior on the Linux candidate; verify no panic, unintended browser/profile launch or late credential save, and record which checks used Secret Service versus isolated private-file fallback.
- [ ] 8.2 Validate native/Flatpak discovery where available, multiple libraries, complete owned/Family results, Play/Install, account replacement/sign-out, cached failure recovery and preserved matches/favorites/covers/selection; verify observed mainline parity and list any required scenario still lacking live evidence.
- [ ] 8.3 Validate Linux responsiveness, plugin crash/restart, missing endpoint/executable, login profile cleanup, child exit and actual plugin/single-instance socket file removal on Quit; verify applicable GameNode login/settings/library regression flows and retain Linux's existing tracking and PC ports platform limitations.
- [ ] 8.4 Present the packaged Linux candidate and parity evidence to the user and record explicit confirmation that it works locally; verify Windows VM acceptance is scheduled only after that confirmation and keep final readiness pending until section 9 passes.

## 9. Native Windows VM acceptance

- [ ] 9.1 After Linux confirmation and user-provided VM availability, build/package/install the Windows candidate and execute transport/lifecycle checks; verify protected pipe ownership/recreation, remote-client rejection, busy-pipe reconnect, multiple launches, paths with spaces, missing-plugin degradation, tray/hide/Quit and responsive ports operation during Steam failure, distinguishing cross-build results from executed tests.
- [ ] 9.2 Validate real Windows Steam login/Guard, close/cancel, Credential Manager restore/rotation/deletion failures, registry/multiple-drive discovery, owned/Family catalog, account switching/sign-out and Play/Install; verify baseline behavior and record native outcomes for each account/session scenario.
- [ ] 9.3 Validate Windows gameplay tracking, hidden-window and external launches, Store/Internal switching, startup/scan/exit counter requests, attribution, pending totals and deduplication; verify unchanged measured-session submission policy and existing manual-game/backup/tool behavior.
- [ ] 9.4 Run Windows ports acceptance on the integrated candidate: release-only/guided/package installs with suitable user-owned inputs, update merge, progress, cancel/Quit/restart recovery, missing-executable follow-up, remove/re-add, offline launch, launch environment and gameplay roles; verify Steam sign-out/failure does not affect these flows and record required scenarios without evidence as unresolved.

## 10. Integration readiness report

- [ ] 10.1 Re-run checks affected by acceptance fixes and complete `docs/steam-plugin-integration.md` with baseline/candidate/package identities, both native gates, predecessor mapping and remaining deviations; verify every required Steam, ports, data-preservation and unrelated-mainline scenario has evidence before declaring merge readiness.
- [ ] 10.2 Review the final diff against selected mainline and validate this change strictly; verify only Steam and required infrastructure were added, PC ports remains in the application, deferred work is explicitly retained and documentation describes the delivered API without claiming unresolved predecessor tasks completed.

## Workflow follow-up

- Request commit, push and merge actions separately; this proposal performs none of them.
- After implementation acceptance and review, synchronize the consolidated plugin/provider deltas once through the requested OpenSpec workflow; preserve mainline ports specs and reconcile predecessor supersession without archiving deferred package-manager/settings work as completed.
- Archive only after applicable review and native acceptance requirements are satisfied, then verify the resulting main specs retain all required mainline behavior.
