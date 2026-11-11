# Tasks

This change evolves the Steam integration baseline consolidated in main `desktop-plugins`, `desktop-plugin-services` and `desktop-providers`. Preserve its surviving contracts while adding managed third-party behavior. Steam integration native acceptance remains tracked by `integrate-steam-plugin-into-main` and `docs/steam-plugin-integration.md`; these tasks do not mark those pending checks complete.

## 1. Public licensing and official compatibility baseline

- [x] 1.1 Record the consolidated main plugin/service/provider specs, current official descriptors, grants, credential mappings, packaging and API 1.1–1.3 outcomes as regression fixtures; verify those fixtures pass before introducing third-party paths and distinguish the delivered baseline from pending native acceptance.
- [x] 1.2 Review provenance of SDK, protocol and example source; deliver a file-level licensing-authority record and resolve unauthorized material before MIT distribution, preserving existing third-party notices.
- [x] 1.3 Add explicit MIT notices/licenses for approved public assets and retain GPL-3.0-only on host/Core; verify package metadata and dependency closures and document the public-only licensing boundary.

## 2. Public API 1.4 and optional SDK

- [x] 2.1 Add backward-compatible third-party version-range/feature negotiation, provider/extension session kinds, scoped target-game contexts and support-window metadata to the public protocol; verify lower/higher overlap, required/optional unavailable features and unchanged official handshakes with contract tests.
- [ ] 2.2 Define public RPC/message contracts for every row of design section 4, including operation ownership, pagination, revisions, quotas, idempotency and separate game.launch/game.custom_launch contracts; verify protobuf generation and contract fixtures while documenting method-to-permission mappings.
- [x] 2.3 Add DescribeApi/GetPermissions/WatchPermissionChanges and structured denial details; verify SDK-independent decoding distinguishes denied, unauthenticated, stale, unsupported, invalid and unavailable outcomes.
- [x] 2.4 Extend the optional Rust SDK with detailed errors, new service clients and configuration lifecycle dispatch without breaking existing official call sites; verify SDK client fixtures and unchanged official builds.
- [ ] 2.5 Document the complete transport/session/Exchange lifecycle, credentials, limits, cancellation and existing-versus-new RPCs in public API guides; verify every existing method/event family and every new contract has a reference entry.

- [ ] 2.6 Turn design section 18 into a per-RPC request/response/extension/access contract fixture and reference docs; verify every existing/proposed route and Exchange operation has a classification and official wire fixtures remain compatible.
- [ ] 2.7 Add typed ExtensionPayload/namespace patches and bounded packaged schema descriptors to protocol/SDK without introducing dynamic host dispatch; verify precise 64-bit fields stay typed and Go/Rust preserve unselected application extension namespaces.

## 3. Package format, trust and managed registry

- [x] 3.1 Implement manifest schema 1 validation, including provider/extension kinds, optional provider identity, platform entrypoints, API range, scoped permissions, configuration modes and SPDX/LicenseRef notices; verify valid and malformed package fixtures and publish the manifest reference.
- [x] 3.2 Implement bounded ZIP staging and extraction rejection for traversal, links/reparse points, duplicates, case collisions, unlisted payloads and wrong architecture; verify hostile-archive tests cannot write outside staging.
- [ ] 3.3 Implement SHA-256 payload verification and domain-separated Ed25519 integrity signatures, including pre-launch checking; verify changed binaries/manifests/notices and invalid keys are rejected and document the signature byte format.
- [x] 3.4 Add persistent per-user package registry, immutable version paths, active pointer and transaction recovery; verify restart/interruption cases preserve either a complete valid version or inactive state.
- [ ] 3.5 Implement initial publisher fingerprint confirmation, identity pinning and explicit re-trust for key changes; verify a valid signature alone cannot establish official attribution or silently replace a pinned publisher.
- [ ] 3.6 Deliver package-building/signing/validation tooling and format documentation with clearly marked development keys; verify both target package fixtures round-trip without executing installation hooks.

