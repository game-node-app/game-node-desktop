# Tasks

This follow-up is part of the delivery split documented in `../add-third-party-plugins/delivery-split.md`. Source task IDs are retained for traceability.

- [x] 1.1 **Source 12.1.** Define typed stable domain/report bases, versioned namespaced Struct schemas, opaque byte envelopes and get/list/watch access; add a typed per-query selector for category grants and independently selected record grants that defaults to excluded, then verify both authorization paths plus sensitive/custom broker projections.

- [x] 1.2 **Source 12.2.** Implement bounded publication/session retention with host-assigned source/developer/origin/publisher/version/package provenance, schema versions, sequence and TTL; verify forged attribution, duplicate/out-of-order records and oversized payloads produce explicit outcomes.

- [x] 1.3 **Source 12.3.** Implement default-deny common-data category read grants selected during activation and separate sensitive/custom source-to-recipient field/history/live-sharing policies; verify grants permit but do not trigger queries, explicit common queries cover all publishers within scope, and denied queries leave private data available.

- [x] 1.4 **Source 12.4.** Implement domain queries and Data projections/subscriptions over consistent authorization with revocation/cursor fencing, bounded queues and onward provenance; return the DataList snapshot revision for gap-free DataWatch resumption; verify neither alias can recover filtered fields and a two-plugin exchange shares only approved resources/data.

- [ ] 1.5 **Source 12.5.** Document each dataset/method/permission/projection and safe non-shareable exclusions; after task 1.27, verify the published producer-consumer contract can be followed through public APIs without source-private RPCs or Core dashboards.
  - Review (2026-11-18): The shared-data guide exists. Public API interoperability and documentation of supported domain routes still depend on 1.27.


- [ ] 1.6 **Source 12.6.** Implement structural Struct validation, size/depth/key budgets and namespace/field projection; verify metadata cannot shadow typed execution/ownership/grant fields and unknown optional schemas do not invalidate usable base resources.
  - Review (2026-11-18): `6330a94` adds broker-side Struct budgets and reserved field rejection. Verify namespace projection and preservation of base resources with unknown optional schemas before closing.

- [ ] 1.7 **Source 12.7.** Implement revision-checked typed field masks and owner-authorized namespace patches; verify old-client updates preserve unfamiliar namespaces and read-only/foreign/stale writers cannot overwrite resource data.


- [ ] 1.8 **Source 13.2.** Add shared-emulator-resource declarations and backing-resource coordination for backup/restore; verify changing a memory card scope requires full affected-game consent and active emulator use blocks unsafe restore.

- [ ] 1.9 **Source 15.1.** Implement StorageDiagnostics benchmark/SMART report ingestion and projected retrieval with volume binding, tool/version, units/conditions and explicit unavailable/error states; verify no benchmark/SMART subprocess is launched by Core and replacement disks do not inherit old reports.

- [ ] 1.10 **Source 15.2.** Implement Computer hardware and GamePresence publish/clear/query/watch with versioned fields, safe identities, game/session attribution and expiry; verify absent identifiers, denied sharing, stale producer data and presence not triggering automatic online publication or process-state decisions.

- [ ] 1.11 **Source 15.3.** Implement bounded GamePerformance batches/end-series and retained query/aggregation with FPS/CPU/RAM semantics, computer/game/runtime context, source and sample coverage; verify gaps, duplicate/out-of-order sequences, normalization, quotas and backpressure without Core capture hooks or prediction algorithms.

- [ ] 1.12 **Source 15.4.** Document collection-versus-transport responsibilities and performance schema examples; verify all three data families operate between producer/consumer fixtures without any Core dashboard, hardware scanner or game instrumentation plugin bundled in the host.


- [ ] 1.13 **Source 16.1.** Add independent default-deny storage.persist, owner-attributed durable stores keyed by the 4.4 namespace resolver and explicit durability/expiry responses; verify receiver persistence additionally needs retention consent, cross-plugin/profile isolation and missing grants never silently downgrade a durable write.
  - Review (2026-11-18): Private durable Storage already checks storage.persist. Recipient retention consent and the shared-data durability path remain open.

- [ ] 1.14 **Source 16.2.** Implement logical-only Persistence usage/list/delete/clear/retention-policy operations with authenticated owner resolution; verify foreign ids/cursors, traversal-like selectors, forged plugin ids and generic Files access cannot address another namespace or reveal physical persistence paths.

- [ ] 1.15 **Source 16.3.** Implement per-plugin/category logical/physical accounting, quotas and bounded performance-history retention with user-protected records; verify quota exhaustion preserves history, cleanup reclaims only owner data and physical/index overhead estimates are labelled accurately.

- [ ] 1.16 **Source 16.4.** Implement uninstall keep/delete choice and inert archived-data reclaim/reuse with preserved source/version provenance; verify no implicit access on reinstall/new consumer and deletion never affects game files, live saves or backup archives.

- [ ] 1.17 **Source 16.5.** Implement own-namespace automatic cleanup policy within user limits and reviewed host-wide cleanup for unused/large datasets, coordinated with active readers/writers; verify deletion events/tokens, protected data, partial failures and measured reclaimed space, and document API isolation versus native OS access.


- [x] 1.18 **Source 17.1.** Add activation-time common-data category read controls and separate sensitive-data source/recipient field/history/delivery/retention review, explaining that grants do not trigger per-query use; verify all start off, private-only query defaults and common lookup has no mandatory transfer wizard in en/pt-BR component tests. This task covers the review UI; persistence and enforcement belong to task 1.27.

- [ ] 1.19 **Source 17.2.** Add host-only physical-path storage accounting, uninstall retain/delete and cleanup preview/policy screens; verify plugin IPC cannot access these host administrative paths and slow accounting/deletion remains off the UI thread.

- [ ] 1.20 **Source 17.3.** Extend both Go/Rust examples with fixture producer-consumer exchanges, performance history/cleanup, save/location inspection and multiple content identities under a shared runtime; verify parity, no real collector dependency, no physical managed paths and explicit denied/granted/revoked flows.


- [ ] 1.21 **Source 17.4.** Add matching Go/Rust hybrid-record, opaque-byte and custom-dataset examples with schema-version/unknown-namespace handling; verify equivalent field preservation, projection and unsupported-feature outcomes, and document which values Core interprets.


- [ ] 1.22 **Source 18.6.** Exercise the combined two-plugin data flow: selected per-game read/write, private-only query defaults, explicit performance-history inclusion under a category grant, separately authorized persistent history, producer uninstall, category-scoped archived-data reuse and own/global cleanup; record no leakage across games/namespaces and clear native-process API-only disclosure.


- [ ] 1.23 **Source 18.7.** Verify the entire endpoint matrix against generated contract fixtures and both clients, including per-query common-data selectors and defaults, same-console ROM/runtime availability projections, common category grants versus sensitive/custom sharing, unknown metadata preservation, rejected Struct authority injection and unchanged official 1.1–1.5 behavior; record coverage without treating annotation exchange as execution approval.


- [ ] 1.24 **Source 19.3.** Implement producer-requested, user-authorized public-pool publication distinct from private storage and independent read/create/update/delete grants with both category-wide and selected-record scopes; verify revision conflicts, provenance, revocation, separate persistence and identical domain/Data enforcement.

- [ ] 1.25 **Source 19.4.** Add installation/settings controls for public-pool publication, operation and category/record scopes; verify default denial and no implicit permission expansion.

- [x] 1.26 **Source 9.4.** Implement update review that rejects feed downgrades, supports only explicitly confirmed user-requested downgrades through the verified-package path, and requires separate re-trust of the exact changed publisher fingerprint; keep the plugin/publisher/profile data namespace stable across versions with the same publisher key, require versioned logical keys or schema IDs for incompatible private-data changes, preserve older values for rollback, and intersect current grants with the selected package manifest; verify feed downgrade/key changes never activate silently and rollback preserves compatible data without restoring broader grants.

- [x] 1.27 **Shared-data policy integration.** Build on `add-third-party-plugins` tasks 10.2/10.4 for verified plugin identity, profile-bound activation and generic permission inventory; do not duplicate that lifecycle. Connect common-category and selected-record review choices to persisted `SharedSharingPolicies` grants keyed by host-authenticated plugin/publisher/profile identity, and add host-controlled exact source/recipient/dataset review for sensitive/custom fields and history/live delivery. Keep read grants separate from recipient persistence/retention consent and prevent plugin RPCs from creating or widening policy. Verify grants reload only for the same identity/profile but still require explicit query selectors, separate pair/dataset/field/delivery rules, revocation fences queued watches and cursors, and a two-plugin producer/consumer flow works through public APIs without filtered-field leakage. Task 1.5's end-to-end contract verification depends on this task.
  - Async Tauri snapshot and replace commands use the verified package, active profile and policy revision. Settings reviews common grants and exact source/recipient/dataset fields and delivery modes in en/pt-BR.
  - Authenticated producer and consumer exercised public ResourceHost Data RPCs with a reopened policy store. Verified explicit common selectors, profile isolation, exact custom datasets and field projection, independent history/live delivery, and revocation of queued watch events and cursors. DataPublish rejects persistent publication and private Storage requires its independent storage.persist permission. Recipient retention consent and managed durable shared copies remain tracked by 1.13.
  - Durable policy commits are ordered against profile and publisher transitions. Five focused regressions reopened the store after a profile switch, a switch back to the original profile, activation of a package with another publisher, publisher re-trust during import, or a save that committed before the profile switched. Rejections preserved the previous policy and revision. The successful save returned its committed revision after the switch. A focused UI regression verified the late response stayed in the reviewed publisher's cache.
  - Focused validation passed 8 ResourceHost Data, 15 sharing-policy, 7 profile and package lifecycle, and 5 publisher-related UI tests, in addition to the 5 commit regressions. The desktop binary check and TypeScript typecheck passed. The earlier round also passed 10 broker and 37 focused UI tests.

- [ ] 1.28 **Reviewed publisher-key data migration.** After 1.26 re-trust and MVP 10.2/10.4 identity and review, add host-controlled preview and explicit migration of compatible data from the old publisher identity to the new one. Preserve original provenance and source data; report incompatible schemas, conflicts and partial failures; never migrate secrets or grants automatically. Verify rejection, cancellation, profile isolation, rollback and separate authorization of new sharing and retention.

Tasks 1.5 and end-to-end shared-runtime projections depend on 1.27; 1.27 depends on MVP 10.2/10.4. Backup coordination 1.8 depends on actions 1.5; shared-runtime examples depend on runtime identity and runtime 1.10.