- [x] 3.7 Validate packaged namespace/schema ownership, versions, classifications and bounded declarative schemas without remote references/code; verify malformed/missing assets and namespace collisions fail package validation with actionable errors.

- [x] 3.8 Extend signed manifest target schema/reference with explicit OS/architecture/ABI, Windows version/build/product and Linux libc/distribution constraints plus separate author-tested environments; verify invalid bounds, unknown required selectors, contradictory ABI, distribution alternatives and ambiguous selection fixtures.
- [ ] 3.9 Implement minimal Core/platform detection of the effective plugin execution environment on background workers, independent of detector plugins; verify os-release identity, libc/ABI/version, Windows client/server/build, unknown facts, isolation boundaries and explicit emulation policy without executing candidate code.
- [ ] 3.10 Implement typed compatibility verdicts/reasons and requirement evaluation at validation, update selection/activation and pre-launch; verify compatible/incompatible/indeterminate, informational tested coverage, environment changes, failed-update recovery and preserved package/data state without fallback execution.

## 4. Permission persistence and enforcement

- [ ] 4.1 Implement third-party default-deny grants with independent read/write/delete, required/optional flags, scopes and revisions; map runtime/library-root/save-profile methods to read/write/delete rather than configure, and verify own-creator scope, selected foreign resource/operation consent, immutable provenance, acting-plugin audit, revocation, save/restart and persistence-failure rollback. Preserve the explicit storage.persist own-cleanup exception without bypassing Storage.Delete or foreign namespace isolation.
- [ ] 4.2 Enforce grants and identity/profile/session/operation scope for unary services and Exchange messages; verify explicit denial details and rejected submissions preserve previous successful facts.
- [ ] 4.3 Implement revocation notifications, affected-job cancellation and authorization checks at effect commit after queued work/confirmations; verify races cannot commit newly unauthorized changes.
- [ ] 4.4 Isolate account identity, secret slots, storage and jobs by external identity/profile; verify Core/Steam aliases and foreign plugin resources are inaccessible through the API.
- [ ] 4.5 Add bounded redacted authorization audit events and permission-reference docs describing API/native differences; verify captured logs contain no credentials, cookie values or raw request payloads.

## 5. Library and presentation adapters

- [ ] 5.1 Implement Library list/get/watch with own-provider filtering, separately granted whole-library reads and bounded events/cursors; verify cross-provider disclosure and stale cursor cases with domain fixtures.
- [ ] 5.2 Implement validated display-metadata and favorite read/write adapters with resource revisions; verify idempotent writes, conflicts and rejection of foreign-provider mutations and document writable fields.
- [ ] 5.3 Implement bounded matching search/assignment through existing Core policies; verify ownership, unavailable catalog and match-conflict outcomes with fixtures and document result semantics.
- [ ] 5.4 Implement artwork read/set with bounded input, approved resource references and worker-side decoding/resizing; verify malformed images, oversized data and unauthorized targets cannot change library artwork.
- [ ] 5.5 Add SDK convenience methods and practical read/write recipes for these adapters; verify equivalent raw gRPC and SDK outcomes against fixture-host service tests.

## 6. Scoped storage, files, network and notifications

- [ ] 6.1 Implement namespaced storage get/put/delete, explicit SESSION_ONLY/PERSIST durability, quotas and revisions; verify storage.persist denial, atomic quota failures and isolation, and document non-secret versus Secret storage without exposing physical paths.
- [ ] 6.2 Implement user-initiated per-plugin/per-game root approval and opaque file handles with list/read/write/delete, disabled at installation; verify sibling-game denial, reserved persistence-root exclusion, shared-resource scope, traversal, symlink/junction replacement and independent mutation permissions.
- [ ] 6.3 Implement constrained HTTPS fetch with origin grants, redirect/DNS revalidation, size/deadline bounds and no Core credentials; verify private/local destinations and widened origins fail before data disclosure.
- [ ] 6.4 Implement bounded plugin-attributed notifications without executable markup or arbitrary links; verify rate limiting, localization fallback and untrusted text rendering.
- [ ] 6.5 Document utility scopes, limits, errors and direct-native-access limitations with SDK examples; verify the published examples against the corresponding fixture services.

## 7. Host-owned action and job adapters

- [ ] 7.1 Implement caller-owned jobs with get/watch/cancel, idempotency and terminal outcomes for long operations; verify retries, foreign ids, cancellation and permission changes cannot duplicate effects.
- [ ] 7.2 Implement game.launch using detected URI/executable targets from any source within per-game scope, plus the existing independent install flow; verify overrides are rejected, explicit user intent is required and official playtime/launch behavior is preserved.
- [ ] 7.3 Implement scoped backup list/create/restore through approved game/save-context profiles across providers without faking manual-provider ownership, with restore confirmation; verify target validation, cancellation and unsupported-platform results and document filesystem effects.
- [ ] 7.4 Implement tools list/apply/remove through the existing host catalog and rollback/confirmation flow; verify plugins cannot supply arbitrary installer targets or bypass prerequisites.
- [ ] 7.5 Implement ports list/detail/install through host catalog/planning/pickers with Windows availability checks; verify Linux install is explicitly unavailable and supplied arbitrary URLs are rejected.
- [ ] 7.6 Add SDK methods and documented confirmation/job recipes for action services; verify permission revocation while a confirmation is open prevents execution after confirmation.

- [ ] 7.7 Implement managed detected-executable candidate registration and target approval/revision persistence, retaining legacy URI DeclareActions; verify disc-installed games can launch after approval while unreviewed scans cannot execute code and changed/missing paths offer repair.
- [ ] 7.8 Implement game.custom_launch recipe propose/request/remove with alternate executable/URI, argument vectors, working directory, approved external runtime handles and environment set/unset; verify actual worker-spawned fixture processes receive every field and incapable adapters/URI handlers fail explicitly rather than ignoring environment.
- [ ] 7.9 Implement per-game/profile plugin-owned overlays for official, third-party and manual games, with independent normal/custom grants and one-use host user intents; verify an extension can target a selected Steam game without provider impersonation, broad library reads or modification of official state.
- [ ] 7.10 Fence approvals to recipe/base-target/resource revisions, sanitize child environment and associate wrapper/child process attribution with the original game; verify revocation and changed recipes block new launches without killing running games, leaking session credentials or silently switching routes.

- [ ] 7.11 Build immutable typed LaunchPlan resolution and versioned canonical approval fingerprints, binding source/package/component identities, effective target/argv/cwd/env and resource/grant revisions; verify plan substitution, replay, expiry and changed known mod/runtime components cannot dispatch through an old approval, with hashing/revalidation off the UI thread.

## 8. External provider and configuration lifecycle

- [ ] 8.1 Compose external provider backends and provider-free launch extensions beside the official registry, reserving official ids/namespaces; verify valid registered scanner/catalog/counter backends are accepted, operations exceeding their approved descriptor are rejected and collisions, missing provider contexts and invalid policies cannot impersonate or change official registration.
- [ ] 8.2 Add external secret/account/login service composition with approved origins/cookies and host-defined page probes; verify no third-party credential alias or arbitrary privileged-page script is accepted.
- [ ] 8.3 Implement start/enable/disable/remove and bounded process-tree shutdown with durable state and session fencing; verify crashes, timeouts, restart and removal preserve library facts and official operation.
- [ ] 8.4 Add ui.configure and correlated Open/Focus/Close lifecycle results with one window handle per plugin/profile; verify repeated Configure focuses, five-second acknowledgements time out and provider commands remain serviceable.
- [ ] 8.5 Handle window closure/fencing on profile change, revocation, disable, update and removal; verify stale window contexts cannot access new profile data or grant permissions.
- [ ] 8.6 Connect optional declarative ui.settings rendering with its existing closed schema and explicit authorization, preferring external configuration when both are available; verify external-only, form-only, both, denied and absent modes.
- [ ] 8.7 Document lifecycle, process-supervision limits and plugin-owned UI/storage responsibilities; verify lifecycle fixtures distinguish management from functional configuration and do not embed plugin code into the GameNode frontend.

## 9. Updates and recovery

- [ ] 9.1 Implement signed expiring update indexes, monotonic sequence checks and validated HTTPS package retrieval; verify invalid signatures, expiry, replay, redirects, interrupted downloads and offline checks preserve the active version.
- [ ] 9.2 Implement opt-in periodic checks and manual approval with permission/scope/native-access/license/API diffs; verify checks never execute code and new/wider grants remain denied without consent.
- [ ] 9.3 Implement staged activation, readiness timeout and last-known-good rollback using current grants; verify failed activation and interruption cannot restore revoked privileges or execute partial packages.
- [ ] 9.4 Implement explicit downgrade and publisher-key re-trust review, with versioned plugin-private data policy; verify automated feeds cannot silently downgrade or change identity and document rollback-safe storage requirements.

## 10. Settings integration

- [ ] 10.1 Add asynchronous Tauri commands and React Query inventory/permission/update/configuration cache updates; verify grant revisions and saved host state remain authoritative after errors and concurrent events.
- [ ] 10.2 Extend Stores PluginInventory with import, activation, disable, permission scopes and removal using shared controls/modals; verify keyboard behavior, denied states and unchanged official plugin details in component tests.
- [ ] 10.3 Add Configure routing and lifecycle status, with external-window preference and optional form fallback; verify unavailable/disabled modes, focus requests, timeout feedback and independent settings navigation.
- [ ] 10.4 Add publisher fingerprint, requested/granted/effective access, native-code consent and offline license viewers; verify notice availability without network and accurate distinction between declared and enforced access.
- [ ] 10.5 Add update diff, failure and rollback status plus complete en/pt-BR labels and mocks; verify slow operations leave unrelated UI usable and restore controls to actual saved state on persistence errors.

- [ ] 10.6 Add per-game launch recipe selection and explicit return to detected default using the shared host launch-plan review for runtime/emulator, mod-loader/layer and custom target/parameter changes; verify unchanged approved Play needs no repeated prompt and changed/revoked plans require an actionable choice in both locales.

- [ ] 10.7 Implement the shared Launch plan / Plano de lançamento review with source provenance, ordered known components, complete literal arguments/paths/environment delta and differences; verify Cancel default, Run once, exact-recipe remembered approval, explicit secret reveal, accessibility, denied-request no-bypass behavior and scope-not-safety wording in en/pt-BR.

- [ ] 10.8 Show target requirements, detected effective environment, compatibility verdict/reasons and separate author-tested coverage in import/details/update review; verify glibc/musl mismatch, unknown mandatory facts and compatible-but-untested activation states in en/pt-BR component fixtures.

## 11. Standalone SDK distributions and two examples

- [ ] 11.1 Prepare versioned public SDK/protocol source archives with usable external dependency metadata and pinned tooling; verify clean-directory builds without workspace-internal path dependencies and document the release process.
- [ ] 11.2 Create examples/plugins/rust-sdk as a runnable separate-identity MIT plugin using the SDK, including provider and launch-extension fixture profiles; verify session/commands/discovery/catalog/counter, protected read/write, denial, fallback and cancellation against fixture host.
- [ ] 11.3 Create examples/plugins/go-grpc with pinned protobuf generation, provider and launch-extension fixture profiles and no GameNode SDK dependency; verify direct Linux Unix-socket and Windows named-pipe dialers and session/command/result interoperability.
- [ ] 11.4 Implement a real plugin-owned configuration window in both examples with focus/close and one persistent non-secret preference; verify lifecycle through fixture tests and provide native run instructions demonstrating concurrent provider work.
- [ ] 11.5 Add runnable fake account/secret/login/action/settings recipes covering all existing RPC/event families and a new Core read/confirmed write; verify equivalent Go/Rust outcomes without real Steam credentials or secret-bearing logs. Include standard URI/local-executable and custom argument/environment/runtime recipes with extension-only profiles and explicit approval.
- [ ] 11.6 Supply per-example manifests, notices, package/sign commands and standalone build/install/run guides; verify package import and consent flows through the same production validators.
- [ ] 11.7 Add conformance checks for generated-code freshness, public dependency boundaries, supported-version fallback and both examples; verify they fail for stale bindings or internal Core imports and document contribution expectations.

- [ ] 11.8 Add equivalent safe Go/Rust launch-consent fixtures for emulators/runtimes, known mod layers and environment-only customization; verify both clients use host review and cannot replace approved fields or treat launch consent as installation/write permission.

- [ ] 11.9 Publish target compatibility examples and deterministic evaluator fixtures for Windows builds/products, Debian release constraints, glibc/musl versions, absent allowlists, interpreted-plugin native dependencies and unknown/ambiguous inputs; document that provisioning/install hooks are outside this decision.

## 12. Shared data plane and consent

- [ ] 12.1 Define typed stable domain/report bases, versioned namespaced Struct schemas, opaque byte envelopes and get/list/watch access; add a typed per-query common-data selector that defaults to excluded and verify category grants plus sensitive/custom broker projections.
- [ ] 12.2 Implement bounded publication/session retention with host-assigned source/developer/origin/publisher/version/package provenance, schema versions, sequence and TTL; verify forged attribution, duplicate/out-of-order records and oversized payloads produce explicit outcomes.
- [ ] 12.3 Implement default-deny common-data category read grants selected during activation and separate sensitive/custom source-to-recipient field/history/live-sharing policies; verify grants permit but do not trigger queries, explicit common queries cover all publishers within scope, and denied queries leave private data available.
- [ ] 12.4 Implement domain queries and Data projections/subscriptions over consistent authorization with revocation/cursor fencing, bounded queues and onward provenance; verify neither alias can recover filtered fields and a two-plugin exchange shares only approved resources/data.
- [ ] 12.5 Document each dataset/method/permission/projection and safe non-shareable exclusions; verify the published producer-consumer contract can be followed without source-private RPCs or Core dashboards.

- [ ] 12.6 Implement structural Struct validation, size/depth/key budgets and namespace/field projection; verify metadata cannot shadow typed execution/ownership/grant fields and unknown optional schemas do not invalidate usable base resources.
- [ ] 12.7 Implement revision-checked typed field masks and owner-authorized namespace patches; verify old-client updates preserve unfamiliar namespaces and read-only/foreign/stale writers cannot overwrite resource data.

## 13. Save profiles and location/storage inspection

- [ ] 13.1 Add plugin-contributed save-profile CRUD with independent read/write/delete, plugin-defined directories/naming rules, optional versioned format/compatibility descriptors, independent runtime save contexts, scope previews and revisions; verify distinct emulator save folders for one ROM, own-scope/explicit foreign grants, denied file access and preserved history without requiring save parsing/conversion.
- [ ] 13.2 Add shared-emulator-resource declarations and backing-resource coordination for backup/restore; verify changing a memory card scope requires full affected-game consent and active emulator use blocks unsafe restore.
- [ ] 13.3 Implement common size/access/last-modified inspection for save/installation/runtime handles; verify logical byte totals, overlapping-root deduplication, read-only versus effective ACL states, unknowns and timestamp precision without modifying source files.
- [ ] 13.4 Implement fresh streaming SHA-256/tree fingerprints and snapshot comparison as cancellable bounded worker jobs; verify same-size/same-mtime content changes, added/deleted files, empty directories, unreadable files and concurrent writes never yield a false complete unchanged result.
- [ ] 13.5 Implement platform location/volume metadata and availability events with separate local/network, external/removable, interface/media dimensions and unknown evidence; verify replaced-volume identities cannot inherit approval and disconnected mounts preserve game/profile/history state.
- [ ] 13.6 Document inspection consistency, per-game permissions, engine/platform availability and non-diagnostic media hints; verify fixtures distinguish unavailable storage from missing/deleted saves and never create replacement source directories while unmounted.

- [ ] 13.7 Document the future save-descriptor roadmap: artifact kind, format/version/size constraints, content binding, compatibility evidence, slots/containers, observed last-loaded events, image provenance and import/conversion lineage; use illustrative metadata fixtures to distinguish unknown fields and save states from in-game saves. Explicitly defer save browsers, collectors, parsers, slot editors and converters; add no successful stub endpoints or mandatory emulator tooling.

## 14. Libraries and runtime-hosted game identity

- [ ] 14.1 Expose logical library CRUD/root operations/list/get/watch backed by existing multi-root monitoring; verify root overlap/alias deduplication, independent unavailable roots and removal without filesystem deletion, and document scope approval.
- [ ] 14.2 Add approved runtime/interpreter resources and per-content game/installation bindings to structured launch recipes; verify several games share a runtime and one registered ROM/GameNode game offers several runtimes without duplicate game identity, lost history or save changes. Cover moving/renaming content, distinct variants, binding migration and runtime removal without cascading file/game/save deletion.
- [ ] 14.3 Define and implement the common typed runtime-session event/result family over Exchange under default-deny per-game/runtime game.sessions.report, with plugin-owned adapters and Core arbitration; verify same-PID content switches, concurrent instances, duplicate evidence, PID reuse, boot failure, stale/disconnected/revoked reports, unknown gaps and fresh confirmation after restart. Preserve direct-executable, account, source, checkpoint and platform policies, and apply Core-owned pause rules. Document that one plugin may support multiple runtimes and that descriptive presence cannot establish sessions; test consented Data projections without exposing handles or credentials.

- [ ] 14.4 Exercise A registering an owned runtime with a custom compatibility extension and B querying it through Runtimes.List/Get/Watch under the applicable category read permission; verify the private-only default, explicit common-data opt-in, same-console ROM/runtime availability through Library.List/Get/Watch without file paths or bytes, no pairwise grant and no mutation/execution authority.

- [ ] 14.5 Add negotiated scoped runtime-content discovery through Games.RegisterContent for runtime plugins/extensions under library.discovery without provider impersonation; verify ZSNES/bsnes observations reconcile one registered game/content with selectable bindings, discovery grants no execution, and DeSmuME-to-melonDS binding migration preserves history and save references without requiring foreign deletion.

## 15. Plugin-produced diagnostic and gameplay data

- [ ] 15.1 Implement StorageDiagnostics benchmark/SMART report ingestion and projected retrieval with volume binding, tool/version, units/conditions and explicit unavailable/error states; verify no benchmark/SMART subprocess is launched by Core and replacement disks do not inherit old reports.
- [ ] 15.2 Implement Computer hardware and GamePresence publish/clear/query/watch with versioned fields, safe identities, game/session attribution and expiry; verify absent identifiers, denied sharing, stale producer data and presence not triggering automatic online publication or process-state decisions.
- [ ] 15.3 Implement bounded GamePerformance batches/end-series and retained query/aggregation with FPS/CPU/RAM semantics, computer/game/runtime context, source and sample coverage; verify gaps, duplicate/out-of-order sequences, normalization, quotas and backpressure without Core capture hooks or prediction algorithms.
- [ ] 15.4 Document collection-versus-transport responsibilities and performance schema examples; verify all three data families operate between producer/consumer fixtures without any Core dashboard, hardware scanner or game instrumentation plugin bundled in the host.

## 16. Managed persistence and cleanup

- [ ] 16.1 Add independent default-deny storage.persist, owner-attributed durable stores and explicit durability/expiry responses; verify receiver persistence additionally needs retention consent and missing grants never silently downgrade a durable write.
- [ ] 16.2 Implement logical-only Persistence usage/list/delete/clear/retention-policy operations with authenticated owner resolution; verify foreign ids/cursors, traversal-like selectors, forged plugin ids and generic Files access cannot address another namespace or reveal physical persistence paths.
- [ ] 16.3 Implement per-plugin/category logical/physical accounting, quotas and bounded performance-history retention with user-protected records; verify quota exhaustion preserves history, cleanup reclaims only owner data and physical/index overhead estimates are labelled accurately.
- [ ] 16.4 Implement uninstall keep/delete choice and inert archived-data reclaim/reuse with preserved source/version provenance; verify no implicit access on reinstall/new consumer and deletion never affects game files, live saves or backup archives.
- [ ] 16.5 Implement own-namespace automatic cleanup policy within user limits and reviewed host-wide cleanup for unused/large datasets, coordinated with active readers/writers; verify deletion events/tokens, protected data, partial failures and measured reclaimed space, and document API isolation versus native OS access.

## 17. Administration and interoperability coverage

- [ ] 17.1 Add activation-time common-data category read controls and separate sensitive-data source/recipient field/history/delivery/retention review, explaining that grants do not trigger per-query use; verify all start off, private-only query defaults and common lookup has no mandatory transfer wizard in en/pt-BR component tests.
- [ ] 17.2 Add host-only physical-path storage accounting, uninstall retain/delete and cleanup preview/policy screens; verify plugin IPC cannot access these host administrative paths and slow accounting/deletion remains off the UI thread.
- [ ] 17.3 Extend both Go/Rust examples with fixture producer-consumer exchanges, performance history/cleanup, save/location inspection and multiple content identities under a shared runtime; verify parity, no real collector dependency, no physical managed paths and explicit denied/granted/revoked flows.

- [ ] 17.4 Add matching Go/Rust hybrid-record, opaque-byte and custom-dataset examples with schema-version/unknown-namespace handling; verify equivalent field preservation, projection and unsupported-feature outcomes, and document which values Core interprets.

## 18. Cross-component acceptance

- [ ] 18.1 Run focused Rust protocol/SDK/host tests, UI component tests and Go example checks, then required workspace checks; record results and resolve integration failures without changing official plugin implementations.
- [ ] 18.2 On native Linux, exercise signed package import, both example transports/windows, grant revocation, scoped file/keyring access, process shutdown, update and rollback; record acceptance evidence and any reproducible platform limitations.
- [ ] 18.3 On native Windows, exercise both example named-pipe sessions/windows, ACL/junction behavior, credential isolation, supervised process shutdown and update/rollback; record native evidence separately from cross-build results.
- [ ] 18.4 Confirm official Steam sign-in/discovery/catalog/counter/actions and Linux package discovery remain unchanged with empty, invalid, enabled and disabled third-party registries; verify a missing/incompatible Steam executable reports unavailability, preserves cached data and does not invoke an in-process substitute, retaining previously validated cached actions where inputs remain valid; record compatibility evidence against the baseline fixtures without claiming unresolved predecessor acceptance is complete.

- [ ] 18.5 Exercise detected and custom launches across official, third-party and manual game fixtures, including a Linux Proton-style runtime wrapper and direct executable bypass; record equivalent Go/Rust parameters, original game identity, explicit unsupported-target outcomes and unchanged official provider files/credentials.

- [ ] 18.6 Exercise the combined two-plugin data flow: selected per-game read/write, private-only query defaults, explicit performance-history inclusion under a category grant, separately authorized persistent history, producer uninstall, category-scoped archived-data reuse and own/global cleanup; record no leakage across games/namespaces and clear native-process API-only disclosure.

- [ ] 18.7 Verify the entire endpoint matrix against generated contract fixtures and both clients, including per-query common-data selectors and defaults, same-console ROM/runtime availability projections, common category grants versus sensitive/custom sharing, unknown metadata preservation, rejected Struct authority injection and unchanged official 1.1–1.3 behavior; record coverage without treating annotation exchange as execution approval.

- [ ] 18.8 Exercise common launch-plan consent end to end for runtime, mod and parameter-only overlays on official/third-party game fixtures; record accurate provenance/differences, changed-plan refusal, unchanged remembered Play, no unauthorized operation bypass and explicit limits on native-program behavior certification.

- [ ] 18.9 Record native Windows/Linux environment-detection acceptance separately from pure evaluator fixtures, including execution userspace versus host, pre-launch revalidation and compatibility failure UI; do not treat a successful declaration check as native code safety certification.
